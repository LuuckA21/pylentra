//! In-process russh server and helpers shared by the SSH session tests.
#![allow(dead_code)]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pylentra_core::ssh::SessionSink;
use pylentra_core::ssh::events::SessionEvent;
use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{Auth, Config, Handler, Msg, Response, Server, Session};
use russh::{Channel, ChannelId, ChannelOpenFailure, MethodKind, MethodSet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub struct TestServerConfig {
    pub password: Option<String>,
    /// (prompt, expected answer)
    pub kbd_interactive: Option<(String, String)>,
    /// Whether the keyboard-interactive prompt echoes the answer.
    pub kbd_echo: bool,
    /// A second keyboard-interactive round (echoed "OTP: " prompt) and its expected answer.
    pub kbd_otp: Option<String>,
    pub authorized_key: Option<PublicKey>,
    /// false: password is not advertised, only the other configured methods.
    pub allow_password_method: bool,
    /// true: a wrong password withdraws the method (russh's default); false: retries are
    /// allowed, like OpenSSH.
    pub single_password_try: bool,
}

impl Default for TestServerConfig {
    fn default() -> Self {
        Self {
            password: None,
            kbd_interactive: None,
            kbd_echo: false,
            kbd_otp: None,
            authorized_key: None,
            allow_password_method: true,
            single_password_try: false,
        }
    }
}

pub struct TestServer {
    pub addr: SocketAddr,
    pub host_key: PublicKey,
    /// (cols, rows) of every window-change request received.
    pub resizes: Arc<Mutex<Vec<(u32, u32)>>>,
    /// (term, cols, rows) of every pty-req received.
    pub ptys: Arc<Mutex<Vec<(String, u32, u32)>>>,
    /// SSH method names ("password", "keyboard-interactive", "publickey") that succeeded.
    pub accepted_auth: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    pub async fn start(cfg: TestServerConfig) -> TestServer {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
        let host_key = key.public_key().clone();

        let mut methods = MethodSet::empty();
        if cfg.allow_password_method {
            methods.push(MethodKind::Password);
        }
        if cfg.kbd_interactive.is_some() {
            methods.push(MethodKind::KeyboardInteractive);
        }
        if cfg.authorized_key.is_some() {
            methods.push(MethodKind::PublicKey);
        }
        let config = Arc::new(Config {
            keys: vec![key],
            methods: methods.clone(),
            // Rejections are instant so failing-auth tests stay fast.
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            // russh stops reading while it flushes, so a russh client and a russh
            // server can both block writing a full window into small socket
            // buffers (Windows loopback). OpenSSH keeps reading; a small window
            // here keeps the client's in-flight data below any socket buffer.
            window_size: 16 * 1024,
            ..Default::default()
        });

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let ptys = Arc::new(Mutex::new(Vec::new()));
        let accepted_auth = Arc::new(Mutex::new(Vec::new()));
        let mut server = Acceptor {
            cfg: Arc::new(cfg),
            methods,
            resizes: resizes.clone(),
            ptys: ptys.clone(),
            accepted_auth: accepted_auth.clone(),
        };
        let task = tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });
        TestServer {
            addr,
            host_key,
            resizes,
            ptys,
            accepted_auth,
            task,
        }
    }
}

#[derive(Clone)]
struct Acceptor {
    cfg: Arc<TestServerConfig>,
    methods: MethodSet,
    resizes: Arc<Mutex<Vec<(u32, u32)>>>,
    ptys: Arc<Mutex<Vec<(String, u32, u32)>>>,
    accepted_auth: Arc<Mutex<Vec<String>>>,
}

impl Server for Acceptor {
    type Handler = ConnHandler;
    fn new_client(&mut self, _: Option<SocketAddr>) -> ConnHandler {
        ConnHandler {
            cfg: self.cfg.clone(),
            methods: self.methods.clone(),
            resizes: self.resizes.clone(),
            ptys: self.ptys.clone(),
            accepted_auth: self.accepted_auth.clone(),
            tasks: Vec::new(),
            listeners: HashMap::new(),
            relayed: HashSet::new(),
            otp_pending: false,
        }
    }
}

