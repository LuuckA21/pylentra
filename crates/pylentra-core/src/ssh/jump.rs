//! Connects a chain of hops: each hop is reached through a direct-tcpip channel of the previous one.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh::Preferred;
use russh::client::{self, Handle};
use russh::keys::Algorithm;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::Instant;

use super::auth::{AuthCtx, authenticate};
use super::events::{SessionEvent, Stage};
use super::forward::RemoteRoutes;
use super::handler::ClientHandler;
use super::{PromptBroker, SessionId, SessionOptions, SessionSink};
use crate::error::CoreError;
use crate::known_hosts::{KnownHosts, host_key_algorithms, same_key_type};
use crate::model::{AuthMethod, Connection};
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
        return Err(CoreError::invalid("chain", "empty"));
    };
    let mut bastions: Vec<Handle<ClientHandler>> = Vec::with_capacity(via.len());
    for conn in via {
        // A bastion never forwards to the target's routes.
        let handle = open_hop(ctx, conn, bastions.last(), Arc::default()).await?;
        bastions.push(handle);
    }
    let target = open_hop(ctx, target, bastions.last(), ctx.routes.clone()).await?;
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
    routes: RemoteRoutes,
) -> Result<Handle<ClientHandler>, CoreError> {
    let mut handle = connect_hop(ctx, conn, via, routes).await?;
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
        can_save: !matches!(conn.auth, AuthMethod::Auto),
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
    routes: RemoteRoutes,
) -> Result<Handle<ClientHandler>, CoreError> {
    let host = conn.host.to_lowercase();
    let port = conn.port;
    let config = Arc::new(hop_config(
        ctx.opts,
        &ctx.known_hosts.algorithms(&host, port)?,
    ));
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
        connect_timeout: ctx.opts.connect_timeout,
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

/// `known`: algorithms of the keys recorded for the hop. They go first, so an honest server
/// presents a key that can be checked; any other type it then picks counts as a changed key.
fn hop_config(opts: &SessionOptions, known: &[Algorithm]) -> client::Config {
    let mut key = host_key_algorithms();
    key.sort_by_key(|a| !known.iter().any(|k| same_key_type(k, a)));
    client::Config {
        keepalive_interval: opts.keepalive_interval,
        keepalive_max: opts.keepalive_max,
        preferred: Preferred {
            key: key.into(),
            ..Preferred::DEFAULT
        },
        ..Default::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::EcdsaCurve::{NistP256, NistP384};
    use russh::keys::HashAlg::{Sha256, Sha512};

    fn order(known: &[Algorithm]) -> Vec<Algorithm> {
        hop_config(&SessionOptions::default(), known)
            .preferred
            .key
            .into_owned()
    }

    #[test]
    fn known_key_types_go_first() {
        let p256 = Algorithm::Ecdsa { curve: NistP256 };
        let p384 = Algorithm::Ecdsa { curve: NistP384 };
        let rsa = |hash| Algorithm::Rsa { hash };
        assert_eq!(order(&[])[0], Algorithm::Ed25519);
        assert_eq!(
            order(std::slice::from_ref(&p384))[..2],
            [p384, Algorithm::Ed25519]
        );
        assert_eq!(
            order(&[rsa(None), p256.clone()])[..4],
            [
                p256,
                rsa(Some(Sha512)),
                rsa(Some(Sha256)),
                Algorithm::Ed25519
            ]
        );
        assert!(!order(&[rsa(None)]).contains(&rsa(None)));
    }
}
