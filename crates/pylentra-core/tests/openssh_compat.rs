//! Compatibility with a real OpenSSH server (Docker). Ignored unless `PYLENTRA_COMPAT=1`.
//!
//! Start the environment first, see `tests/compat/README.md`.
mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::model::{AuthMethod, Connection, ForwardRule};
use pylentra_core::secrets::{MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{ForwardState, PromptAnswer, SessionEvent, Stage};
use pylentra_core::ssh::{SessionId, SessionManager, SessionOptions};
use support::{CollectSink, spawn_tcp_echo};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

/// Generous: sshd start-up and PAM are slower than the in-process server.
const T: Duration = Duration::from_secs(30);

struct Env {
    _dir: TempDir,
    secrets: Arc<MemorySecretStore>,
    mgr: SessionManager,
}

fn env() -> Option<Env> {
    if std::env::var("PYLENTRA_COMPAT").as_deref() != Ok("1") {
        return None;
    }
    let dir = TempDir::new().unwrap();
    let known_hosts = Arc::new(KnownHosts::new(dir.path().join("known_hosts"), None));
    let secrets = Arc::new(MemorySecretStore::default());
    let mgr = SessionManager::new(secrets.clone(), known_hosts, SessionOptions::default());
    Some(Env {
        _dir: dir,
        secrets,
        mgr,
    })
}

fn hop(env: &Env, host: &str, port: u16, auth: AuthMethod) -> Connection {
    let c = Connection {
        id: Uuid::new_v4(),
        name: host.into(),
        group_id: None,
        host: host.into(),
        port,
        username: "tester".into(),
        auth,
        jump_via: None,
        forwards: vec![],
    };
    env.secrets
        .set(c.id, SecretKind::Password, &"tester".to_string().into())
        .unwrap();
    c
}

fn password() -> AuthMethod {
    AuthMethod::Password { save: false }
}

fn bastion(env: &Env, auth: AuthMethod) -> Connection {
    hop(env, "127.0.0.1", 2201, auth)
}

/// Opens the chain, accepting every unknown host key, and returns once the shell is ready.
async fn open_ready(env: &Env, chain: Vec<Connection>) -> (SessionId, Arc<CollectSink>) {
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(chain, 80, 24, sink.clone());
    let deadline = tokio::time::Instant::now() + T;
    let mut answered = 0;
    loop {
        let events = sink.events.lock().unwrap().clone();
        let prompts = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }))
            .count();
        if prompts > answered {
            answered += 1;
            env.mgr
                .answer(id, PromptAnswer::HostKey { accept: true })
                .unwrap();
        } else if events.iter().any(|e| {
            *e == SessionEvent::Status {
                stage: Stage::Ready,
            }
        }) {
            return (id, sink);
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SessionEvent::Closed { .. })),
            "session closed early: {events:?}"
        );
        assert!(tokio::time::Instant::now() < deadline, "{events:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Waits until the shell output contains `needle`, then returns everything received so far.
async fn wait_output(sink: &CollectSink, needle: &str) -> String {
    let deadline = tokio::time::Instant::now() + T;
    loop {
        let out = String::from_utf8_lossy(&sink.data.lock().unwrap()).into_owned();
        if out.contains(needle) {
            return out;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no {needle:?} in {out:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Runs a command in the shell. The `$((1+1))` keeps the typed (echoed) line from matching.
async fn hostname(env: &Env, id: SessionId, sink: &CollectSink) -> String {
    env.mgr
        .write(id, b"echo H$((1+1))=$(hostname)=E$((2+2))\n".to_vec())
        .await
        .unwrap();
    let out = wait_output(sink, "=E4").await;
    let tail = out.split("H2=").nth(1).unwrap();
    tail.split('=').next().unwrap().to_string()
}

async fn bound_port(sink: &CollectSink) -> u16 {
    let ev = sink
        .wait_event(
            |e| {
                matches!(
                    e,
                    SessionEvent::Forward {
                        state: ForwardState::Active { .. },
                        ..
                    }
                )
            },
            T,
        )
        .await;
    match ev {
        SessionEvent::Forward {
            state: ForwardState::Active { bound_port },
            ..
        } => bound_port,
        _ => unreachable!(),
    }
}

async fn ping(port: u16) {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tcp.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    tokio::time::timeout(T, tcp.read_exact(&mut buf))
        .await
        .expect("echo timed out")
        .unwrap();
    assert_eq!(&buf, b"ping");
}

fn local(dest_host: &str, dest_port: u16) -> ForwardRule {
    ForwardRule::Local {
        bind_host: "127.0.0.1".into(),
        bind_port: 0,
        dest_host: dest_host.into(),
        dest_port,
        auto_start: false,
    }
}

#[tokio::test]
#[ignore = "needs the Docker environment and PYLENTRA_COMPAT=1"]
async fn kbd_interactive_password_login() {
    let Some(env) = env() else { return };
    // sshd has PasswordAuthentication off, so the password goes through keyboard-interactive.
    let (id, sink) = open_ready(&env, vec![bastion(&env, password())]).await;
    assert!(
        !sink
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, SessionEvent::SecretPrompt { .. }))
    );
    env.mgr
        .write(id, b"echo ok$((6*7))\n".to_vec())
        .await
        .unwrap();
    wait_output(&sink, "ok42").await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
#[ignore = "needs the Docker environment and PYLENTRA_COMPAT=1"]
async fn key_login() {
    let Some(env) = env() else { return };
    let key = AuthMethod::Key {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/compat/test_ed25519"),
        save_passphrase: false,
    };
    let (id, sink) = open_ready(&env, vec![bastion(&env, key)]).await;
    env.mgr
        .write(id, b"echo ok$((6*7))\n".to_vec())
        .await
        .unwrap();
    wait_output(&sink, "ok42").await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
#[ignore = "needs the Docker environment and PYLENTRA_COMPAT=1"]
async fn jump_to_target_via_bastion() {
    let Some(env) = env() else { return };
    let (direct, direct_sink) = open_ready(&env, vec![bastion(&env, password())]).await;
    let bastion_host = hostname(&env, direct, &direct_sink).await;

    // `target` resolves only on the compose network, i.e. from the bastion.
    let chain = vec![
        bastion(&env, password()),
        hop(&env, "target", 22, password()),
    ];
    let (id, sink) = open_ready(&env, chain).await;
    assert_ne!(hostname(&env, id, &sink).await, bastion_host);

    // The target's `nc -e cat` service answers a shell on the target itself.
    env.mgr
        .write(id, b"echo ping$((1+1)) | nc -w 1 localhost 9000\n".to_vec())
        .await
        .unwrap();
    wait_output(&sink, "ping2").await;
    env.mgr.close(id).await.unwrap();
    env.mgr.close(direct).await.unwrap();
}

#[tokio::test]
#[ignore = "needs the Docker environment and PYLENTRA_COMPAT=1"]
async fn local_forward_through_jump() {
    let Some(env) = env() else { return };
    let chain = vec![
        bastion(&env, password()),
        hop(&env, "target", 22, password()),
    ];
    let (id, sink) = open_ready(&env, chain).await;
    // The destination is resolved on the target, where `nc -e cat` listens.
    env.mgr
        .forward_start(id, local("localhost", 9000))
        .await
        .unwrap();
    ping(bound_port(&sink).await).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
#[ignore = "needs the Docker environment and PYLENTRA_COMPAT=1"]
async fn remote_forward() {
    let Some(env) = env() else { return };
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![bastion(&env, password())]).await;
    env.mgr
        .forward_start(
            id,
            ForwardRule::Remote {
                bind_port: 0,
                dest_host: "127.0.0.1".into(),
                dest_port: echo.port(),
                auto_start: false,
            },
        )
        .await
        .unwrap();
    let remote_port = bound_port(&sink).await;

    // sshd listens on its own loopback, which a second session reaches with a local forward.
    let (probe, probe_sink) = open_ready(&env, vec![bastion(&env, password())]).await;
    env.mgr
        .forward_start(probe, local("127.0.0.1", remote_port))
        .await
        .unwrap();
    ping(bound_port(&probe_sink).await).await;
    env.mgr.close(probe).await.unwrap();
    env.mgr.close(id).await.unwrap();
}
