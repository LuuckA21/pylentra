//! End-to-end tests of `SessionManager` against the in-process russh server.
mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pylentra_core::error::CoreError;
use pylentra_core::known_hosts::{HostKeyStatus, KnownHosts, fingerprint};
use pylentra_core::model::{AuthMethod, Connection};
use pylentra_core::secrets::{MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{CloseReason, PromptAnswer, PromptKind, SessionEvent, Stage};
use pylentra_core::ssh::{SessionId, SessionManager, SessionOptions};
use russh::keys::{Algorithm, PrivateKey};
use support::{CollectSink, TestServer, TestServerConfig};
use tempfile::TempDir;
use tokio::net::TcpListener;
use uuid::Uuid;

const T: Duration = Duration::from_secs(5);
const PW: &str = "pw";

struct Env {
    _dir: TempDir,
    known_hosts: Arc<KnownHosts>,
    secrets: Arc<MemorySecretStore>,
    mgr: SessionManager,
}

fn env_with(opts: SessionOptions) -> Env {
    let dir = TempDir::new().unwrap();
    let known_hosts = Arc::new(KnownHosts::new(dir.path().join("known_hosts"), None));
    let secrets = Arc::new(MemorySecretStore::default());
    let mgr = SessionManager::new(secrets.clone(), known_hosts.clone(), opts);
    Env {
        _dir: dir,
        known_hosts,
        secrets,
        mgr,
    }
}

fn env() -> Env {
    env_with(SessionOptions::default())
}

fn conn(host: &str, port: u16) -> Connection {
    Connection {
        id: Uuid::new_v4(),
        name: "test".into(),
        group_id: None,
        host: host.into(),
        port,
        username: "u".into(),
        auth: AuthMethod::Password { save: false },
        jump_via: None,
        forwards: vec![],
        color: None,
        icon: None,
    }
}

async fn pw_server() -> TestServer {
    TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        ..Default::default()
    })
    .await
}

/// Server trusted in `env` and a connection whose password is stored.
fn trusted_conn(env: &Env, server: &TestServer) -> Connection {
    let c = conn("127.0.0.1", server.addr.port());
    env.known_hosts
        .learn("127.0.0.1", server.addr.port(), &server.host_key)
        .unwrap();
    env.secrets
        .set(c.id, SecretKind::Password, &PW.to_string().into())
        .unwrap();
    c
}

fn is_ready(e: &SessionEvent) -> bool {
    *e == SessionEvent::Status {
        stage: Stage::Ready,
    }
}

async fn open_ready(env: &Env, server: &TestServer) -> (SessionId, Arc<CollectSink>) {
    let sink = Arc::new(CollectSink::default());
    let id = env
        .mgr
        .open(vec![trusted_conn(env, server)], 80, 24, sink.clone());
    sink.wait_event(is_ready, T).await;
    (id, sink)
}

async fn closed(sink: &CollectSink) -> CloseReason {
    match sink
        .wait_event(|e| matches!(e, SessionEvent::Closed { .. }), T)
        .await
    {
        SessionEvent::Closed { reason } => reason,
        _ => unreachable!(),
    }
}

async fn closed_code(sink: &CollectSink) -> String {
    match closed(sink).await {
        CloseReason::Error { error } => error.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn closed_count(sink: &CollectSink) -> usize {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, SessionEvent::Closed { .. }))
        .count()
}

fn has_event(sink: &CollectSink, pred: impl Fn(&SessionEvent) -> bool) -> bool {
    sink.events.lock().unwrap().iter().any(pred)
}

#[tokio::test]
async fn password_session_reaches_ready_and_echoes() {
    let server = pw_server().await;
    let env = env();
    let sink = Arc::new(CollectSink::default());
    let id = env
        .mgr
        .open(vec![trusted_conn(&env, &server)], 100, 30, sink.clone());
    sink.wait_event(is_ready, T).await;
    let stages: Vec<_> = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            SessionEvent::Status { stage } => Some(stage.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        stages,
        [Stage::Connecting, Stage::Authenticating, Stage::Ready]
    );

    env.mgr.write(id, b"hello".to_vec()).await.unwrap();
    sink.wait_data_len(5, T).await;
    assert_eq!(*sink.data.lock().unwrap(), b"hello");
    assert_eq!(
        *server.ptys.lock().unwrap(),
        [("xterm-256color".to_string(), 100, 30)]
    );
    assert_eq!(*server.accepted_auth.lock().unwrap(), ["password"]);
}