struct ConnHandler {
    cfg: Arc<TestServerConfig>,
    /// Advertised methods, re-offered after a wrong password or answer.
    methods: MethodSet,
    resizes: Arc<Mutex<Vec<(u32, u32)>>>,
    ptys: Arc<Mutex<Vec<(String, u32, u32)>>>,
    accepted_auth: Arc<Mutex<Vec<String>>>,
    /// Forward listeners and pipes; aborted when the client connection ends.
    tasks: Vec<JoinHandle<()>>,
    /// Accept loops of tcpip-forward listeners by bound port, for cancel-tcpip-forward.
    listeners: HashMap<u16, JoinHandle<()>>,
    /// direct-tcpip channels: their bytes belong to the pipe, not to the echo shell.
    relayed: HashSet<ChannelId>,
    /// The first keyboard-interactive round passed; the OTP round is next.
    otp_pending: bool,
}

impl Drop for ConnHandler {
    fn drop(&mut self) {
        for t in self.tasks.iter().chain(self.listeners.values()) {
            t.abort();
        }
    }
}

impl ConnHandler {
    fn accept(&self, method: &str) -> Auth {
        self.accepted_auth
            .lock()
            .expect("accepted_auth")
            .push(method.to_string());
        Auth::Accept
    }

    /// Rejects but lets the client try again, like OpenSSH (russh would drop the method).
    fn retry(&self) -> Auth {
        Auth::Reject {
            proceed_with_methods: Some(self.methods.clone()),
            partial_success: false,
        }
    }
}

impl Handler for ConnHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        if self.cfg.allow_password_method && self.cfg.password.as_deref() == Some(password) {
            Ok(self.accept("password"))
        } else if self.cfg.single_password_try {
            Ok(Auth::reject())
        } else {
            Ok(self.retry())
        }
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        let Some((prompt, answer)) = self.cfg.kbd_interactive.clone() else {
            return Ok(Auth::reject());
        };
        let Some(mut r) = response else {
            self.otp_pending = false;
            return Ok(Auth::Partial {
                name: Cow::Borrowed("test"),
                instructions: Cow::Borrowed(""),
                prompts: Cow::Owned(vec![(Cow::Owned(prompt), self.cfg.kbd_echo)]),
            });
        };
        let got = r.next();
        if std::mem::take(&mut self.otp_pending) {
            return Ok(match &self.cfg.kbd_otp {
                Some(otp) if got.as_deref() == Some(otp.as_bytes()) => {
                    self.accept("keyboard-interactive")
                }
                _ => self.retry(),
            });
        }
        if got.as_deref() == Some(answer.as_bytes()) {
            if self.cfg.kbd_otp.is_some() {
                self.otp_pending = true;
                return Ok(Auth::Partial {
                    name: Cow::Borrowed("test"),
                    instructions: Cow::Borrowed(""),
                    prompts: Cow::Owned(vec![(Cow::Borrowed("OTP: "), true)]),
                });
            }
            Ok(self.accept("keyboard-interactive"))
        } else {
            Ok(self.retry())
        }
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        Ok(match &self.cfg.authorized_key {
            Some(k) if k.key_data() == key.key_data() => Auth::Accept,
            _ => Auth::reject(),
        })
    }

    async fn auth_publickey(&mut self, _user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        Ok(match &self.cfg.authorized_key {
            Some(k) if k.key_data() == key.key_data() => self.accept("publickey"),
            _ => Auth::reject(),
        })
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _pix_w: u32,
        _pix_h: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.ptys
            .lock()
            .expect("ptys")
            .push((term.to_string(), cols, rows));
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        cols: u32,
        rows: u32,
        _pix_w: u32,
        _pix_h: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.resizes.lock().expect("resizes").push((cols, rows));
        session.channel_success(channel)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.relayed.contains(&channel) {
            Ok(())
        } else if data == b"__exit 7\n" {
            session.exit_status_request(channel, 7)?;
            session.close(channel)
        } else {
            session.data(channel, data.to_vec())
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let target = u16::try_from(port_to_connect).ok();
        let stream = match target {
            Some(p) => TcpStream::connect((host_to_connect, p)).await.ok(),
            None => None,
        };
        match stream {
            Some(mut tcp) => {
                self.relayed.insert(channel.id());
                reply.accept().await;
                self.tasks.push(tokio::spawn(async move {
                    let mut ch = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut ch, &mut tcp).await;
                }));
            }
            None => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let Ok(requested) = u16::try_from(*port) else {
            return Ok(false);
        };
        let Ok(listener) = TcpListener::bind(("127.0.0.1", requested)).await else {
            return Ok(false);
        };
        let Ok(bound) = listener.local_addr() else {
            return Ok(false);
        };
        *port = u32::from(bound.port());

        let handle = session.handle();
        let address = address.to_string();
        let connected_port = *port;
        let accept_loop = tokio::spawn(async move {
            while let Ok((mut tcp, peer)) = listener.accept().await {
                let opened = handle
                    .channel_open_forwarded_tcpip(
                        address.clone(),
                        connected_port,
                        peer.ip().to_string(),
                        u32::from(peer.port()),
                    )
                    .await;
                // One refused open (e.g. the client has no route) must not end the listener.
                let Ok(channel) = opened else { continue };
                tokio::spawn(async move {
                    let mut ch = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut ch, &mut tcp).await;
                });
            }
        });
        self.listeners.insert(bound.port(), accept_loop);
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let task = u16::try_from(port)
            .ok()
            .and_then(|p| self.listeners.remove(&p));
        let Some(task) = task else { return Ok(false) };
        task.abort();
        // Awaited, so the port is closed by the time the client sees the reply.
        let _ = task.await;
        Ok(true)
    }
}

