//! Streaming body reader that yields raw framing bytes.
//!
//! The reader never rewrites what it reads: for chunked bodies it yields the
//! chunk-size lines, data, CRLFs and trailers exactly as received, so the
//! concatenation of all yielded pieces equals the bytes on the wire.

use crate::HttpError;
use crate::framing::BodyKind;
use crate::head::trim_eol;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Maximum bytes returned by one call to [`BodyReader::next_chunk`].
const PIECE: usize = 64 * 1024;
/// Maximum length of a chunk-size or trailer line.
const MAX_LINE: usize = 8 * 1024;
/// Maximum total size of the trailer section.
const MAX_TRAILERS: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Length { remaining: u64 },
    ChunkSize,
    ChunkData { remaining: u64 },
    ChunkDataEnd,
    Trailers { seen: usize },
    UntilClose,
    Done,
}

/// Incremental reader for one message body.
#[derive(Debug)]
pub struct BodyReader {
    state: State,
}

impl BodyReader {
    /// Creates a reader for a body framed as `kind`.
    pub fn new(kind: BodyKind) -> BodyReader {
        let state = match kind {
            BodyKind::Empty | BodyKind::Length(0) => State::Done,
            BodyKind::Length(n) => State::Length { remaining: n },
            BodyKind::Chunked => State::ChunkSize,
            BodyKind::UntilClose => State::UntilClose,
        };
        BodyReader { state }
    }

    /// True once the whole body has been read.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// True if the next piece is body data (not chunk framing or trailers).
    pub fn next_is_data(&self) -> bool {
        matches!(
            self.state,
            State::Length { .. } | State::ChunkData { .. } | State::UntilClose
        )
    }

    /// Returns the next piece of raw body bytes, or `None` at the end.
    pub async fn next_chunk<R>(&mut self, r: &mut R) -> Result<Option<Vec<u8>>, HttpError>
    where
        R: AsyncBufRead + Unpin,
    {
        match self.state {
            State::Done => Ok(None),
            State::Length { remaining } => {
                let piece = read_some(r, remaining).await?;
                let left = remaining - piece.len() as u64;
                self.state = if left == 0 {
                    State::Done
                } else {
                    State::Length { remaining: left }
                };
                Ok(Some(piece))
            }
            State::ChunkData { remaining } => {
                let piece = read_some(r, remaining).await?;
                let left = remaining - piece.len() as u64;
                self.state = if left == 0 {
                    State::ChunkDataEnd
                } else {
                    State::ChunkData { remaining: left }
                };
                Ok(Some(piece))
            }
            State::ChunkSize => {
                let line = read_line(r).await?;
                let size = parse_chunk_size(&line)?;
                self.state = if size == 0 {
                    State::Trailers { seen: 0 }
                } else {
                    State::ChunkData { remaining: size }
                };
                Ok(Some(line))
            }
            State::ChunkDataEnd => {
                let line = read_line(r).await?;
                if !trim_eol(&line).is_empty() {
                    return Err(HttpError::BadChunk("missing CRLF after chunk data".into()));
                }
                self.state = State::ChunkSize;
                Ok(Some(line))
            }
            State::Trailers { seen } => {
                let line = read_line(r).await?;
                let seen = seen + line.len();
                if seen > MAX_TRAILERS {
                    return Err(HttpError::BadChunk("trailer section too large".into()));
                }
                self.state = if trim_eol(&line).is_empty() {
                    State::Done
                } else {
                    State::Trailers { seen }
                };
                Ok(Some(line))
            }
            State::UntilClose => {
                let avail = r.fill_buf().await?;
                if avail.is_empty() {
                    self.state = State::Done;
                    return Ok(None);
                }
                let n = avail.len().min(PIECE);
                let piece = avail[..n].to_vec();
                r.consume(n);
                Ok(Some(piece))
            }
        }
    }
}

async fn read_some<R: AsyncBufRead + Unpin>(r: &mut R, limit: u64) -> Result<Vec<u8>, HttpError> {
    let avail = r.fill_buf().await?;
    if avail.is_empty() {
        return Err(HttpError::UnexpectedEof);
    }
    let n = avail
        .len()
        .min(PIECE)
        .min(usize::try_from(limit).unwrap_or(usize::MAX));
    let piece = avail[..n].to_vec();
    r.consume(n);
    Ok(piece)
}