#[tokio::test]
async fn unknown_host_prompts_then_learns() {
    let server = pw_server().await;
    let env = env();
    let c = conn("127.0.0.1", server.addr.port());
    env.secrets
        .set(c.id, SecretKind::Password, &PW.to_string().into())
        .unwrap();
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![c], 80, 24, sink.clone());

    let prompt = sink
        .wait_event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }), T)
        .await;
    let SessionEvent::HostKeyPrompt {
        host,
        port,
        fingerprint: fp,
        ..
    } = prompt
    else {
        unreachable!()
    };
    assert_eq!((host.as_str(), port), ("127.0.0.1", server.addr.port()));
    assert_eq!(fp, fingerprint(&server.host_key));
    assert_eq!(env.mgr.pending_prompts(), 1);

    env.mgr
        .answer(id, PromptAnswer::HostKey { accept: true })
        .unwrap();
    sink.wait_event(is_ready, T).await;
    assert_eq!(env.mgr.pending_prompts(), 0);
    assert_eq!(
        env.known_hosts
            .check("127.0.0.1", server.addr.port(), &server.host_key)
            .unwrap(),
        HostKeyStatus::Known
    );
}

#[tokio::test]
async fn host_key_prompt_does_not_count_toward_connect_timeout() {
    let server = pw_server().await;
    let env = env_with(SessionOptions {
        connect_timeout: Duration::from_millis(300),
        ..Default::default()
    });
    let c = conn("127.0.0.1", server.addr.port());
    env.secrets
        .set(c.id, SecretKind::Password, &PW.to_string().into())
        .unwrap();
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![c], 80, 24, sink.clone());
    sink.wait_event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }), T)
        .await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    env.mgr
        .answer(id, PromptAnswer::HostKey { accept: true })
        .unwrap();
    sink.wait_event(is_ready, T).await;
}

#[tokio::test]
async fn host_is_lowercased_for_known_hosts() {
    let server = pw_server().await;
    let env = env();
    let c = conn("LOCALHOST", server.addr.port());
    env.known_hosts
        .learn("localhost", server.addr.port(), &server.host_key)
        .unwrap();
    env.secrets
        .set(c.id, SecretKind::Password, &PW.to_string().into())
        .unwrap();
    let sink = Arc::new(CollectSink::default());
    env.mgr.open(vec![c], 80, 24, sink.clone());
    sink.wait_event(is_ready, T).await;
    assert!(!has_event(&sink, |e| matches!(
        e,
        SessionEvent::HostKeyPrompt { .. }
    )));
}

#[tokio::test]
async fn rejected_host_key_closes() {
    let server = pw_server().await;
    let env = env();
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(
        vec![conn("127.0.0.1", server.addr.port())],
        80,
        24,
        sink.clone(),
    );
    sink.wait_event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }), T)
        .await;
    env.mgr
        .answer(id, PromptAnswer::HostKey { accept: false })
        .unwrap();
    assert_eq!(closed_code(&sink).await, "hostkey.rejected");
    assert_eq!(
        env.known_hosts
            .check("127.0.0.1", server.addr.port(), &server.host_key)
            .unwrap(),
        HostKeyStatus::Unknown
    );
}

#[tokio::test]
async fn mismatch_closes_without_prompt() {
    let server = pw_server().await;
    let env = env();
    let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    env.known_hosts
        .learn("127.0.0.1", server.addr.port(), other.public_key())
        .unwrap();
    let sink = Arc::new(CollectSink::default());
    env.mgr.open(
        vec![conn("127.0.0.1", server.addr.port())],
        80,
        24,
        sink.clone(),
    );

    let ev = sink
        .wait_event(|e| matches!(e, SessionEvent::HostKeyMismatch { .. }), T)
        .await;
    let SessionEvent::HostKeyMismatch { expected, got, .. } = ev else {
        unreachable!()
    };
    assert_eq!(expected, fingerprint(other.public_key()));
    assert_eq!(got, fingerprint(&server.host_key));
    assert_eq!(closed_code(&sink).await, "hostkey.mismatch");
    assert!(!has_event(&sink, |e| matches!(
        e,
        SessionEvent::HostKeyPrompt { .. }
    )));
}

#[tokio::test]
async fn missing_password_prompts() {
    let server = pw_server().await;
    let env = env();
    let c = conn("127.0.0.1", server.addr.port());
    env.known_hosts
        .learn("127.0.0.1", server.addr.port(), &server.host_key)
        .unwrap();
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![c.clone()], 80, 24, sink.clone());

    sink.wait_event(
        |e| {
            matches!(e, SessionEvent::SecretPrompt { host, port, kind: PromptKind::Password, .. }
                if host == "127.0.0.1" && *port == server.addr.port())
        },
        T,
    )
    .await;
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
    assert!(
        env.secrets
            .get(c.id, SecretKind::Password)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        env.mgr.answer(id, PromptAnswer::Cancel),
        Err(CoreError::NotFound { .. })
    ));
}

#[tokio::test]
async fn resize_reaches_server() {
    let server = pw_server().await;
    let env = env();
    let (id, sink) = open_ready(&env, &server).await;
    env.mgr.resize(id, 120, 40).await.unwrap();
    // A write after the resize proves the server processed it (same channel, in order).
    env.mgr.write(id, b"x".to_vec()).await.unwrap();
    sink.wait_data_len(1, T).await;
    assert_eq!(*server.resizes.lock().unwrap(), [(120, 40)]);
}

