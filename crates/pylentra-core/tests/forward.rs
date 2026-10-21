//! Local and remote port forwarding over an in-process server.
mod support;

use std::sync::Arc;
use std::time::Duration;

use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::model::{AuthMethod, Connection, ForwardRule};
use pylentra_core::secrets::{MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{ForwardState, SessionEvent, Stage};
use pylentra_core::ssh::{SessionId, SessionManager, SessionOptions};
use support::{CollectSink, TestServer, TestServerConfig, spawn_tcp_echo};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

const T: Duration = Duration::from_secs(5);
const PW: &str = "pw";

struct Env {
    _dir: TempDir,
    secrets: Arc<MemorySecretStore>,
    mgr: SessionManager,
    server: TestServer,
}

async fn env() -> Env {
    let dir = TempDir::new().unwrap();
    let known_hosts = Arc::new(KnownHosts::new(dir.path().join("known_hosts"), None));
    let secrets = Arc::new(MemorySecretStore::default());
    let mgr = SessionManager::new(
        secrets.clone(),
        known_hosts.clone(),
        SessionOptions::default(),
    );
    let server = TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        ..Default::default()
    })
    .await;
    known_hosts
        .learn("127.0.0.1", server.addr.port(), &server.host_key)
        .unwrap();
    Env {
        _dir: dir,
        secrets,
        mgr,
        server,
    }
}

fn conn(env: &Env, forwards: Vec<ForwardRule>) -> Connection {
    let id = Uuid::new_v4();
    env.secrets
        .set(id, SecretKind::Password, &PW.to_string().into())
        .unwrap();
    Connection {
        id,
        name: "fwd".into(),
        group_id: None,
        host: "127.0.0.1".into(),
        port: env.server.addr.port(),
        username: "u".into(),
        auth: AuthMethod::Password { save: false },
        jump_via: None,
        forwards,
    }
}

fn is_ready(e: &SessionEvent) -> bool {
    *e == SessionEvent::Status {
        stage: Stage::Ready,
    }
}

async fn open_ready(env: &Env, forwards: Vec<ForwardRule>) -> (SessionId, Arc<CollectSink>) {
    let sink = Arc::new(CollectSink::default());
    let id = env
        .mgr
        .open(vec![conn(env, forwards)], 80, 24, sink.clone());
    sink.wait_event(is_ready, T).await;
    (id, sink)
}

fn local(bind_port: u16, dest: u16, auto_start: bool) -> ForwardRule {
    ForwardRule::Local {
        bind_host: "127.0.0.1".into(),
        bind_port,
        dest_host: "127.0.0.1".into(),
        dest_port: dest,
        auto_start,
    }
}

fn remote(bind_port: u16, dest: u16) -> ForwardRule {
    ForwardRule::Remote {
        bind_port,
        dest_host: "127.0.0.1".into(),
        dest_port: dest,
        auto_start: false,
    }
}

async fn state_of(sink: &CollectSink, id: u32, want: fn(&ForwardState) -> bool) -> ForwardState {
    let ev = sink
        .wait_event(
            |e| matches!(e, SessionEvent::Forward { forward_id, state, .. } if *forward_id == id && want(state)),
            T,
        )
        .await;
    let SessionEvent::Forward { state, .. } = ev else {
        unreachable!()
    };
    state
}

