//! How outgoing connections are made (ADR-0029): directly, through an HTTP
//! proxy or through a SOCKS5 proxy, with host name overrides and timeouts
//! from `network.toml`. The settings are process-wide so that every sender
//! (proxy, Repeater, tools) follows them without passing them around.

use crate::io::{BoxIo, Prefixed};
use base64::Engine as _;
use chosproxy_core::config::{NetworkConfig, UpstreamKind, UpstreamRule};
use chosproxy_http::head::read_head;
use chosproxy_http::{MAX_HEAD_BYTES, ResponseHead};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

static NETWORK: RwLock<Option<Arc<NetworkConfig>>> = RwLock::new(None);

/// Replaces the outgoing connection settings (new connections use them).
pub fn set_network(config: NetworkConfig) {
    let value = Some(Arc::new(config));
    match NETWORK.write() {
        Ok(mut g) => *g = value,
        Err(poisoned) => *poisoned.into_inner() = value,
    }
}

/// The current outgoing connection settings.
pub fn network() -> Arc<NetworkConfig> {
    static DEFAULT: OnceLock<Arc<NetworkConfig>> = OnceLock::new();
    let current = match NETWORK.read() {
        Ok(g) => g.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    current.unwrap_or_else(|| DEFAULT.get_or_init(Default::default).clone())
}

/// Seconds as a duration.
pub(crate) fn secs(s: u32) -> Duration {
    Duration::from_secs(u64::from(s))
}

/// What the caller will do with the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// Plain HTTP requests: through an HTTP proxy they are sent in absolute
    /// form instead of opening a tunnel.
    PlainHttp,
    /// TLS or an opaque byte stream: always a tunnel.
    Tunnel,
}

/// Plain HTTP sent through an HTTP proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Forward {
    /// `http://host:port`, put before origin-form targets.
    pub(crate) origin: String,
    /// `Proxy-Authorization: …\r\n`, when the proxy needs credentials.
    pub(crate) auth_line: Option<String>,
}

/// A connected byte stream to the destination (or to a proxy carrying it).
pub(crate) struct Opened {
    pub(crate) io: BoxIo,
    /// The address actually connected to (the proxy, when there is one).
    pub(crate) peer: SocketAddr,
    pub(crate) dns_ms: u64,
    /// "HTTP proxy proxy.corp:3128", when not direct.
    pub(crate) via: Option<String>,
    pub(crate) forward: Option<Forward>,
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Resolves `host` (fixed address first), returning the addresses and the
/// lookup time.
async fn resolve(
    net: &NetworkConfig,
    host: &str,
    port: u16,
) -> Result<(Vec<SocketAddr>, u64), String> {
    if let Some(ip) = net.resolve(host) {
        return Ok((vec![SocketAddr::new(ip, port)], 0));
    }
    let started = Instant::now();
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("no addresses found for {host}"));
    }
    Ok((addrs, elapsed_ms(started)))
}

async fn tcp(addrs: &[SocketAddr]) -> Result<(TcpStream, SocketAddr), String> {
    let mut last = String::from("no address to connect to");
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok((stream, *addr));
            }
            Err(e) => last = format!("cannot connect to {addr}: {e}"),
        }
    }
    Err(last)
}

/// `host:port` with IPv6 addresses in brackets.
fn authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn basic(user: &str, password: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
}

/// Opens a stream to `host:port` following the rules, within the connect
/// timeout.
pub(crate) async fn open(host: &str, port: u16, purpose: Purpose) -> Result<Opened, String> {
    let net = network();
    let limit = secs(net.timeouts.connect_secs);
    let rule = net.route(host).filter(|r| r.kind != UpstreamKind::Direct);
    let attempt = async {
        match rule {
            None => {
                let (addrs, dns_ms) = resolve(&net, host, port).await?;
                let (stream, peer) = tcp(&addrs).await?;
                Ok(Opened {
                    io: Box::new(stream),
                    peer,
                    dns_ms,
                    via: None,
                    forward: None,
                })
            }
            Some(rule) => through_proxy(&net, rule, host, port, purpose).await,
        }
    };
    tokio::time::timeout(limit, attempt).await.map_err(|_| {
        let via = rule.map_or(String::new(), |r| format!(" through {}", describe(r)));
        format!(
            "timed out connecting to {}{via} (after {}s)",
            authority(host, port),
            limit.as_secs()
        )
    })?
}

fn describe(rule: &UpstreamRule) -> String {
    let kind = match rule.kind {
        UpstreamKind::Http => "HTTP proxy",
        UpstreamKind::Socks5 => "SOCKS5 proxy",
        UpstreamKind::Direct => "direct",
    };
    format!("{kind} {}", authority(&rule.proxy_host, rule.proxy_port))
}

