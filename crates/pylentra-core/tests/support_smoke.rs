//! Smoke tests for the in-process test server in `support`.
mod support;

use std::sync::Arc;
use std::time::Duration;

use pylentra_core::ssh::SessionSink;
use russh::ChannelMsg;
use russh::client::{self, Handler, KeyboardInteractiveAuthResponse};
use russh::keys::PublicKeyOrCertificate;
use support::{CollectSink, TestServer, TestServerConfig, spawn_tcp_echo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const T: Duration = Duration::from_secs(5);

/// Test-only client: trusts any host key; forwarded-tcpip channels are echoed back.
struct Client;

impl Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        tokio::spawn(async move {
            let (mut r, mut w) = tokio::io::split(channel.into_stream());
            let _ = tokio::io::copy(&mut r, &mut w).await;
        });
        Ok(())
    }
}

async fn connect(server: &TestServer) -> client::Handle<Client> {
    client::connect(Arc::new(client::Config::default()), server.addr, Client)
        .await
        .expect("connect")
}

fn with_password(pw: &str) -> TestServerConfig {
    TestServerConfig {
        password: Some(pw.into()),
        ..Default::default()
    }
}

async fn login(server: &TestServer) -> client::Handle<Client> {
    let mut session = connect(server).await;
    let auth = session
        .authenticate_password("u", "pw")
        .await
        .expect("auth");
    assert!(auth.success());
    session
}

#[tokio::test]
async fn shell_echoes_and_records_auth_and_resize() {
    let server = TestServer::start(with_password("pw")).await;
    let session = login(&server).await;
    assert_eq!(*server.accepted_auth.lock().unwrap(), ["password"]);

    let mut ch = session.channel_open_session().await.expect("channel");
    ch.request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .expect("pty");
    ch.request_shell(true).await.expect("shell");
    ch.window_change(120, 40, 0, 0).await.expect("resize");
    ch.data(&b"hi"[..]).await.expect("data");

    let sink = Arc::new(CollectSink::default());
    let pump = sink.clone();
    tokio::spawn(async move {
        while let Some(msg) = ch.wait().await {
            if let ChannelMsg::Data { data } = msg {
                pump.data(data.to_vec());
            }
        }
    });
    sink.wait_data_len(2, T).await;
    assert_eq!(*sink.data.lock().unwrap(), b"hi");
    assert_eq!(*server.resizes.lock().unwrap(), [(120, 40)]);
}

#[tokio::test]
async fn exit_command_reports_status_7() {
    let server = TestServer::start(with_password("pw")).await;
    let session = login(&server).await;
    let mut ch = session.channel_open_session().await.expect("channel");
    ch.request_shell(true).await.expect("shell");
    ch.data(&b"__exit 7\n"[..]).await.expect("data");

    let mut exit = None;
    while let Some(msg) = tokio::time::timeout(T, ch.wait()).await.expect("timeout") {
        if let ChannelMsg::ExitStatus { exit_status } = msg {
            exit = Some(exit_status);
        }
    }
    assert_eq!(exit, Some(7));
}

#[tokio::test]
async fn wrong_password_and_disabled_password_are_rejected() {
    let server = TestServer::start(with_password("pw")).await;
    let mut session = connect(&server).await;
    let auth = session
        .authenticate_password("u", "nope")
        .await
        .expect("auth");
    assert!(!auth.success());
    assert!(server.accepted_auth.lock().unwrap().is_empty());

    let server = TestServer::start(TestServerConfig {
        password: Some("pw".into()),
        kbd_interactive: Some(("Code: ".into(), "42".into())),
        allow_password_method: false,
        ..Default::default()
    })
    .await;
    let mut session = connect(&server).await;
    let auth = session
        .authenticate_password("u", "pw")
        .await
        .expect("auth");
    assert!(!auth.success());
}

#[tokio::test]
async fn keyboard_interactive_prompts_and_accepts_answer() {
    let server = TestServer::start(TestServerConfig {
        kbd_interactive: Some(("Code: ".into(), "42".into())),
        allow_password_method: false,
        ..Default::default()
    })
    .await;
    let mut session = connect(&server).await;
    let first = session
        .authenticate_keyboard_interactive_start("u", None)
        .await
        .expect("start");
    let KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } = first else {
        panic!("expected an info request");
    };
    assert_eq!(prompts[0].prompt, "Code: ");
    let done = session
        .authenticate_keyboard_interactive_respond(vec!["42".into()])
        .await
        .expect("respond");
    assert!(matches!(done, KeyboardInteractiveAuthResponse::Success));
    assert_eq!(
        *server.accepted_auth.lock().unwrap(),
        ["keyboard-interactive"]
    );
}

#[tokio::test]
async fn direct_tcpip_bridges_to_tcp_echo() {
    let echo = spawn_tcp_echo().await;
    let server = TestServer::start(with_password("pw")).await;
    let session = login(&server).await;
    let ch = session
        .channel_open_direct_tcpip("127.0.0.1", u32::from(echo.port()), "127.0.0.1", 0)
        .await
        .expect("direct-tcpip");
    let mut stream = ch.into_stream();
    stream.write_all(b"ping").await.expect("write");
    let mut buf = [0u8; 4];
    tokio::time::timeout(T, stream.read_exact(&mut buf))
        .await
        .expect("timeout")
        .expect("read");
    assert_eq!(&buf, b"ping");
}

#[tokio::test]
async fn remote_forward_opens_channel_to_client() {
    let server = TestServer::start(with_password("pw")).await;
    let session = login(&server).await;
    let port = session
        .tcpip_forward("127.0.0.1", 0)
        .await
        .expect("forward");
    assert_ne!(port, 0);

    let mut tcp = TcpStream::connect(("127.0.0.1", u16::try_from(port).unwrap()))
        .await
        .expect("connect to forwarded port");
    tcp.write_all(b"hello").await.expect("write");
    let mut buf = [0u8; 5];
    tokio::time::timeout(T, tcp.read_exact(&mut buf))
        .await
        .expect("timeout")
        .expect("read");
    assert_eq!(&buf, b"hello");
}

#[tokio::test]
async fn collect_sink_wait_event_returns_matching_event() {
    use pylentra_core::ssh::events::{SessionEvent, Stage};
    let sink = Arc::new(CollectSink::default());
    let tx = sink.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        tx.event(SessionEvent::Status {
            stage: Stage::Ready,
        });
    });
    let ev = sink
        .wait_event(|e| matches!(e, SessionEvent::Status { .. }), T)
        .await;
    assert_eq!(
        ev,
        SessionEvent::Status {
            stage: Stage::Ready
        }
    );
}
