//! Port forwarding of one session: local and SOCKS5 listeners, remote (`tcpip-forward`) routes, state events.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::ChannelOpenFailure;
use russh::client::Handle;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use super::SessionSink;
use super::events::{ForwardState, SessionEvent};
use super::handler::ClientHandler;
use super::socks::{self, HANDSHAKE_TIMEOUT};
use crate::error::CoreError;
use crate::model::ForwardRule;

/// Address the server binds for remote forwards.
const REMOTE_BIND: &str = "127.0.0.1";

/// Remote forwards by the port the server listens on: where its connections must go.
pub(crate) type RemoteRoutes = Arc<Mutex<HashMap<u16, (String, u16)>>>;

type Target = Arc<Handle<ClientHandler>>;

enum Active {
    /// The accept loop; it owns the relays, so aborting it ends them too.
    Local(JoinHandle<()>),
    Remote {
        bound_port: u16,
    },
}

/// Everything forwarded for one session. Dropped with the session: listeners stop silently.
pub(crate) struct Forwards {
    sink: Arc<dyn SessionSink>,
    pub routes: RemoteRoutes,
    next_id: u32,
    active: HashMap<u32, (ForwardRule, Active)>,
}

impl Drop for Forwards {
    fn drop(&mut self) {
        for (_, a) in self.active.values() {
            if let Active::Local(task) = a {
                task.abort();
            }
        }
    }
}

impl Forwards {
    pub fn new(sink: Arc<dyn SessionSink>) -> Self {
        Self {
            sink,
            routes: Arc::default(),
            next_id: 1,
            active: HashMap::new(),
        }
    }

    /// Starts `rule` and reports the outcome as a `Forward` event (failures included).
    pub async fn start(&mut self, target: &Target, rule: ForwardRule) -> Result<u32, CoreError> {
        let forward_id = self.next_id;
        self.next_id += 1;
        let started = match &rule {
            ForwardRule::Local {
                bind_host,
                bind_port,
                dest_host,
                dest_port,
                ..
            } => start_local(target, bind_host, *bind_port, dest_host, *dest_port).await,
            ForwardRule::Remote {
                bind_port,
                dest_host,
                dest_port,
                ..
            } => start_remote(target, &self.routes, *bind_port, dest_host, *dest_port).await,
            ForwardRule::Dynamic {
                bind_host,
                bind_port,
                ..
            } => start_dynamic(target, bind_host, *bind_port).await,
        };
        let (state, result) = match started {
            Ok((active, bound_port)) => {
                self.active.insert(forward_id, (rule.clone(), active));
                (ForwardState::Active { bound_port }, Ok(forward_id))
            }
            Err(e) => (ForwardState::Failed { error: e.to_api() }, Err(e)),
        };
        self.emit(forward_id, rule, state);
        result
    }

    /// Stops the forward; a local listener is closed once this returns.
    pub async fn stop(&mut self, target: &Target, forward_id: u32) -> Result<(), CoreError> {
        let (rule, active) =
            self.active
                .remove(&forward_id)
                .ok_or_else(|| CoreError::NotFound {
                    what: format!("forward {forward_id}"),
                })?;
        match active {
            Active::Local(task) => {
                task.abort();
                let _ = task.await;
            }
            Active::Remote { bound_port } => {
                self.routes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&bound_port);
                if let Err(e) = target
                    .cancel_tcpip_forward(REMOTE_BIND, bound_port.into())
                    .await
                {
                    // Without a route the server's connections are refused anyway.
                    log::debug!("cancel-tcpip-forward failed: {e}");
                }
            }
        }
        self.emit(forward_id, rule, ForwardState::Stopped);
        Ok(())
    }

    /// Closes every local listener and waits for it, without events: `Closed` covers them.
    pub async fn stop_all(&mut self) {
        for (_, (_, a)) in self.active.drain() {
            if let Active::Local(task) = a {
                task.abort();
                let _ = task.await;
            }
        }
    }

    fn emit(&self, forward_id: u32, rule: ForwardRule, state: ForwardState) {
        self.sink.event(SessionEvent::Forward {
            forward_id,
            rule,
            state,
        });
    }
}

pub(crate) fn auto_start(rule: &ForwardRule) -> bool {
    match rule {
        ForwardRule::Local { auto_start, .. }
        | ForwardRule::Remote { auto_start, .. }
        | ForwardRule::Dynamic { auto_start, .. } => *auto_start,
    }
}

async fn bind_listener(bind_host: &str, bind_port: u16) -> Result<TcpListener, CoreError> {
    TcpListener::bind((bind_host, bind_port))
        .await
        .map_err(|e| match e.kind() {
            io::ErrorKind::AddrInUse => CoreError::PortInUse { port: bind_port },
            io::ErrorKind::PermissionDenied => CoreError::PermissionDenied { port: bind_port },
            _ => CoreError::Io(e),
        })
}