async fn through_proxy(
    net: &NetworkConfig,
    rule: &UpstreamRule,
    host: &str,
    port: u16,
    purpose: Purpose,
) -> Result<Opened, String> {
    let via = describe(rule);
    let (addrs, dns_ms) = resolve(net, &rule.proxy_host, rule.proxy_port)
        .await
        .map_err(|e| format!("{via}: {e}"))?;
    let (mut stream, peer) = tcp(&addrs).await.map_err(|e| format!("{via}: {e}"))?;
    // A fixed address for the destination applies through the proxy too.
    let fixed = net.resolve(host);
    match rule.kind {
        UpstreamKind::Http if purpose == Purpose::PlainHttp => {
            let target = fixed.map_or_else(|| host.to_string(), |ip| ip.to_string());
            Ok(Opened {
                io: Box::new(stream),
                peer,
                dns_ms,
                via: Some(via),
                forward: Some(Forward {
                    origin: format!("http://{}", authority(&target, port)),
                    auth_line: (!rule.username.is_empty()).then(|| {
                        format!(
                            "Proxy-Authorization: Basic {}\r\n",
                            basic(&rule.username, &rule.password)
                        )
                    }),
                }),
            })
        }
        UpstreamKind::Http => {
            let target = fixed.map_or_else(|| host.to_string(), |ip| ip.to_string());
            let io = http_connect(stream, rule, &target, port)
                .await
                .map_err(|e| format!("{via}: {e}"))?;
            Ok(Opened {
                io,
                peer,
                dns_ms,
                via: Some(via),
                forward: None,
            })
        }
        UpstreamKind::Socks5 => {
            let target = match fixed {
                Some(ip) => SocksTarget::Ip(ip),
                None if rule.remote_dns => SocksTarget::Name(host.to_string()),
                None => {
                    let (addrs, _) = resolve(net, host, port).await?;
                    SocksTarget::Ip(addrs[0].ip())
                }
            };
            socks5(&mut stream, rule, &target, port)
                .await
                .map_err(|e| format!("{via}: {e}"))?;
            Ok(Opened {
                io: Box::new(stream),
                peer,
                dns_ms,
                via: Some(via),
                forward: None,
            })
        }
        UpstreamKind::Direct => Err("direct rules do not use a proxy".into()),
    }
}

/// Opens a tunnel with `CONNECT`; bytes the proxy sent after its answer are
/// kept.
async fn http_connect(
    stream: TcpStream,
    rule: &UpstreamRule,
    host: &str,
    port: u16,
) -> Result<BoxIo, String> {
    let target = authority(host, port);
    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if !rule.username.is_empty() {
        request.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            basic(&rule.username, &rule.password)
        ));
    }
    request.push_str("\r\n");
    let mut reader = BufReader::new(stream);
    reader
        .get_mut()
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("cannot send CONNECT: {e}"))?;
    let raw = read_head(&mut reader, MAX_HEAD_BYTES)
        .await
        .map_err(|e| format!("invalid answer to CONNECT: {e}"))?
        .ok_or("the proxy closed the connection")?;
    let head = ResponseHead::parse(raw).map_err(|e| format!("invalid answer to CONNECT: {e}"))?;
    if !(200..300).contains(&head.status) {
        let hint = if head.status == 407 {
            if rule.username.is_empty() {
                " (the proxy wants credentials)"
            } else {
                " (the proxy rejected the credentials)"
            }
        } else {
            ""
        };
        return Err(format!(
            "CONNECT {target} refused: {} {}{hint}",
            head.status, head.reason
        ));
    }
    let leftover = reader.buffer().to_vec();
    Ok(Box::new(Prefixed::new(leftover, reader.into_inner())))
}

/// Where a SOCKS5 proxy should connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SocksTarget {
    Ip(IpAddr),
    Name(String),
}

/// The SOCKS5 CONNECT request (RFC 1928).
pub(crate) fn socks_request(target: &SocksTarget, port: u16) -> Result<Vec<u8>, String> {
    let mut v = vec![5, 1, 0];
    match target {
        SocksTarget::Ip(IpAddr::V4(ip)) => {
            v.push(1);
            v.extend_from_slice(&ip.octets());
        }
        SocksTarget::Ip(IpAddr::V6(ip)) => {
            v.push(4);
            v.extend_from_slice(&ip.octets());
        }
        SocksTarget::Name(name) => {
            let len = u8::try_from(name.len()).map_err(|_| "host name too long for SOCKS5")?;
            v.push(3);
            v.push(len);
            v.extend_from_slice(name.as_bytes());
        }
    }
    v.extend_from_slice(&port.to_be_bytes());
    Ok(v)
}

