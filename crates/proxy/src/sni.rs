//! Invisible listeners: the first bytes of a connection tell whether the
//! client speaks TLS directly (it does not know about the proxy), and the
//! ClientHello's server name says where it wanted to go.

use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

/// TLS record type of a handshake message.
pub(crate) const TLS_HANDSHAKE: u8 = 0x16;
/// Largest TLS record (16 KiB plus expansion) read to find the name.
const MAX_RECORD: usize = 16 * 1024 + 2048;
/// How long to wait for the client's first bytes.
const SNIFF_TIMEOUT: Duration = Duration::from_secs(10);

/// What the client sent first.
pub(crate) enum Sniffed {
    /// A TLS ClientHello, with its server name when present.
    Tls(Option<String>),
    /// Anything else (plain HTTP).
    Other,
}

/// Reads the client's first bytes without losing them: they are returned to
/// be replayed in front of the stream.
pub(crate) async fn sniff(stream: &mut TcpStream) -> (Vec<u8>, Option<Sniffed>) {
    let mut buf = Vec::with_capacity(1024);
    let read = async {
        loop {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf[0] != TLS_HANDSHAKE {
                return Some(Sniffed::Other);
            }
            if buf.len() >= 5 {
                let len = usize::from(u16::from_be_bytes([buf[3], buf[4]]));
                if len > MAX_RECORD {
                    return Some(Sniffed::Tls(None));
                }
                if buf.len() >= 5 + len {
                    return Some(Sniffed::Tls(server_name(&buf[5..5 + len])));
                }
            }
        }
    };
    let result = tokio::time::timeout(SNIFF_TIMEOUT, read)
        .await
        .ok()
        .flatten();
    (buf, result)
}

/// A cursor over bytes that fails instead of panicking.
struct Cur<'a>(&'a [u8]);

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<usize> {
        self.take(1).map(|b| usize::from(b[0]))
    }
    fn u16(&mut self) -> Option<usize> {
        self.take(2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
    }
}

/// The host name in a ClientHello handshake message (record payload).
pub(crate) fn server_name(handshake: &[u8]) -> Option<String> {
    let mut c = Cur(handshake);
    if c.u8()? != 1 {
        return None; // not a ClientHello
    }
    c.take(3)?; // length
    c.take(2 + 32)?; // version, random
    let sid = c.u8()?;
    c.take(sid)?;
    let suites = c.u16()?;
    c.take(suites)?;
    let comp = c.u8()?;
    c.take(comp)?;
    let ext_len = c.u16()?;
    let mut ext = Cur(c.take(ext_len)?);
    while !ext.0.is_empty() {
        let kind = ext.u16()?;
        let len = ext.u16()?;
        let data = ext.take(len)?;
        if kind != 0 {
            continue;
        }
        let mut list = Cur(data);
        let list_len = list.u16()?;
        let mut names = Cur(list.take(list_len)?);
        while !names.0.is_empty() {
            let name_type = names.u8()?;
            let n = names.u16()?;
            let name = names.take(n)?;
            if name_type == 0 {
                let s = std::str::from_utf8(name).ok()?;
                let ok = !s.is_empty()
                    && s.len() <= 253
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
                return ok.then(|| s.to_ascii_lowercase());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ClientHello handshake message carrying `name` (and one other
    /// extension before it).
    fn hello(name: &str) -> Vec<u8> {
        let mut sni = Vec::new();
        let entry_len = 3 + name.len();
        sni.extend_from_slice(&u16::try_from(entry_len).unwrap().to_be_bytes());
        sni.push(0);
        sni.extend_from_slice(&u16::try_from(name.len()).unwrap().to_be_bytes());
        sni.extend_from_slice(name.as_bytes());
        let mut exts = vec![0x00, 0x0b, 0x00, 0x02, 0x01, 0x00]; // ec_point_formats
        exts.extend_from_slice(&[0, 0]);
        exts.extend_from_slice(&u16::try_from(sni.len()).unwrap().to_be_bytes());
        exts.extend_from_slice(&sni);
        let mut body = vec![3, 3];
        body.extend_from_slice(&[7u8; 32]);
        body.push(0); // session id
        body.extend_from_slice(&[0, 2, 0x13, 0x01]); // one suite
        body.extend_from_slice(&[1, 0]); // null compression
        body.extend_from_slice(&u16::try_from(exts.len()).unwrap().to_be_bytes());
        body.extend_from_slice(&exts);
        let mut hs = vec![1];
        let len = u32::try_from(body.len()).unwrap().to_be_bytes();
        hs.extend_from_slice(&len[1..]);
        hs.extend_from_slice(&body);
        hs
    }

    #[test]
    fn finds_the_server_name() {
        assert_eq!(server_name(&hello("Lab.Test")).as_deref(), Some("lab.test"));
        let h = hello("lab.test");
        for cut in 0..h.len() {
            assert_eq!(server_name(&h[..cut]), None, "truncated at {cut}");
        }
        assert_eq!(server_name(&hello("bad name")), None);
        let mut not_hello = h.clone();
        not_hello[0] = 2;
        assert_eq!(server_name(&not_hello), None);
    }

    #[tokio::test]
    async fn sniff_keeps_the_bytes() {
        use tokio::io::AsyncWriteExt;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let hs = hello("app.lab.test");
        let mut record = vec![TLS_HANDSHAKE, 3, 1];
        record.extend_from_slice(&u16::try_from(hs.len()).unwrap().to_be_bytes());
        record.extend_from_slice(&hs);
        let sent = record.clone();
        tokio::spawn(async move {
            let mut c = TcpStream::connect(addr).await.unwrap();
            // Split the record in two writes.
            c.write_all(&sent[..7]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
            c.write_all(&sent[7..]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let (mut s, _) = l.accept().await.unwrap();
        let (bytes, what) = sniff(&mut s).await;
        assert_eq!(bytes, record);
        assert!(matches!(what, Some(Sniffed::Tls(Some(n))) if n == "app.lab.test"));
    }
}