/// Binds the listener and spawns the accept loop, which runs `relay` per connection.
async fn listen<F: Future<Output = ()> + Send + 'static>(
    bind_host: &str,
    bind_port: u16,
    relay: impl Fn(TcpStream, SocketAddr) -> F + Send + 'static,
) -> Result<(Active, u16), CoreError> {
    let listener = bind_listener(bind_host, bind_port).await?;
    let bound_port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        let mut relays = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((tcp, peer)) => {
                        relays.spawn(relay(tcp, peer));
                    }
                    Err(e) => {
                        // e.g. out of file descriptors: back off instead of spinning.
                        log::debug!("forward accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                // Reaps finished relays; disabled while there are none.
                Some(_) = relays.join_next() => {}
            }
        }
    });
    Ok((Active::Local(task), bound_port))
}

async fn start_local(
    target: &Target,
    bind_host: &str,
    bind_port: u16,
    dest_host: &str,
    dest_port: u16,
) -> Result<(Active, u16), CoreError> {
    let (target, dest_host) = (target.clone(), dest_host.to_string());
    listen(bind_host, bind_port, move |tcp, peer| {
        relay_local(target.clone(), tcp, peer, dest_host.clone(), dest_port)
    })
    .await
}

async fn start_dynamic(
    target: &Target,
    bind_host: &str,
    bind_port: u16,
) -> Result<(Active, u16), CoreError> {
    let target = target.clone();
    listen(bind_host, bind_port, move |tcp, peer| {
        relay_dynamic(target.clone(), tcp, peer)
    })
    .await
}

async fn relay_local(
    target: Target,
    mut tcp: TcpStream,
    peer: SocketAddr,
    dest_host: String,
    dest_port: u16,
) {
    let opened = target
        .channel_open_direct_tcpip(
            dest_host,
            dest_port.into(),
            peer.ip().to_string(),
            peer.port().into(),
        )
        .await;
    match opened {
        Ok(channel) => {
            let _ = copy_bidirectional(&mut channel.into_stream(), &mut tcp).await;
        }
        // `tcp` drops: the client sees the connection closed.
        Err(e) => log::debug!("direct-tcpip open failed: {e}"),
    }
}

async fn relay_dynamic(target: Target, mut tcp: TcpStream, peer: SocketAddr) {
    let (host, port) = match socks::handshake(&mut tcp, HANDSHAKE_TIMEOUT).await {
        Ok(dest) => dest,
        Err(e) => return log::debug!("socks handshake failed: {e}"),
    };
    let opened = target
        .channel_open_direct_tcpip(host, port.into(), peer.ip().to_string(), peer.port().into())
        .await;
    match opened {
        Ok(channel) => {
            if socks::reply(&mut tcp, 0x00).await.is_ok() {
                let _ = copy_bidirectional(&mut channel.into_stream(), &mut tcp).await;
            }
        }
        Err(e) => {
            log::debug!("direct-tcpip open failed: {e}");
            let _ = socks::reply(&mut tcp, 0x04).await;
        }
    }
}

async fn start_remote(
    target: &Target,
    routes: &RemoteRoutes,
    bind_port: u16,
    dest_host: &str,
    dest_port: u16,
) -> Result<(Active, u16), CoreError> {
    let route = (dest_host.to_string(), dest_port);
    let lock = || routes.lock().unwrap_or_else(|e| e.into_inner());
    // A requested port is known up front, so the first connection cannot beat the route.
    // One already routed belongs to an active forward, which the server would refuse to
    // bind twice: leave its route alone.
    if bind_port != 0 {
        match lock().entry(bind_port) {
            Entry::Occupied(_) => return Err(CoreError::ForwardRejected),
            Entry::Vacant(v) => v.insert(route.clone()),
        };
    }
    let bound_port = match target.tcpip_forward(REMOTE_BIND, bind_port.into()).await {
        // A server answers 0 when it bound the requested port.
        Ok(0) => bind_port,
        Ok(p) => u16::try_from(p).unwrap_or(bind_port),
        Err(e) => {
            lock().remove(&bind_port);
            return Err(match e {
                russh::Error::RequestDenied => CoreError::ForwardRejected,
                e => e.into(),
            });
        }
    };
    let mut routes = lock();
    if bound_port != bind_port {
        routes.remove(&bind_port);
    }
    routes.insert(bound_port, route);
    drop(routes);
    Ok((Active::Remote { bound_port }, bound_port))
}

/// Relays a `forwarded-tcpip` channel to the destination of its route.
/// Runs detached: connecting must not stall the client's event loop.
pub(crate) async fn accept_remote(
    routes: RemoteRoutes,
    connected_port: u32,
    channel: russh::Channel<russh::client::Msg>,
    reply: russh::client::ChannelOpenHandle,
) {
    let dest = u16::try_from(connected_port).ok().and_then(|p| {
        routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&p)
            .cloned()
    });
    let Some((host, port)) = dest else {
        return reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
    };
    match TcpStream::connect((host.as_str(), port)).await {
        Ok(mut tcp) => {
            reply.accept().await;
            let _ = copy_bidirectional(&mut channel.into_stream(), &mut tcp).await;
        }
        Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
    }
}
