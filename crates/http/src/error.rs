/// Errors produced while reading or interpreting HTTP messages.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// Transport error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The peer closed the connection in the middle of a message.
    #[error("connection closed mid-message")]
    UnexpectedEof,
    /// The message head exceeded the size limit.
    #[error("message head larger than {0} bytes")]
    HeadTooLarge(usize),
    /// A chunk-size or trailer line exceeded the size limit.
    #[error("line too long")]
    LineTooLong,
    /// The head could not be parsed.
    #[error("malformed message: {0}")]
    Malformed(String),
    /// Body length could not be determined unambiguously.
    #[error("invalid message framing: {0}")]
    BadFraming(String),
    /// Invalid chunked transfer coding.
    #[error("invalid chunked encoding: {0}")]
    BadChunk(String),
    /// The request target is not usable.
    #[error("invalid request target: {0}")]
    BadTarget(String),
}
