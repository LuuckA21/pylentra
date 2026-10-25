//! Jump host chains: one in-process server relays to the next through direct-tcpip.
mod support;

use std::sync::Arc;
use std::time::Duration;

use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::model::{AuthMethod, Connection};
use pylentra_core::secrets::{MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{CloseReason, PromptAnswer, SessionEvent, Stage};
use pylentra_core::ssh::{SessionManager, SessionOptions};
use support::{CollectSink, TestServer, TestServerConfig};
use tempfile::TempDir;
use uuid::Uuid;

const T: Duration = Duration::from_secs(5);
const PW: &str = "pw";

struct Env {
    _dir: TempDir,
    known_hosts: Arc<KnownHosts>,
    secrets: Arc<MemorySecretStore>,
    mgr: SessionManager,
}

fn env() -> Env {
    let dir = TempDir::new().unwrap();
    let known_hosts = Arc::new(KnownHosts::new(dir.path().join("known_hosts"), None));
    let secrets = Arc::new(MemorySecretStore::default());
    let mgr = SessionManager::new(
        secrets.clone(),
        known_hosts.clone(),
        SessionOptions::default(),
    );
    Env {
        _dir: dir,
        known_hosts,
        secrets,
        mgr,
    }
}

async fn pw_server(single_password_try: bool) -> TestServer {
    TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        single_password_try,
        ..Default::default()
    })
    .await
}

fn conn(port: u16) -> Connection {
    Connection {
        id: Uuid::new_v4(),
        name: "hop".into(),
        group_id: None,
        host: "127.0.0.1".into(),
        port,
        username: "u".into(),
        auth: AuthMethod::Password { save: false },
        jump_via: None,
        forwards: vec![],
        color: None,
        icon: None,
    }
}

/// A hop whose host key is not trusted yet, with `password` stored.
fn hop(env: &Env, port: u16, password: &str) -> Connection {
    let c = conn(port);
    env.secrets
        .set(c.id, SecretKind::Password, &password.to_string().into())
        .unwrap();
    c
}

fn trust(env: &Env, server: &TestServer) {
    env.known_hosts
        .learn("127.0.0.1", server.addr.port(), &server.host_key)
        .unwrap();
}

fn is_ready(e: &SessionEvent) -> bool {
    *e == SessionEvent::Status {
        stage: Stage::Ready,
    }
}

async fn closed_error(sink: &CollectSink) -> pylentra_core::error::ApiError {
    match sink
        .wait_event(|e| matches!(e, SessionEvent::Closed { .. }), T)
        .await
    {
        SessionEvent::Closed {
            reason: CloseReason::Error { error },
        } => error,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn accepted(server: &TestServer) -> Vec<String> {
    server.accepted_auth.lock().unwrap().clone()
}

#[tokio::test]
async fn two_hop_session_reaches_target() {
    let (env, a, b) = (env(), pw_server(false).await, pw_server(false).await);
    trust(&env, &a);
    trust(&env, &b);
    let chain = vec![hop(&env, a.addr.port(), PW), hop(&env, b.addr.port(), PW)];
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(chain, 80, 24, sink.clone());
    sink.wait_event(is_ready, T).await;

    env.mgr.write(id, b"hello".to_vec()).await.unwrap();
    sink.wait_data_len(5, T).await;
    assert_eq!(sink.data.lock().unwrap().as_slice(), b"hello");
    // Both hops authenticated, and the shell lives on the target.
    assert_eq!(accepted(&a), ["password"]);
    assert_eq!(accepted(&b), ["password"]);
    assert_eq!(b.ptys.lock().unwrap().len(), 1);
    assert!(a.ptys.lock().unwrap().is_empty());
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn each_hop_host_key_is_checked() {
    let (env, a, b) = (env(), pw_server(false).await, pw_server(false).await);
    trust(&env, &a);
    let chain = vec![hop(&env, a.addr.port(), PW), hop(&env, b.addr.port(), PW)];
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(chain, 80, 24, sink.clone());

    let prompt = sink
        .wait_event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }), T)
        .await;
    let SessionEvent::HostKeyPrompt { host, port, .. } = prompt else {
        unreachable!()
    };
    assert_eq!((host.as_str(), port), ("127.0.0.1", b.addr.port()));
    env.mgr
        .answer(id, PromptAnswer::HostKey { accept: true })
        .unwrap();
    sink.wait_event(is_ready, T).await;

    // Only the target asked, and its key is now trusted.
    let asked = |e: &SessionEvent| matches!(e, SessionEvent::HostKeyPrompt { .. });
    assert_eq!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| asked(e))
            .count(),
        1
    );
    assert!(
        env.known_hosts
            .check("127.0.0.1", b.addr.port(), &b.host_key)
            .is_ok_and(|s| s == pylentra_core::known_hosts::HostKeyStatus::Known)
    );
    assert_eq!(accepted(&a), ["password"]);
    env.mgr.close(id).await.unwrap();
}

/// The bastion takes one password try (`single_password_try`), so the stored wrong password
/// fails at once and the client never prompts.
#[tokio::test]
async fn bastion_auth_failure_reported() {
    let (env, a, b) = (env(), pw_server(true).await, pw_server(false).await);
    trust(&env, &a);
    trust(&env, &b);
    let chain = vec![
        hop(&env, a.addr.port(), "wrong"),
        hop(&env, b.addr.port(), PW),
    ];
    let sink = Arc::new(CollectSink::default());
    env.mgr.open(chain, 80, 24, sink.clone());

    assert_eq!(closed_error(&sink).await.code, "auth.all_failed");
    assert!(accepted(&a).is_empty() && accepted(&b).is_empty());
    assert!(
        !sink
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, SessionEvent::SecretPrompt { .. }))
    );
}

#[tokio::test]
async fn unreachable_target_via_bastion() {
    let (env, a) = (env(), pw_server(false).await);
    trust(&env, &a);
    let closed_port = {
        let l = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        l.local_addr().unwrap().port()
    };
    let chain = vec![hop(&env, a.addr.port(), PW), hop(&env, closed_port, PW)];
    let sink = Arc::new(CollectSink::default());
    env.mgr.open(chain, 80, 24, sink.clone());

    let err = closed_error(&sink).await;
    assert!(
        ["network.unreachable", "network.refused"].contains(&err.code.as_str()),
        "{err:?}"
    );
    // The error names the target, not the bastion.
    assert_eq!(err.params["port"], closed_port.to_string());
    assert_eq!(accepted(&a), ["password"]);
}

#[tokio::test]
async fn secret_prompt_names_the_current_hop() {
    let (env, a, b) = (env(), pw_server(false).await, pw_server(false).await);
    trust(&env, &a);
    trust(&env, &b);
    // The target has no stored password.
    let chain = vec![hop(&env, a.addr.port(), PW), conn(b.addr.port())];
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(chain, 80, 24, sink.clone());

    let prompt = sink
        .wait_event(|e| matches!(e, SessionEvent::SecretPrompt { .. }), T)
        .await;
    let SessionEvent::SecretPrompt { host, port, .. } = prompt else {
        unreachable!()
    };
    assert_eq!((host.as_str(), port), ("127.0.0.1", b.addr.port()));
    env.mgr
        .answer(
            id,
            PromptAnswer::Secret {
                value: PW.into(),
                save: false,
            },
        )
        .unwrap();
    sink.wait_event(is_ready, T).await;
    env.mgr.close(id).await.unwrap();
}
