/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// SQLite error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Filesystem error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The file is a SQLite database but not a ChosProxy project.
    #[error("not a ChosProxy project file")]
    NotAProject,
    /// The project was created by a newer version.
    #[error("project schema v{found} is newer than supported v{supported}; upgrade ChosProxy")]
    NewerSchema {
        /// Schema version in the file.
        found: i64,
        /// Latest version this build understands.
        supported: i64,
    },
    /// A stored blob is corrupt.
    #[error("corrupt blob {0}")]
    CorruptBlob(i64),
    /// The writer queue is full; the item was not stored.
    #[error("storage queue full")]
    QueueFull,
    /// The database has been closed.
    #[error("project is closed")]
    Closed,
    /// No exchange with this id.
    #[error("exchange {0} not found")]
    NotFound(i64),
    /// Invalid query parameters.
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    /// The project was opened read-only.
    #[error("the project is open read-only")]
    ReadOnly,
    /// The project is encrypted and no password was given.
    #[error("the file is encrypted: its password is needed")]
    Encrypted,
    /// The password does not open the project (or the file is not a
    /// database).
    #[error("wrong password, or the file is damaged or not a ChosProxy file")]
    WrongKey,
}
