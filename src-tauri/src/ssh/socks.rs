//! The server side of a SOCKS handshake, for dynamic forwards (`ssh -D`).
//! A browser or `curl --socks5-hostname` asks this local proxy for a host
//! and port; the forward then opens that connection from the SSH server.
//!
//! Supported: SOCKS5 without authentication (RFC 1928) and SOCKS4/4a, the
//! CONNECT command only, the way OpenSSH's `-D` does. The proxy listens on
//! 127.0.0.1 only, so no authentication is asked for.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V4,
    V5,
}

/// Where the client wants to go
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub version: Version,
    pub host: String,
    pub port: u16,
}

/// Why a reply failed, as the client is told
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The SSH server could not open the connection
    Refused,
    /// Anything else: the session is down, the channel failed
    General,
}

#[derive(Debug)]
pub enum HandshakeError {
    Io(std::io::Error),
    /// Not SOCKS, or a request this proxy does not serve. The client has been
    /// answered where the protocol allows it; the connection should be closed.
    Unsupported(&'static str),
}

impl From<std::io::Error> for HandshakeError {
    fn from(e: std::io::Error) -> Self {
        HandshakeError::Io(e)
    }
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::Io(e) => write!(f, "{e}"),
            HandshakeError::Unsupported(what) => f.write_str(what),
        }
    }
}

const V5_NO_AUTH: u8 = 0x00;
const V5_NO_ACCEPTABLE_METHOD: u8 = 0xff;
const CMD_CONNECT: u8 = 0x01;
const V5_REP_GENERAL: u8 = 0x01;
const V5_REP_REFUSED: u8 = 0x05;
const V5_REP_COMMAND: u8 = 0x07;
const V5_REP_ADDRESS: u8 = 0x08;
/// The longest user id or host name a SOCKS4a request may send
const MAX_V4_STRING: usize = 255;

/// Read the client's greeting and request. On success the client waits for
/// [`reply`]; nothing has been written for SOCKS4, and the method choice
/// for SOCKS5.
pub async fn handshake<S>(s: &mut S) -> Result<Request, HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match s.read_u8().await? {
        5 => handshake_v5(s).await,
        4 => handshake_v4(s).await,
        _ => Err(HandshakeError::Unsupported("not a SOCKS request")),
    }
}

async fn handshake_v5<S>(s: &mut S) -> Result<Request, HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let n = s.read_u8().await? as usize;
    let mut methods = vec![0u8; n];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&V5_NO_AUTH) {
        s.write_all(&[5, V5_NO_ACCEPTABLE_METHOD]).await?;
        return Err(HandshakeError::Unsupported("the client wants SOCKS authentication"));
    }
    s.write_all(&[5, V5_NO_AUTH]).await?;

    let mut head = [0u8; 4];
    s.read_exact(&mut head).await?;
    let [ver, cmd, _reserved, atyp] = head;
    if ver != 5 {
        return Err(HandshakeError::Unsupported("not a SOCKS5 request"));
    }
    let host = match atyp {
        0x01 => {
            let mut ip = [0u8; 4];
            s.read_exact(&mut ip).await?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        0x03 => {
            let len = s.read_u8().await? as usize;
            let mut name = vec![0u8; len];
            s.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|_| HandshakeError::Unsupported("host name is not UTF-8"))?
        }
        0x04 => {
            let mut ip = [0u8; 16];
            s.read_exact(&mut ip).await?;
            std::net::Ipv6Addr::from(ip).to_string()
        }
        _ => {
            write_v5(s, V5_REP_ADDRESS).await?;
            return Err(HandshakeError::Unsupported("unknown SOCKS5 address type"));
        }
    };
    let port = s.read_u16().await?;
    if cmd != CMD_CONNECT {
        // BIND and UDP ASSOCIATE have no SSH channel to ride on
        write_v5(s, V5_REP_COMMAND).await?;
        return Err(HandshakeError::Unsupported("only SOCKS CONNECT is supported"));
    }
    if host.is_empty() {
        write_v5(s, V5_REP_ADDRESS).await?;
        return Err(HandshakeError::Unsupported("empty host name"));
    }
    Ok(Request { version: Version::V5, host, port })
}

async fn handshake_v4<S>(s: &mut S) -> Result<Request, HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let cmd = s.read_u8().await?;
    let port = s.read_u16().await?;
    let mut ip = [0u8; 4];
    s.read_exact(&mut ip).await?;
    let _user_id = read_nul_string(s).await?;
    // SOCKS4a: 0.0.0.x with x != 0 means a host name follows the user id
    let host = if ip[..3] == [0, 0, 0] && ip[3] != 0 {
        let name = read_nul_string(s).await?;
        String::from_utf8(name).map_err(|_| HandshakeError::Unsupported("host name is not UTF-8"))?
    } else {
        std::net::Ipv4Addr::from(ip).to_string()
    };
    if cmd != CMD_CONNECT {
        write_v4(s, false).await?;
        return Err(HandshakeError::Unsupported("only SOCKS CONNECT is supported"));
    }
    if host.is_empty() {
        write_v4(s, false).await?;
        return Err(HandshakeError::Unsupported("empty host name"));
    }
    Ok(Request { version: Version::V4, host, port })
}

