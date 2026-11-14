//! WebSocket (RFC 6455) frames: reading them exactly as they arrive (so
//! unmodified frames can be forwarded byte for byte), encoding new ones, and
//! assembling fragmented messages.

use ring::rand::SecureRandom;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Largest frame payload accepted (larger frames end the connection).
pub const MAX_FRAME_PAYLOAD: u64 = 64 * 1024 * 1024;

/// Frame opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    /// Continuation of a fragmented message.
    Continuation,
    /// UTF-8 text.
    Text,
    /// Binary data.
    Binary,
    /// Connection close.
    Close,
    /// Ping.
    Ping,
    /// Pong.
    Pong,
    /// Reserved opcode.
    Reserved(u8),
}

impl Opcode {
    /// From the 4-bit wire value.
    pub fn from_u8(v: u8) -> Opcode {
        match v {
            0 => Opcode::Continuation,
            1 => Opcode::Text,
            2 => Opcode::Binary,
            8 => Opcode::Close,
            9 => Opcode::Ping,
            10 => Opcode::Pong,
            other => Opcode::Reserved(other),
        }
    }

    /// The 4-bit wire value.
    pub fn as_u8(self) -> u8 {
        match self {
            Opcode::Continuation => 0,
            Opcode::Text => 1,
            Opcode::Binary => 2,
            Opcode::Close => 8,
            Opcode::Ping => 9,
            Opcode::Pong => 10,
            Opcode::Reserved(v) => v,
        }
    }

    /// Close, ping and pong.
    pub fn is_control(self) -> bool {
        self.as_u8() >= 8
    }

    /// Lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            Opcode::Continuation => "continuation",
            Opcode::Text => "text",
            Opcode::Binary => "binary",
            Opcode::Close => "close",
            Opcode::Ping => "ping",
            Opcode::Pong => "pong",
            Opcode::Reserved(_) => "reserved",
        }
    }
}

/// Reading a frame failed.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The connection failed or ended inside a frame.
    #[error("connection error: {0}")]
    Io(#[from] std::io::Error),
    /// The frame announces a payload above [`MAX_FRAME_PAYLOAD`].
    #[error("frame payload of {0} bytes exceeds the limit")]
    TooLarge(u64),
}

/// One frame as read from the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Final fragment of a message.
    pub fin: bool,
    /// RSV1–3 bits (non-zero means an extension such as compression).
    pub rsv: u8,
    /// Opcode.
    pub opcode: Opcode,
    /// The payload was masked (client-to-server frames always are).
    pub masked: bool,
    /// Unmasked payload.
    pub payload: Vec<u8>,
    /// The exact bytes of the frame (header + masked payload).
    pub raw: Vec<u8>,
}

/// Reads one frame. `Ok(None)` when the connection ends cleanly before a
/// new frame starts.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Frame>, FrameError> {
    let mut head = [0u8; 2];
    match reader.read(&mut head[..1]).await? {
        0 => return Ok(None),
        _ => reader.read_exact(&mut head[1..]).await?,
    };
    let mut raw = head.to_vec();
    let fin = head[0] & 0x80 != 0;
    let rsv = (head[0] >> 4) & 0x07;
    let opcode = Opcode::from_u8(head[0] & 0x0f);
    let masked = head[1] & 0x80 != 0;
    let len = match head[1] & 0x7f {
        126 => {
            let mut b = [0u8; 2];
            reader.read_exact(&mut b).await?;
            raw.extend_from_slice(&b);
            u64::from(u16::from_be_bytes(b))
        }
        127 => {
            let mut b = [0u8; 8];
            reader.read_exact(&mut b).await?;
            raw.extend_from_slice(&b);
            u64::from_be_bytes(b)
        }
        n => u64::from(n),
    };
    if len > MAX_FRAME_PAYLOAD {
        return Err(FrameError::TooLarge(len));
    }
    let mut mask = [0u8; 4];
    if masked {
        reader.read_exact(&mut mask).await?;
        raw.extend_from_slice(&mask);
    }
    let len = usize::try_from(len).map_err(|_| FrameError::TooLarge(len))?;
    let start = raw.len();
    raw.resize(start + len, 0);
    reader.read_exact(&mut raw[start..]).await?;
    let mut payload = raw[start..].to_vec();
    if masked {
        apply_mask(&mut payload, mask);
    }
    Ok(Some(Frame {
        fin,
        rsv,
        opcode,
        masked,
        payload,
        raw,
    }))
}