#[tokio::test]
async fn remote_exit_reports_code() {
    let server = pw_server().await;
    let env = env();
    let (id, sink) = open_ready(&env, &server).await;
    env.mgr.write(id, b"__exit 7\n".to_vec()).await.unwrap();
    assert_eq!(closed(&sink).await, CloseReason::Exited { code: 7 });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(closed_count(&sink), 1);
    assert!(matches!(
        env.mgr.write(id, b"x".to_vec()).await,
        Err(CoreError::NotFound { .. })
    ));
}

#[tokio::test]
async fn user_close_emits_user_closed_once() {
    let server = pw_server().await;
    let env = env();
    let (id, sink) = open_ready(&env, &server).await;
    env.mgr.close(id).await.unwrap();
    assert_eq!(closed(&sink).await, CloseReason::UserClosed);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(closed_count(&sink), 1);
    assert!(matches!(
        env.mgr.write(id, b"x".to_vec()).await,
        Err(CoreError::NotFound { .. })
    ));
    assert!(matches!(
        env.mgr.resize(id, 1, 1).await,
        Err(CoreError::NotFound { .. })
    ));
    assert!(matches!(
        env.mgr.close(id).await,
        Err(CoreError::NotFound { .. })
    ));
}

#[tokio::test]
async fn close_while_prompting_emits_user_closed_once() {
    let server = pw_server().await;
    let env = env();
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(
        vec![conn("127.0.0.1", server.addr.port())],
        80,
        24,
        sink.clone(),
    );
    sink.wait_event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }), T)
        .await;
    env.mgr.close(id).await.unwrap();
    assert_eq!(closed(&sink).await, CloseReason::UserClosed);
    assert_eq!(env.mgr.pending_prompts(), 0);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(closed_count(&sink), 1);
}

#[tokio::test]
async fn connect_timeout() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((tcp, _)) = listener.accept().await {
            held.push(tcp);
        }
    });
    let env = env_with(SessionOptions {
        connect_timeout: Duration::from_millis(300),
        ..Default::default()
    });
    let sink = Arc::new(CollectSink::default());
    env.mgr
        .open(vec![conn("127.0.0.1", port)], 80, 24, sink.clone());
    assert_eq!(closed_code(&sink).await, "network.timeout");
}

#[tokio::test]
async fn refused() {
    let port = {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.local_addr().unwrap().port()
    };
    let env = env();
    let sink = Arc::new(CollectSink::default());
    env.mgr
        .open(vec![conn("127.0.0.1", port)], 80, 24, sink.clone());
    assert_eq!(closed_code(&sink).await, "network.refused");
}

#[tokio::test]
async fn empty_chain_closes_with_error() {
    let env = env();
    let sink = Arc::new(CollectSink::default());
    env.mgr.open(vec![], 80, 24, sink.clone());
    assert_eq!(closed_code(&sink).await, "config.invalid");
    assert_eq!(closed_count(&sink), 1);
}

#[tokio::test]
async fn burst_output_arrives_complete_and_ordered() {
    const TOTAL: usize = 8 * 1024 * 1024;
    const CHUNK: usize = 32 * 1024;
    let server = pw_server().await;
    let env = env();
    let (id, sink) = open_ready(&env, &server).await;
    let input: Vec<u8> = (0..TOTAL).map(|i| (i % 251) as u8).collect();
    for chunk in input.chunks(CHUNK) {
        env.mgr.write(id, chunk.to_vec()).await.unwrap();
    }
    sink.wait_data_len(TOTAL, Duration::from_secs(60)).await;
    assert!(*sink.data.lock().unwrap() == input);
}

#[tokio::test]
async fn non_utf8_bytes_roundtrip() {
    let server = pw_server().await;
    let env = env();
    let (id, sink) = open_ready(&env, &server).await;
    env.mgr
        .write(id, vec![0xff, 0xfe, 0xe2, 0x82])
        .await
        .unwrap();
    env.mgr.write(id, vec![0xac]).await.unwrap();
    sink.wait_data_len(5, T).await;
    assert_eq!(*sink.data.lock().unwrap(), [0xff, 0xfe, 0xe2, 0x82, 0xac]);
}

#[tokio::test]
async fn close_all_respects_timeout() {
    let server = pw_server().await;
    let env = env();
    let mut sinks = Vec::new();
    for _ in 0..3 {
        sinks.push(open_ready(&env, &server).await.1);
    }
    let start = Instant::now();
    env.mgr.close_all(Duration::from_secs(2)).await;
    assert!(start.elapsed() < Duration::from_millis(2500));
    for sink in &sinks {
        assert_eq!(closed(sink).await, CloseReason::UserClosed);
        assert_eq!(closed_count(sink), 1);
    }
}