async fn read_nul_string<S>(s: &mut S) -> Result<Vec<u8>, HandshakeError>
where
    S: AsyncRead + Unpin,
{
    let mut out = Vec::new();
    loop {
        let b = s.read_u8().await?;
        if b == 0 {
            return Ok(out);
        }
        if out.len() == MAX_V4_STRING {
            return Err(HandshakeError::Unsupported("SOCKS4 field too long"));
        }
        out.push(b);
    }
}

async fn write_v5<S: AsyncWrite + Unpin>(s: &mut S, rep: u8) -> std::io::Result<()> {
    // The bound address is not meaningful here; 0.0.0.0:0 is what OpenSSH sends
    s.write_all(&[5, rep, 0, 0x01, 0, 0, 0, 0, 0, 0]).await?;
    s.flush().await
}

async fn write_v4<S: AsyncWrite + Unpin>(s: &mut S, ok: bool) -> std::io::Result<()> {
    s.write_all(&[0, if ok { 0x5a } else { 0x5b }, 0, 0, 0, 0, 0, 0]).await?;
    s.flush().await
}

/// Tell the client whether its connection was opened. After a success the
/// stream carries the connection's bytes.
pub async fn reply<S: AsyncWrite + Unpin>(
    s: &mut S,
    version: Version,
    result: Result<(), Failure>,
) -> std::io::Result<()> {
    match version {
        Version::V5 => {
            let rep = match result {
                Ok(()) => 0x00,
                Err(Failure::Refused) => V5_REP_REFUSED,
                Err(Failure::General) => V5_REP_GENERAL,
            };
            write_v5(s, rep).await
        }
        Version::V4 => write_v4(s, result.is_ok()).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Run the handshake against `client_bytes`; returns the request (or the
    /// error text) and everything the proxy wrote back
    async fn run(client_bytes: &[u8]) -> (Result<Request, String>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(client_bytes).await.unwrap();
        let result = handshake(&mut server).await.map_err(|e| e.to_string());
        drop(server);
        let mut written = Vec::new();
        client.read_to_end(&mut written).await.unwrap();
        (result, written)
    }

    #[tokio::test]
    async fn socks5_with_a_host_name() {
        let mut req = vec![5, 1, 0, 5, 1, 0, 3, 11];
        req.extend_from_slice(b"example.com");
        req.extend_from_slice(&443u16.to_be_bytes());
        let (result, written) = run(&req).await;
        assert_eq!(
            result,
            Ok(Request { version: Version::V5, host: "example.com".into(), port: 443 })
        );
        assert_eq!(written, [5, 0], "only the method choice is sent before the reply");
    }

    #[tokio::test]
    async fn socks5_with_ipv4_and_ipv6() {
        let (r, _) = run(&[5, 2, 2, 0, 5, 1, 0, 1, 10, 0, 4, 12, 0x15, 0x38]).await;
        assert_eq!(r, Ok(Request { version: Version::V5, host: "10.0.4.12".into(), port: 5432 }));

        let mut v6 = vec![5, 1, 0, 5, 1, 0, 4];
        v6.extend_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
        v6.extend_from_slice(&[0, 80]);
        let (r, _) = run(&v6).await;
        assert_eq!(r, Ok(Request { version: Version::V5, host: "::1".into(), port: 80 }));
    }

    #[tokio::test]
    async fn socks5_refuses_authentication_and_other_commands() {
        // Only username/password offered
        let (r, written) = run(&[5, 1, 2]).await;
        assert!(r.is_err());
        assert_eq!(written, [5, 0xff]);

        // UDP ASSOCIATE
        let (r, written) = run(&[5, 1, 0, 5, 3, 0, 1, 127, 0, 0, 1, 0, 53]).await;
        assert!(r.is_err());
        assert_eq!(written[..2], [5, 0]);
        assert_eq!(written[2..4], [5, V5_REP_COMMAND]);
    }

    #[tokio::test]
    async fn socks4_and_4a() {
        let (r, written) = run(&[4, 1, 0, 22, 192, 168, 1, 9, b'm', b'e', 0]).await;
        assert_eq!(r, Ok(Request { version: Version::V4, host: "192.168.1.9".into(), port: 22 }));
        assert!(written.is_empty());

        let mut req = vec![4, 1, 0x1f, 0x90, 0, 0, 0, 1, 0];
        req.extend_from_slice(b"intranet.local\0");
        let (r, _) = run(&req).await;
        assert_eq!(r, Ok(Request { version: Version::V4, host: "intranet.local".into(), port: 8080 }));
    }

    #[tokio::test]
    async fn not_socks_at_all() {
        let (r, written) = run(b"GET / HTTP/1.1\r\n\r\n").await;
        assert!(r.is_err());
        assert!(written.is_empty());
    }

    #[tokio::test]
    async fn replies_carry_the_outcome() {
        let mut out = Vec::new();
        reply(&mut out, Version::V5, Ok(())).await.unwrap();
        reply(&mut out, Version::V5, Err(Failure::Refused)).await.unwrap();
        reply(&mut out, Version::V4, Ok(())).await.unwrap();
        reply(&mut out, Version::V4, Err(Failure::General)).await.unwrap();
        assert_eq!(out[..2], [5, 0]);
        assert_eq!(out[10..12], [5, V5_REP_REFUSED]);
        assert_eq!(out[20..22], [0, 0x5a]);
        assert_eq!(out[28..30], [0, 0x5b]);
    }
}
