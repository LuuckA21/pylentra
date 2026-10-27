//! SOCKS5 server-side handshake (RFC 1928): no authentication, CONNECT only.

use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) enum SocksError {
    Io(io::Error),
    Timeout,
    BadVersion,
    NoAcceptableMethod,
    Rejected(u8),
}

impl std::fmt::Display for SocksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Timeout => f.write_str("timed out"),
            Self::BadVersion => f.write_str("not SOCKS5"),
            Self::NoAcceptableMethod => f.write_str("no acceptable method"),
            Self::Rejected(code) => write!(f, "request refused with code {code:#04x}"),
        }
    }
}

impl From<io::Error> for SocksError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Runs greeting and request within `deadline`; refusals are answered here, a wrong
/// version closes silently. Returns the requested target host and port.
pub(crate) async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    deadline: Duration,
) -> Result<(String, u16), SocksError> {
    tokio::time::timeout(deadline, negotiate(s))
        .await
        .map_err(|_| SocksError::Timeout)?
}

async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
) -> Result<(String, u16), SocksError> {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Err(SocksError::BadVersion);
    }
    let mut methods = vec![0u8; head[1] as usize];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        s.write_all(&[5, 0xFF]).await?;
        s.flush().await?;
        return Err(SocksError::NoAcceptableMethod);
    }
    s.write_all(&[5, 0]).await?;
    s.flush().await?;

    let mut req = [0u8; 4];
    s.read_exact(&mut req).await?;
    if req[0] != 5 {
        return Err(SocksError::BadVersion);
    }
    if req[1] != 1 {
        return refuse(s, 0x07).await;
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        4 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        3 => {
            let len = s.read_u8().await? as usize;
            let mut name = vec![0u8; len];
            s.read_exact(&mut name).await?;
            match String::from_utf8(name) {
                Ok(n) if !n.is_empty() => n,
                _ => return refuse(s, 0x01).await,
            }
        }
        _ => return refuse(s, 0x08).await,
    };
    let port = s.read_u16().await?;
    Ok((host, port))
}

async fn refuse<S: AsyncWrite + Unpin, T>(s: &mut S, code: u8) -> Result<T, SocksError> {
    reply(s, code).await?;
    Err(SocksError::Rejected(code))
}

/// Replies with `code` and the bound address 0.0.0.0:0.
pub(crate) async fn reply<S: AsyncWrite + Unpin>(s: &mut S, code: u8) -> io::Result<()> {
    s.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    s.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    const T: Duration = Duration::from_millis(200);

    /// Runs the handshake against a client that sends `input` (then closes if `close`).
    async fn run(input: &[u8], close: bool) -> (Result<(String, u16), SocksError>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(input).await.unwrap();
        if close {
            client.shutdown().await.unwrap();
        }
        let res = handshake(&mut server, T).await;
        drop(server);
        let mut got = Vec::new();
        client.read_to_end(&mut got).await.unwrap();
        (res, got)
    }

    fn connect(atyp_and_addr: &[u8], port: u16) -> Vec<u8> {
        let mut v = vec![5, 1, 0, 5, 1, 0];
        v.extend_from_slice(atyp_and_addr);
        v.extend_from_slice(&port.to_be_bytes());
        v
    }

    #[tokio::test]
    async fn connect_ipv4() {
        let (r, got) = run(&[5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 1, 0x1f, 0x90], false).await;
        assert_eq!(r.unwrap(), ("10.0.0.1".to_string(), 8080));
        assert_eq!(got, [5, 0]);
    }

    #[tokio::test]
    async fn connect_ipv6() {
        let mut addr = vec![4];
        addr.extend_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
        let (r, _) = run(&connect(&addr, 22), false).await;
        assert_eq!(r.unwrap(), ("::1".to_string(), 22));
    }

    #[tokio::test]
    async fn connect_domain() {
        let mut addr = vec![3, 11];
        addr.extend_from_slice(b"example.com");
        let (r, _) = run(&connect(&addr, 443), false).await;
        assert_eq!(r.unwrap(), ("example.com".to_string(), 443));
    }

    #[tokio::test]
    async fn greeting_and_request_in_one_write() {
        let (mut client, mut server) = duplex(1024);
        let mut all = vec![5, 2, 2, 0];
        all.extend_from_slice(&[5, 1, 0, 1, 127, 0, 0, 1, 0, 80]);
        client.write_all(&all).await.unwrap();
        let r = handshake(&mut server, T).await.unwrap();
        assert_eq!(r, ("127.0.0.1".to_string(), 80));
    }

    #[tokio::test]
    async fn domain_255_bytes_ok() {
        let mut addr = vec![3, 255];
        addr.extend(std::iter::repeat_n(b'a', 255));
        let (r, _) = run(&connect(&addr, 80), false).await;
        let (host, port) = r.unwrap();
        assert_eq!((host.len(), port), (255, 80));
    }

    #[tokio::test]
    async fn empty_domain_rejected() {
        let (r, got) = run(&connect(&[3, 0], 80), false).await;
        assert!(matches!(r, Err(SocksError::Rejected(1))));
        assert_eq!(got, [5, 0, 5, 1, 0, 1, 0, 0, 0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn password_only_rejected() {
        let (r, got) = run(&[5, 1, 2], false).await;
        assert!(matches!(r, Err(SocksError::NoAcceptableMethod)));
        assert_eq!(got, [5, 0xFF]);
    }

    #[tokio::test]
    async fn bind_command_rejected() {
        let (r, got) = run(&[5, 1, 0, 5, 2, 0, 1, 1, 2, 3, 4, 0, 80], false).await;
        assert!(matches!(r, Err(SocksError::Rejected(7))));
        assert_eq!(got[3], 7);
        assert_eq!(&got[..2], [5, 0]);
    }

    #[tokio::test]
    async fn unknown_addr_type_rejected() {
        let (r, got) = run(&[5, 1, 0, 5, 1, 0, 5, 0, 0], false).await;
        assert!(matches!(r, Err(SocksError::Rejected(8))));
        assert_eq!(got[3], 8);
    }

    #[tokio::test]
    async fn socks4_closed() {
        let (r, got) = run(&[4, 1, 0, 80, 1, 2, 3, 4, 0], false).await;
        assert!(matches!(r, Err(SocksError::BadVersion)));
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn truncated_request() {
        let (r, _) = run(&[5, 1, 0, 5, 1, 0, 1, 10, 0], true).await;
        assert!(matches!(r, Err(SocksError::Io(_))));
    }

    #[tokio::test]
    async fn silent_client_times_out() {
        let (_client, mut server) = duplex(1024);
        let start = std::time::Instant::now();
        let r = handshake(&mut server, T).await;
        assert!(matches!(r, Err(SocksError::Timeout)));
        assert!(start.elapsed() < Duration::from_millis(400));
    }

    #[tokio::test]
    async fn reply_bytes() {
        let (mut client, mut server) = duplex(64);
        reply(&mut server, 4).await.unwrap();
        drop(server);
        let mut got = Vec::new();
        client.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, [5, 4, 0, 1, 0, 0, 0, 0, 0, 0]);
    }
}
