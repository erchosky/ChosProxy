//! Wire-exact HTTP/1.1 codec (see ADR-0003).
//!
//! Raw bytes are the source of truth. Heads are tokenized with `httparse`,
//! bodies are framed per RFC 9112 and streamed as *raw framing bytes*, so a
//! message can be forwarded and stored exactly as it crossed the wire.
//! Parsed views (method, headers, target) are derived from the raw bytes and
//! are never re-serialized implicitly.

pub mod body;
pub mod content;
pub mod cookies;
pub mod edit;
mod error;
pub mod framing;
pub mod head;
pub mod rewrite;
pub mod target;

pub use body::BodyReader;
pub use error::HttpError;
pub use framing::{BodyKind, Framing};
pub use head::{Header, RequestHead, ResponseHead, Version};
pub use target::{RequestTarget, Scheme};

/// Largest accepted message head (request line/status line + headers).
pub const MAX_HEAD_BYTES: usize = 64 * 1024;
/// Largest accepted number of header fields.
pub const MAX_HEADERS: usize = 256;
