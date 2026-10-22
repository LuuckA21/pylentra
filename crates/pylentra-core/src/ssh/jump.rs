//! Connects a chain of hops: each hop is reached through a direct-tcpip channel of the previous one.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::Instant;

use super::auth::{AuthCtx, authenticate};
use super::events::{SessionEvent, Stage};
use super::forward::RemoteRoutes;
use super::handler::ClientHandler;
use super::{PromptBroker, SessionId, SessionOptions, SessionSink};
use crate::error::CoreError;
use crate::known_hosts::KnownHosts;
use crate::model::Connection;
use crate::secrets::SecretStore;

pub(crate) struct ChainCtx<'a> {
    pub id: SessionId,
    pub secrets: &'a dyn SecretStore,
    pub known_hosts: &'a Arc<KnownHosts>,
    pub broker: &'a Arc<PromptBroker>,
    pub sink: &'a Arc<dyn SessionSink>,
    pub opts: &'a SessionOptions,
    pub routes: &'a RemoteRoutes,
}

/// The authenticated target plus the bastions its transport runs through.
pub(crate) struct Chain {
    pub target: Arc<Handle<ClientHandler>>,
    /// Dropping a bastion's handle would tear down the connection tunnelled through it.
    _bastions: Vec<Handle<ClientHandler>>,
}

/// Connects and authenticates every hop of `chain` (bastions first, target last).
/// The connect timeout applies to each hop on its own.
pub(crate) async fn connect_chain(
    chain: &[Connection],
    ctx: &ChainCtx<'_>,
) -> Result<Chain, CoreError> {
    let Some((target, via)) = chain.split_last() else {
        return Err(CoreError::InvalidInput {
            field: "chain".into(),
            reason: "empty".into(),
        });
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(ctx.opts.keepalive_interval),
        keepalive_max: ctx.opts.keepalive_max,
        ..Default::default()
    });
    let mut bastions: Vec<Handle<ClientHandler>> = Vec::with_capacity(via.len());
    for conn in via {
        // A bastion never forwards to the target's routes.
        let handle = open_hop(ctx, conn, bastions.last(), config.clone(), Arc::default()).await?;
        bastions.push(handle);
    }
    let target = open_hop(ctx, target, bastions.last(), config, ctx.routes.clone()).await?;
    Ok(Chain {
        target: Arc::new(target),
        _bastions: bastions,
    })
}

/// Connects one hop (through `via` unless it is the first) and authenticates it.
/// `routes` serves the hop's `forwarded-tcpip` channels.
async fn open_hop(
    ctx: &ChainCtx<'_>,
    conn: &Connection,
    via: Option<&Handle<ClientHandler>>,
    config: Arc<client::Config>,
    routes: RemoteRoutes,
) -> Result<Handle<ClientHandler>, CoreError> {
    let mut handle = connect_hop(ctx, conn, via, config, routes).await?;
    ctx.sink.event(SessionEvent::Status {
        stage: Stage::Authenticating,
    });
    let auth = AuthCtx {
        secrets: ctx.secrets,
        prompts: ctx.broker,
        session: ctx.id,
        sink: &**ctx.sink,
        opts: ctx.opts,
        host: &conn.host.to_lowercase(),
        port: conn.port,
    };
    authenticate(&mut handle, conn, &auth).await?;
    Ok(handle)
}

/// Transport plus SSH handshake (host key included) for one hop, bounded by `connect_timeout`:
/// over TCP for the first hop, over a direct-tcpip channel of `via` for the others.
async fn connect_hop(
    ctx: &ChainCtx<'_>,
    conn: &Connection,
    via: Option<&Handle<ClientHandler>>,
    config: Arc<client::Config>,
    routes: RemoteRoutes,
) -> Result<Handle<ClientHandler>, CoreError> {
    let host = conn.host.to_lowercase();
    let port = conn.port;
    let (prompting, paused) = watch::channel(false);
    let handler = ClientHandler {
        id: ctx.id,
        host: host.clone(),
        port,
        known_hosts: ctx.known_hosts.clone(),
        broker: ctx.broker.clone(),
        sink: ctx.sink.clone(),
        prompting,
        routes,
    };
    let attempt = async {
        match via {
            None => {
                let tcp = tcp_connect(&host, port).await?;
                client::connect_stream(config, tcp, handler).await
            }
            Some(bastion) => {
                let channel = bastion
                    .channel_open_direct_tcpip(host.clone(), port.into(), "127.0.0.1", 0)
                    .await
                    .map_err(|e| match e {
                        // The bastion could not reach the next hop; its reason code does not
                        // tell a refusal from a routing failure.
                        russh::Error::ChannelOpenFailure(reason) => CoreError::Unreachable {
                            host: host.clone(),
                            port,
                            reason: format!("{reason:?}"),
                        },
                        e => e.into(),
                    })?;
                client::connect_stream(config, channel.into_stream(), handler).await
            }
        }
    };
    match timeout_unless_paused(ctx.opts.connect_timeout, paused, attempt).await {
        Some(r) => r,
        None => Err(CoreError::Timeout { host, port }),
    }
}

async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, CoreError> {
    let dns = || CoreError::Dns { host: host.into() };
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| dns())?
        .collect();
    if addrs.is_empty() {
        return Err(dns());
    }
    let tcp = TcpStream::connect(&addrs[..])
        .await
        .map_err(|e| match e.kind() {
            io::ErrorKind::ConnectionRefused => CoreError::Refused {
                host: host.into(),
                port,
            },
            _ => CoreError::Unreachable {
                host: host.into(),
                port,
                reason: e.to_string(),
            },
        })?;
    // Keystrokes should not wait for Nagle.
    let _ = tcp.set_nodelay(true);
    Ok(tcp)
}

/// `tokio::time::timeout`, except that the clock stops while `paused` is true
/// (the user is reading a prompt) and restarts in full afterwards.
async fn timeout_unless_paused<F: Future>(
    limit: Duration,
    mut paused: watch::Receiver<bool>,
    fut: F,
) -> Option<F::Output> {
    tokio::pin!(fut);
    let sleep = tokio::time::sleep(limit);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            out = &mut fut => return Some(out),
            () = &mut sleep, if !*paused.borrow() => return None,
            Ok(()) = paused.changed() => {
                if !*paused.borrow() {
                    sleep.as_mut().reset(Instant::now() + limit);
                }
            }
        }
    }
}