async fn bound_port(sink: &CollectSink, id: u32) -> u16 {
    match state_of(sink, id, |s| matches!(s, ForwardState::Active { .. })).await {
        ForwardState::Active { bound_port } => bound_port,
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

/// Connecting must fail, retrying briefly: a remote listener closes asynchronously.
async fn assert_refused(port: u16) {
    let deadline = tokio::time::Instant::now() + T;
    while TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "port {port} still listening"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn local_forward_relays_to_destination() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![]).await;

    let fid = env
        .mgr
        .forward_start(id, local(0, echo.port(), false))
        .await
        .unwrap();
    let port = bound_port(&sink, fid).await;
    ping(port).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn local_forward_port_in_use_keeps_session() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![]).await;
    let taken = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let taken_port = taken.local_addr().unwrap().port();

    let err = env
        .mgr
        .forward_start(id, local(taken_port, echo.port(), false))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "forward.port_in_use");
    let failed = sink
        .wait_event(
            |e| {
                matches!(e, SessionEvent::Forward { state: ForwardState::Failed { error }, .. }
                    if error.code == "forward.port_in_use")
            },
            T,
        )
        .await;
    assert!(matches!(failed, SessionEvent::Forward { .. }));

    // The session still echoes, and a later forward still works.
    env.mgr.write(id, b"hi".to_vec()).await.unwrap();
    sink.wait_data_len(2, T).await;
    let fid = env
        .mgr
        .forward_start(id, local(0, echo.port(), false))
        .await
        .unwrap();
    ping(bound_port(&sink, fid).await).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn remote_forward_relays_to_local_destination() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![]).await;

    let fid = env
        .mgr
        .forward_start(id, remote(0, echo.port()))
        .await
        .unwrap();
    let port = bound_port(&sink, fid).await;
    ping(port).await;
    // A second connection works too: the route outlives one use.
    ping(port).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn remote_forward_refused_destination_keeps_listener() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let dead = {
        let l = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        l.local_addr().unwrap().port()
    };
    let (id, sink) = open_ready(&env, vec![]).await;

    let bad = env.mgr.forward_start(id, remote(0, dead)).await.unwrap();
    let bad_port = bound_port(&sink, bad).await;
    // The destination refuses: the connection is dropped, the forward lives on.
    let mut tcp = TcpStream::connect(("127.0.0.1", bad_port)).await.unwrap();
    let mut buf = [0u8; 1];
    let n = tokio::time::timeout(T, tcp.read(&mut buf)).await.unwrap();
    assert!(matches!(n, Ok(0) | Err(_)));
    let good = env
        .mgr
        .forward_start(id, remote(0, echo.port()))
        .await
        .unwrap();
    ping(bound_port(&sink, good).await).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn remote_forward_rejected_by_server() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![]).await;
    let first = env
        .mgr
        .forward_start(id, remote(0, echo.port()))
        .await
        .unwrap();
    let taken = bound_port(&sink, first).await;

    // The server cannot bind a port that is already taken.
    let err = env
        .mgr
        .forward_start(id, remote(taken, echo.port()))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "forward.rejected");
    sink.wait_event(
        |e| {
            matches!(e, SessionEvent::Forward { state: ForwardState::Failed { error }, .. }
                if error.code == "forward.rejected")
        },
        T,
    )
    .await;
    env.mgr.write(id, b"ok".to_vec()).await.unwrap();
    sink.wait_data_len(2, T).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn auto_start_forwards_active_before_ready() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let taken = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let rules = vec![
        local(0, echo.port(), true),
        // Not started: auto_start is off.
        local(0, echo.port(), false),
        // Fails, yet the session still reaches Ready.
        local(taken.local_addr().unwrap().port(), echo.port(), true),
    ];
    let (id, sink) = open_ready(&env, rules).await;

    let events = sink.events.lock().unwrap().clone();
    let ready = events.iter().position(is_ready).unwrap();
    let forwards: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            SessionEvent::Forward {
                forward_id, state, ..
            } => Some((i, *forward_id, state.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(forwards.len(), 2, "{forwards:?}");
    assert!(forwards.iter().all(|(i, ..)| *i < ready));
    assert_ne!(forwards[0].1, forwards[1].1);
    let ForwardState::Active { bound_port } = forwards[0].2 else {
        panic!("{forwards:?}")
    };
    assert!(
        matches!(&forwards[1].2, ForwardState::Failed { error } if error.code == "forward.port_in_use")
    );
    ping(bound_port).await;
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn stop_forward_closes_listener() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![]).await;

    for rule in [local(0, echo.port(), false), remote(0, echo.port())] {
        let fid = env.mgr.forward_start(id, rule).await.unwrap();
        let port = bound_port(&sink, fid).await;
        ping(port).await;
        env.mgr.forward_stop(id, fid).await.unwrap();
        state_of(&sink, fid, |s| *s == ForwardState::Stopped).await;
        assert_refused(port).await;
        // Stopped twice is an unknown forward.
        let err = env.mgr.forward_stop(id, fid).await.unwrap_err();
        assert_eq!(err.code(), "not_found");
    }
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn unknown_session_or_forward_is_not_found() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let err = env
        .mgr
        .forward_start(Uuid::new_v4(), local(0, echo.port(), false))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "not_found");
    let err = env.mgr.forward_stop(Uuid::new_v4(), 1).await.unwrap_err();
    assert_eq!(err.code(), "not_found");
    let (id, _sink) = open_ready(&env, vec![]).await;
    let err = env.mgr.forward_stop(id, 99).await.unwrap_err();
    assert_eq!(err.code(), "not_found");
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn session_close_stops_forwards() {
    let env = env().await;
    let echo = spawn_tcp_echo().await;
    let (id, sink) = open_ready(&env, vec![local(0, echo.port(), true)]).await;
    let fid = env
        .mgr
        .forward_start(id, local(0, echo.port(), false))
        .await
        .unwrap();
    let manual = bound_port(&sink, fid).await;
    let auto = match sink.events.lock().unwrap().iter().find_map(|e| match e {
        SessionEvent::Forward {
            state: ForwardState::Active { bound_port },
            ..
        } if *bound_port != manual => Some(*bound_port),
        _ => None,
    }) {
        Some(p) => p,
        None => panic!("auto-started forward missing"),
    };
    // A live relayed connection must not keep the session (or its port) alive.
    let mut open = TcpStream::connect(("127.0.0.1", manual)).await.unwrap();
    open.write_all(b"x").await.unwrap();

    env.mgr.close(id).await.unwrap();
    assert!(TcpStream::connect(("127.0.0.1", manual)).await.is_err());
    assert!(TcpStream::connect(("127.0.0.1", auto)).await.is_err());
    // The final Closed is enough: no Stopped events.
    assert!(!sink.events.lock().unwrap().iter().any(|e| matches!(
        e,
        SessionEvent::Forward {
            state: ForwardState::Stopped,
            ..
        }
    )));
}