async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R) -> Result<Vec<u8>, HttpError> {
    let mut line = Vec::new();
    loop {
        let avail = r.fill_buf().await?;
        if avail.is_empty() {
            return Err(HttpError::UnexpectedEof);
        }
        match memchr::memchr(b'\n', avail) {
            Some(i) => {
                line.extend_from_slice(&avail[..=i]);
                r.consume(i + 1);
                if line.len() > MAX_LINE {
                    return Err(HttpError::LineTooLong);
                }
                return Ok(line);
            }
            None => {
                let n = avail.len();
                line.extend_from_slice(avail);
                r.consume(n);
                if line.len() > MAX_LINE {
                    return Err(HttpError::LineTooLong);
                }
            }
        }
    }
}

/// Parses a chunk-size line (`1a;ext=v\r\n`).
pub(crate) fn parse_chunk_size(line: &[u8]) -> Result<u64, HttpError> {
    let line = trim_eol(line);
    let size_part = line.split(|b| *b == b';').next().unwrap_or_default();
    let size_part = size_part.trim_ascii();
    if size_part.is_empty() || size_part.len() > 16 || !size_part.iter().all(u8::is_ascii_hexdigit)
    {
        return Err(HttpError::BadChunk(format!(
            "invalid chunk size {:?}",
            String::from_utf8_lossy(size_part)
        )));
    }
    let text =
        std::str::from_utf8(size_part).map_err(|_| HttpError::BadChunk("non-ASCII".into()))?;
    u64::from_str_radix(text, 16).map_err(|e| HttpError::BadChunk(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, BufReader};

    async fn read_all(
        kind: BodyKind,
        data: &[u8],
        cap: usize,
    ) -> Result<(Vec<u8>, Vec<u8>), HttpError> {
        let mut r = BufReader::with_capacity(cap, data);
        let mut br = BodyReader::new(kind);
        let mut out = Vec::new();
        while let Some(p) = br.next_chunk(&mut r).await? {
            out.extend(p);
        }
        assert!(br.is_done());
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).await.unwrap();
        Ok((out, rest))
    }

    #[tokio::test]
    async fn length_body_leaves_next_message() {
        for cap in [1, 3, 4096] {
            let (body, rest) = read_all(BodyKind::Length(5), b"helloGET", cap)
                .await
                .unwrap();
            assert_eq!(body, b"hello");
            assert_eq!(rest, b"GET");
        }
    }

    #[tokio::test]
    async fn chunked_is_byte_exact_including_extensions_and_trailers() {
        let wire = b"5;ext=1\r\nhello\r\n0\r\nX-T: 1\r\n\r\nNEXT";
        for cap in [1, 2, 7, 4096] {
            let (body, rest) = read_all(BodyKind::Chunked, wire, cap).await.unwrap();
            assert_eq!(body, &wire[..wire.len() - 4]);
            assert_eq!(rest, b"NEXT");
        }
    }

    #[tokio::test]
    async fn chunked_accepts_bare_lf() {
        let wire = b"3\nabc\n0\n\n";
        let (body, _) = read_all(BodyKind::Chunked, wire, 4096).await.unwrap();
        assert_eq!(body, wire);
    }

    #[tokio::test]
    async fn chunked_errors() {
        assert!(matches!(
            read_all(BodyKind::Chunked, b"zz\r\n", 64).await,
            Err(HttpError::BadChunk(_))
        ));
        assert!(matches!(
            read_all(BodyKind::Chunked, b"3\r\nabcX\r\n", 64).await,
            Err(HttpError::BadChunk(_))
        ));
        assert!(matches!(
            read_all(BodyKind::Chunked, b"5\r\nab", 64).await,
            Err(HttpError::UnexpectedEof)
        ));
        assert!(matches!(
            read_all(BodyKind::Chunked, b"fffffffffffffffff\r\n", 64).await,
            Err(HttpError::BadChunk(_))
        ));
        let long = vec![b'1'; MAX_LINE + 10];
        assert!(matches!(
            read_all(BodyKind::Chunked, &long, 64).await,
            Err(HttpError::LineTooLong)
        ));
    }

    #[tokio::test]
    async fn until_close_and_empty() {
        let (body, _) = read_all(BodyKind::UntilClose, b"all of it", 2)
            .await
            .unwrap();
        assert_eq!(body, b"all of it");
        let (body, rest) = read_all(BodyKind::Empty, b"x", 2).await.unwrap();
        assert!(body.is_empty());
        assert_eq!(rest, b"x");
        assert!(matches!(
            read_all(BodyKind::Length(10), b"short", 64).await,
            Err(HttpError::UnexpectedEof)
        ));
    }
}