fn apply_mask(data: &mut [u8], mask: [u8; 4]) {
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= mask[i % 4];
    }
}

/// A fresh random masking key.
pub fn random_mask() -> [u8; 4] {
    let mut mask = [0u8; 4];
    if ring::rand::SystemRandom::new().fill(&mut mask).is_err() {
        // Masking only protects intermediaries from cache poisoning; a
        // time-derived key is an acceptable fallback.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0x5a5a_5a5a);
        mask = t.to_le_bytes();
    }
    mask
}

/// Encodes a single, final frame. `mask` must be set for client-to-server
/// frames.
pub fn encode_frame(opcode: Opcode, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | (opcode.as_u8() & 0x0f));
    let mask_bit = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => out.push(mask_bit | n as u8),
        n if n <= usize::from(u16::MAX) => {
            out.push(mask_bit | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(mask_bit | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    let start = out.len();
    if let Some(m) = mask {
        out.extend_from_slice(&m);
        out.extend_from_slice(payload);
        apply_mask(&mut out[start + 4..], m);
    } else {
        out.extend_from_slice(payload);
    }
    out
}

/// Close code and reason from a close frame payload.
pub fn parse_close(payload: &[u8]) -> (Option<u16>, String) {
    if payload.len() < 2 {
        return (None, String::new());
    }
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    (
        Some(code),
        String::from_utf8_lossy(&payload[2..]).into_owned(),
    )
}

/// A complete message: one unfragmented frame, or a data frame plus its
/// continuations, or a control frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Text, binary or a control opcode.
    pub opcode: Opcode,
    /// Payload (fragments joined).
    pub payload: Vec<u8>,
    /// The frames as they arrived (to forward unmodified).
    pub frames: Vec<Vec<u8>>,
    /// An extension bit was set (e.g. compression): the payload is not plain.
    pub extension: bool,
}

/// Joins fragments into messages. Control frames, which may arrive between
/// fragments, are returned immediately.
#[derive(Debug, Default)]
pub struct Assembler {
    partial: Option<Message>,
}

impl Assembler {
    /// Adds a frame; returns a message when one is complete.
    pub fn push(&mut self, frame: Frame) -> Option<Message> {
        let extension = frame.rsv != 0;
        if frame.opcode.is_control() || (frame.opcode != Opcode::Continuation && frame.fin) {
            if !frame.opcode.is_control() {
                // A new data message replaces any unfinished one (protocol error upstream).
                self.partial = None;
            }
            return Some(Message {
                opcode: frame.opcode,
                payload: frame.payload,
                frames: vec![frame.raw],
                extension,
            });
        }
        match (&mut self.partial, frame.opcode) {
            (Some(m), Opcode::Continuation) => {
                m.payload.extend_from_slice(&frame.payload);
                m.frames.push(frame.raw);
                m.extension |= extension;
            }
            (_, Opcode::Continuation) => {
                // A continuation without a start: pass it on as it is.
                return Some(Message {
                    opcode: Opcode::Continuation,
                    payload: frame.payload,
                    frames: vec![frame.raw],
                    extension,
                });
            }
            (_, opcode) => {
                self.partial = Some(Message {
                    opcode,
                    payload: frame.payload,
                    frames: vec![frame.raw],
                    extension,
                });
            }
        }
        if frame.fin { self.partial.take() } else { None }
    }

    /// Frames of an unfinished message (to forward if the connection ends).
    pub fn pending_frames(&mut self) -> Vec<Vec<u8>> {
        self.partial.take().map(|m| m.frames).unwrap_or_default()
    }
}

/// A random `Sec-WebSocket-Key` for a client handshake.
pub fn random_key() -> String {
    let mut raw = [0u8; 16];
    let _ = ring::rand::SystemRandom::new().fill(&mut raw);
    base64_encode(&raw)
}

/// `Sec-WebSocket-Accept` for a `Sec-WebSocket-Key`.
pub fn accept_key(key: &str) -> String {
    const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{}{GUID}", key.trim()).as_bytes(),
    );
    base64_encode(digest.as_ref())
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read_all(bytes: &[u8]) -> Vec<Frame> {
        let mut r = bytes;
        let mut out = Vec::new();
        while let Some(f) = read_frame(&mut r).await.unwrap() {
            out.push(f);
        }
        out
    }

    #[tokio::test]
    async fn rfc_examples() {
        // RFC 6455 §5.7: unmasked "Hello" and masked "Hello".
        let unmasked = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let masked = [
            0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
        ];
        for bytes in [&unmasked[..], &masked[..]] {
            let f = read_all(bytes).await.remove(0);
            assert!(f.fin);
            assert_eq!(f.opcode, Opcode::Text);
            assert_eq!(f.payload, b"Hello");
            assert_eq!(f.raw, bytes, "raw bytes kept exactly");
        }
        assert_eq!(encode_frame(Opcode::Text, b"Hello", None), unmasked);
        assert_eq!(
            encode_frame(Opcode::Text, b"Hello", Some([0x37, 0xfa, 0x21, 0x3d])),
            masked
        );
    }

    #[tokio::test]
    async fn extended_lengths_round_trip() {
        for len in [125usize, 126, 65535, 65536] {
            let payload = vec![7u8; len];
            let mask = random_mask();
            let bytes = encode_frame(Opcode::Binary, &payload, Some(mask));
            let f = read_all(&bytes).await.remove(0);
            assert_eq!(f.payload.len(), len);
            assert!(f.masked);
            assert_eq!(f.raw, bytes);
        }
    }

    #[tokio::test]
    async fn limits_and_truncation() {
        let huge = [0x82, 0x7f, 0, 0, 0, 1, 0, 0, 0, 0];
        assert!(matches!(
            read_frame(&mut &huge[..]).await,
            Err(FrameError::TooLarge(_))
        ));
        let cut = [0x81, 0x05, b'H'];
        assert!(read_frame(&mut &cut[..]).await.is_err());
        assert!(read_frame(&mut &[][..]).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn assembles_fragments_around_control_frames() {
        // RFC 6455 §5.4 example: "Hel" + ping + "lo".
        let mut bytes = vec![0x01, 0x03, b'H', b'e', b'l'];
        bytes.extend(encode_frame(Opcode::Ping, b"p", None));
        bytes.extend([0x80, 0x02, b'l', b'o']);
        let mut a = Assembler::default();
        let mut out = Vec::new();
        for f in read_all(&bytes).await {
            out.extend(a.push(f));
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].opcode, Opcode::Ping);
        assert_eq!(out[1].opcode, Opcode::Text);
        assert_eq!(out[1].payload, b"Hello");
        assert_eq!(out[1].frames.len(), 2);
        assert_eq!(out[1].frames.concat(), [&bytes[..5], &bytes[8..]].concat());
    }

    #[test]
    fn close_and_accept() {
        assert_eq!(
            parse_close(&[0x03, 0xe8, b'b', b'y', b'e']),
            (Some(1000), "bye".into())
        );
        assert_eq!(parse_close(&[]), (None, String::new()));
        // RFC 6455 §1.3 example.
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert!(Opcode::Close.is_control() && !Opcode::Text.is_control());
    }
}