/// Plain TCP echo server on an ephemeral 127.0.0.1 port; lives as long as the runtime.
pub async fn spawn_tcp_echo() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = tcp.read(&mut buf).await {
                    if n == 0 || tcp.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    addr
}

/// `SessionSink` that records everything and lets tests await it.
#[derive(Default)]
pub struct CollectSink {
    pub data: Mutex<Vec<u8>>,
    pub events: Mutex<Vec<SessionEvent>>,
    notify: Notify,
}

impl SessionSink for CollectSink {
    fn data(&self, bytes: Vec<u8>) {
        self.data.lock().expect("data").extend(bytes);
        self.notify.notify_one();
    }

    fn event(&self, ev: SessionEvent) {
        self.events.lock().expect("events").push(ev);
        self.notify.notify_one();
    }
}

impl CollectSink {
    /// Returns the first recorded event matching `pred` (already seen or arriving within `t`).
    pub async fn wait_event(
        &self,
        pred: impl Fn(&SessionEvent) -> bool,
        t: Duration,
    ) -> SessionEvent {
        let found = || {
            self.events
                .lock()
                .expect("events")
                .iter()
                .find(|e| pred(e))
                .cloned()
        };
        self.wait_until(found, t, "event").await
    }

    pub async fn wait_data_len(&self, n: usize, t: Duration) {
        let len_ok = || (self.data.lock().expect("data").len() >= n).then_some(());
        self.wait_until(len_ok, t, "data").await
    }

    // notify_one stores a permit, so a push between the check and the await is never missed.
    async fn wait_until<T>(&self, check: impl Fn() -> Option<T>, t: Duration, what: &str) -> T {
        let work = async {
            loop {
                if let Some(v) = check() {
                    return v;
                }
                self.notify.notified().await;
            }
        };
        match tokio::time::timeout(t, work).await {
            Ok(v) => v,
            Err(_) => panic!(
                "timed out after {t:?} waiting for {what}; events so far: {:?}",
                self.events.lock().expect("events")
            ),
        }
    }
}