fn socks_reply_error(code: u8) -> String {
    let what = match code {
        1 => "general failure",
        2 => "connection not allowed by the proxy's rules",
        3 => "network unreachable",
        4 => "host unreachable",
        5 => "connection refused",
        6 => "TTL expired",
        7 => "command not supported",
        8 => "address type not supported",
        _ => "unknown error",
    };
    format!("SOCKS5 connect failed: {what} ({code})")
}

async fn socks5(
    stream: &mut TcpStream,
    rule: &UpstreamRule,
    target: &SocksTarget,
    port: u16,
) -> Result<(), String> {
    let io = |e: std::io::Error| format!("SOCKS5: {e}");
    let with_auth = !rule.username.is_empty();
    let greeting: &[u8] = if with_auth { &[5, 2, 0, 2] } else { &[5, 1, 0] };
    stream.write_all(greeting).await.map_err(io)?;
    let mut choice = [0u8; 2];
    stream.read_exact(&mut choice).await.map_err(io)?;
    if choice[0] != 5 {
        return Err("not a SOCKS5 proxy".into());
    }
    match choice[1] {
        0 => {}
        2 if with_auth => {
            let user = rule.username.as_bytes();
            let pass = rule.password.as_bytes();
            let mut auth = vec![
                1,
                u8::try_from(user.len()).map_err(|_| "user name too long")?,
            ];
            auth.extend_from_slice(user);
            auth.push(u8::try_from(pass.len()).map_err(|_| "password too long")?);
            auth.extend_from_slice(pass);
            stream.write_all(&auth).await.map_err(io)?;
            let mut status = [0u8; 2];
            stream.read_exact(&mut status).await.map_err(io)?;
            if status[1] != 0 {
                return Err("the SOCKS5 proxy rejected the credentials".into());
            }
        }
        0xFF if !with_auth => {
            return Err("the SOCKS5 proxy wants credentials".into());
        }
        _ => return Err("the SOCKS5 proxy offered no usable authentication method".into()),
    }
    stream
        .write_all(&socks_request(target, port)?)
        .await
        .map_err(io)?;
    let mut reply = [0u8; 4];
    stream.read_exact(&mut reply).await.map_err(io)?;
    if reply[1] != 0 {
        return Err(socks_reply_error(reply[1]));
    }
    let skip = match reply[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await.map_err(io)?;
            usize::from(len[0])
        }
        _ => return Err("invalid SOCKS5 reply".into()),
    };
    let mut rest = vec![0u8; skip + 2];
    stream.read_exact(&mut rest).await.map_err(io)?;
    Ok(())
}

/// Rewrites the request line of `bytes` for an HTTP proxy: an origin-form
/// target gets `origin` in front, and the proxy credentials are added after
/// the request line. Everything else is left byte for byte.
pub(crate) fn absolute_form(bytes: &[u8], forward: &Forward) -> Vec<u8> {
    let Some(line_end) = bytes.iter().position(|b| *b == b'\n').map(|i| i + 1) else {
        return bytes.to_vec();
    };
    let line = &bytes[..line_end];
    let mut out = Vec::with_capacity(bytes.len() + forward.origin.len() + 64);
    match line.iter().position(|b| *b == b' ') {
        Some(sp) if line.get(sp + 1) == Some(&b'/') => {
            out.extend_from_slice(&line[..=sp]);
            out.extend_from_slice(forward.origin.as_bytes());
            out.extend_from_slice(&line[sp + 1..]);
        }
        _ => out.extend_from_slice(line),
    }
    let rest = &bytes[line_end..];
    if let Some(auth) = &forward.auth_line
        && !has_header(rest, b"proxy-authorization")
    {
        out.extend_from_slice(auth.as_bytes());
    }
    out.extend_from_slice(rest);
    out
}

