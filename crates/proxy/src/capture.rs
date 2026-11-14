//! Bounded accumulation of message bytes for storage.

/// Accumulates bytes up to a cap while counting everything seen.
#[derive(Debug)]
pub(crate) struct Capture {
    buf: Vec<u8>,
    cap: usize,
    total: u64,
    truncated: bool,
}

impl Capture {
    pub(crate) fn new(cap: usize) -> Capture {
        Capture {
            buf: Vec::new(),
            cap,
            total: 0,
            truncated: false,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        let room = self.cap.saturating_sub(self.buf.len());
        if bytes.len() > room {
            self.buf.extend_from_slice(&bytes[..room]);
            self.truncated = true;
        } else {
            self.buf.extend_from_slice(bytes);
        }
    }

    /// Bytes seen on the wire (not just stored).
    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    pub(crate) fn truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.buf
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_and_counts() {
        let mut c = Capture::new(5);
        c.push(b"abc");
        assert!(!c.truncated());
        c.push(b"defg");
        assert!(c.truncated());
        assert_eq!(c.total(), 7);
        assert_eq!(c.into_bytes(), b"abcde");
    }
}