/// True if the head at the start of `rest` has header `name` (lower case).
fn has_header(rest: &[u8], name: &[u8]) -> bool {
    for line in rest.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return false;
        }
        if line.len() > name.len()
            && line[name.len()] == b':'
            && line[..name.len()].eq_ignore_ascii_case(name)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chosproxy_core::config::HostOverride;
    use tokio::net::TcpListener;

    #[test]
    fn absolute_form_rewrites_only_the_target() {
        let f = Forward {
            origin: "http://lab.test:8080".into(),
            auth_line: Some("Proxy-Authorization: Basic eDp5\r\n".into()),
        };
        let out = absolute_form(b"GET /a?b HTTP/1.1\r\nHost: lab.test:8080\r\n\r\nbody", &f);
        assert_eq!(
            out,
            b"GET http://lab.test:8080/a?b HTTP/1.1\r\nProxy-Authorization: Basic eDp5\r\nHost: lab.test:8080\r\n\r\nbody"
        );
        let already = absolute_form(
            b"GET http://x/ HTTP/1.1\r\nproxy-authorization: Basic z\r\n\r\n",
            &f,
        );
        assert_eq!(
            already,
            b"GET http://x/ HTTP/1.1\r\nproxy-authorization: Basic z\r\n\r\n"
        );
    }

    #[test]
    fn socks_requests() {
        assert_eq!(
            socks_request(&SocksTarget::Ip("10.0.0.1".parse().unwrap()), 443).unwrap(),
            vec![5, 1, 0, 1, 10, 0, 0, 1, 1, 187]
        );
        assert_eq!(
            socks_request(&SocksTarget::Name("a.test".into()), 80).unwrap(),
            vec![5, 1, 0, 3, 6, b'a', b'.', b't', b'e', b's', b't', 0, 80]
        );
        assert!(socks_request(&SocksTarget::Name("x".repeat(300)), 80).is_err());
    }

    fn rule(kind: UpstreamKind, port: u16) -> UpstreamRule {
        UpstreamRule {
            enabled: true,
            host: "*".into(),
            kind,
            proxy_host: "127.0.0.1".into(),
            proxy_port: port,
            username: "u".into(),
            password: "p".into(),
            remote_dns: true,
        }
    }

    /// A minimal SOCKS5 server that accepts user u / password p and checks
    /// the request, then echoes.
    async fn socks_server() -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut g = [0u8; 4];
            s.read_exact(&mut g).await.unwrap();
            assert_eq!(g, [5, 2, 0, 2]);
            s.write_all(&[5, 2]).await.unwrap();
            let mut a = [0u8; 5];
            s.read_exact(&mut a).await.unwrap();
            assert_eq!(a, [1, 1, b'u', 1, b'p']);
            s.write_all(&[1, 0]).await.unwrap();
            let mut r = vec![0u8; 4 + 1 + 8 + 2];
            s.read_exact(&mut r).await.unwrap();
            assert_eq!(&r[..5], &[5, 1, 0, 3, 8]);
            assert_eq!(&r[5..13], b"lab.test");
            s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
            let mut buf = [0u8; 4];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(&buf).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn socks5_with_credentials_and_remote_dns() {
        let addr = socks_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        socks5(
            &mut stream,
            &rule(UpstreamKind::Socks5, addr.port()),
            &SocksTarget::Name("lab.test".into()),
            80,
        )
        .await
        .unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut back = [0u8; 4];
        stream.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"ping");
    }

    #[tokio::test]
    async fn http_connect_reports_refusals() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            for answer in [
                "HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                "HTTP/1.1 200 Connection established\r\n\r\nhello",
            ] {
                let (s, _) = l.accept().await.unwrap();
                let mut r = BufReader::new(s);
                let head = read_head(&mut r, MAX_HEAD_BYTES).await.unwrap().unwrap();
                let text = String::from_utf8_lossy(&head).to_string();
                assert!(text.starts_with("CONNECT lab.test:443 HTTP/1.1\r\n"));
                assert!(text.contains("Proxy-Authorization: Basic dTpw\r\n"));
                r.get_mut().write_all(answer.as_bytes()).await.unwrap();
                let _ = r.get_mut().flush().await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        let r = rule(UpstreamKind::Http, addr.port());
        let first = TcpStream::connect(addr).await.unwrap();
        let err = http_connect(first, &r, "lab.test", 443)
            .await
            .err()
            .unwrap();
        assert!(err.contains("407") && err.contains("rejected"), "{err}");
        let second = TcpStream::connect(addr).await.unwrap();
        let mut io = http_connect(second, &r, "lab.test", 443).await.unwrap();
        let mut hello = [0u8; 5];
        io.read_exact(&mut hello).await.unwrap();
        assert_eq!(&hello, b"hello", "bytes after the answer are kept");
    }

    #[tokio::test]
    async fn fixed_addresses_skip_dns() {
        let net = NetworkConfig {
            hosts: vec![HostOverride {
                enabled: true,
                host: "nowhere.invalid".into(),
                address: "127.0.0.1".parse().unwrap(),
            }],
            ..NetworkConfig::default()
        };
        let (addrs, _) = resolve(&net, "nowhere.invalid", 81).await.unwrap();
        assert_eq!(addrs, vec!["127.0.0.1:81".parse().unwrap()]);
    }
}
