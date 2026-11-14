//! The project database handle.

use crate::key::ProjectKey;
use crate::model::{
    ExchangeId, Highlight, HistoryOrder, HistoryPage, HistoryQuery, HistoryRow, HistorySort,
    NewExchange, ProjectStats, SiteMap, SiteMapEntry, SiteMark, SiteMarkEntry, StoredExchange,
    flags,
};
use crate::{StorageError, blob, migrations};
use chosproxy_core::websocket::WsEvent;
use chosproxy_core::{Scope, ScopeTarget, Source};
use regex::{Regex, RegexBuilder};
use rusqlite::functions::FunctionFlags;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Exchanges committed in one writer transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertBatch {
    /// Highest id in the batch.
    pub last_id: ExchangeId,
    /// Number of exchanges in the batch.
    pub count: u32,
    /// WebSocket events (connections, messages, closes) in the batch.
    pub ws_events: u32,
}

/// Callback invoked on the writer thread after each committed batch.
pub type BatchHook = Arc<dyn Fn(InsertBatch) + Send + Sync>;

/// Options for [`ProjectDb::open`].
#[derive(Clone, Default)]
pub struct OpenOptions {
    /// Called after each committed batch of inserts.
    pub on_insert: Option<BatchHook>,
    /// Capacity of the write queue (default 2048).
    pub queue_capacity: Option<usize>,
    /// Open without writing anything (M31 recovery): no migration, no
    /// writer; every write fails with [`StorageError::ReadOnly`]. The file
    /// must exist and use the current schema.
    pub read_only: bool,
    /// Key of an encrypted project (M31). A new file opened with a key is
    /// created encrypted; an existing one must match it.
    pub key: Option<ProjectKey>,
}

impl std::fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenOptions")
            .field("on_insert", &self.on_insert.is_some())
            .field("queue_capacity", &self.queue_capacity)
            .field("read_only", &self.read_only)
            .field("encrypted", &self.key.is_some())
            .finish()
    }
}

type Reply = SyncSender<Result<ExchangeId, StorageError>>;
type UnitReply = SyncSender<Result<(), StorageError>>;
/// Delivers a generic write's result once the batch is (or failed to be) committed.
type Finisher = Box<dyn FnOnce(bool) + Send>;
/// A generic write: runs, reports success, and returns its finisher.
type WriteJob = Box<dyn FnOnce(&Connection) -> (bool, Finisher) + Send>;

enum WriteCmd {
    Insert(Box<NewExchange>, Option<Reply>),
    Ws(Box<WsEvent>),
    Annotate {
        id: ExchangeId,
        notes: Option<String>,
        highlight: Option<Highlight>,
        reply: UnitReply,
    },
    Mark {
        node: SiteMarkEntry,
        clear: bool,
        reply: UnitReply,
    },
    /// Any other write, run in a savepoint of the writer's transaction.
    Run(WriteJob),
    Flush(SyncSender<()>),
    /// Changes the encryption key (outside any transaction).
    Rekey(ProjectKey, UnitReply),
}

/// The scope used by `chos_in_scope()` in queries; replaced when it changes.
pub(crate) type SharedScope = Arc<RwLock<Arc<Scope>>>;

/// Longest note stored on an exchange.
pub const MAX_NOTE_CHARS: usize = 10_000;
/// Upper bound for one compiled search regular expression.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Compiles a search expression (case-insensitive unless it says `(?-i)`).
pub fn search_regex(pattern: &str) -> Result<Regex, StorageError> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| StorageError::InvalidQuery(format!("invalid regular expression: {e}")))
}

pub(crate) fn register_functions(c: &Connection, scope: SharedScope) -> rusqlite::Result<()> {
    c.create_scalar_function("chos_in_scope", 4, FunctionFlags::SQLITE_UTF8, move |ctx| {
        let scheme: String = ctx.get(0)?;
        let host: String = ctx.get(1)?;
        let port: i64 = ctx.get(2)?;
        let path: String = ctx.get(3)?;
        let current = match scope.read() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        Ok(current.contains(&ScopeTarget {
            scheme: &scheme,
            host: &host,
            port: u16::try_from(port).unwrap_or(0),
            path: &path,
        }))
    })?;
    c.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let re = ctx.get_or_create_aux(0, |v| -> Result<Regex, StorageError> {
                match v {
                    ValueRef::Text(t) => search_regex(&String::from_utf8_lossy(t)),
                    _ => Err(StorageError::InvalidQuery("regex must be text".into())),
                }
            })?;
            Ok(match ctx.get_raw(1) {
                ValueRef::Text(t) => re.is_match(&String::from_utf8_lossy(t)),
                _ => false,
            })
        },
    )
}

/// Most inserts committed in a single transaction.
const MAX_BATCH: usize = 256;
/// Idle read connections kept open.
const MAX_IDLE_READERS: usize = 10;

/// Handle to an open project database. Cheap to clone.
#[derive(Clone, Debug)]
pub struct ProjectDb {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    sender: Option<SyncSender<WriteCmd>>,
    writer: Option<JoinHandle<()>>,
    readers: Mutex<Vec<Connection>>,
    scope: SharedScope,
    read_only: bool,
    key: RwLock<Option<ProjectKey>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectDb")
            .field("path", &self.path)
            .finish()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Closing the queue makes the writer drain remaining items and exit.
        drop(self.sender.take());
        if let Some(handle) = self.writer.take()
            && handle.join().is_err()
        {
            tracing::error!("project writer thread panicked");
        }
    }
}

/// A connection that cannot write (SQLite read-only, `query_only`).
pub(crate) fn read_only_connection(
    path: &Path,
    key: Option<&ProjectKey>,
) -> Result<Connection, StorageError> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    ProjectKey::unlock(key, &conn)?;
    conn.pragma_update(None, "query_only", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

pub(crate) fn configure(conn: &Connection) -> Result<(), StorageError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// The marks on site map nodes.
fn site_marks(c: &rusqlite::Connection) -> Result<Vec<SiteMarkEntry>, StorageError> {
    let mut stmt =
        c.prepare("SELECT scheme, host, port, path, mark FROM site_marks ORDER BY host, path")?;
    let marks = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u16>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .filter_map(|row| match row {
            Ok((scheme, host, port, path, mark)) => SiteMark::parse(&mark).map(|mark| {
                Ok(SiteMarkEntry {
                    scheme,
                    host,
                    port,
                    path,
                    mark,
                })
            }),
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(marks)
}

impl ProjectDb {
    /// Opens (creating if needed) and migrates a project file.
    pub fn open(path: &Path, opts: OpenOptions) -> Result<ProjectDb, StorageError> {
        if opts.read_only {
            return ProjectDb::open_read_only(path, opts.key);
        }
        let existed = path.exists();
        let mut conn = Connection::open(path)?;
        if !existed {
            chosproxy_core::fs::restrict_file(path)?;
        }
        ProjectKey::unlock(opts.key.as_ref(), &conn)?;
        configure(&conn)?;
        migrations::migrate(&mut conn)?;
        conn.execute(
            "INSERT OR IGNORE INTO project_meta (key, value) VALUES ('created_at', ?1)",
            [chosproxy_core::time::now_ms().to_string()],
        )?;

        let (tx, rx) = mpsc::sync_channel(opts.queue_capacity.unwrap_or(2048));
        let hook = opts.on_insert.clone();
        let writer = std::thread::Builder::new()
            .name("project-writer".into())
            .spawn(move || writer_loop(conn, rx, hook))?;
        Ok(ProjectDb {
            inner: Arc::new(Inner {
                path: path.to_path_buf(),
                sender: Some(tx),
                writer: Some(writer),
                readers: Mutex::new(Vec::new()),
                scope: Arc::default(),
                read_only: false,
                key: RwLock::new(opts.key),
            }),
        })
    }

    fn open_read_only(path: &Path, key: Option<ProjectKey>) -> Result<ProjectDb, StorageError> {
        if !path.is_file() {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} does not exist", path.display()),
            )));
        }
        let conn = read_only_connection(path, key.as_ref())?;
        let scope: SharedScope = Arc::default();
        register_functions(&conn, scope.clone())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let app_id: i64 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if app_id != migrations::APPLICATION_ID {
            return Err(StorageError::NotAProject);
        }
        if version > migrations::latest() {
            return Err(StorageError::NewerSchema {
                found: version,
                supported: migrations::latest(),
            });
        }
        if version < migrations::latest() {
            return Err(StorageError::InvalidQuery(
                "this project uses an older format: open it normally once to upgrade it \
                 before opening it read-only"
                    .into(),
            ));
        }
        Ok(ProjectDb {
            inner: Arc::new(Inner {
                path: path.to_path_buf(),
                sender: None,
                writer: None,
                readers: Mutex::new(vec![conn]),
                scope,
                read_only: true,
                key: RwLock::new(key),
            }),
        })
    }

    /// Whether the project was opened read-only.
    pub fn is_read_only(&self) -> bool {
        self.inner.read_only
    }

    /// SQLite's quick integrity check of a project file, without changing
    /// it: the problems found (empty when the file is sound).
    pub fn quick_check(path: &Path, key: Option<&ProjectKey>) -> Result<Vec<String>, StorageError> {
        let conn = read_only_connection(path, key)?;
        let mut stmt = conn.prepare("PRAGMA quick_check(20)")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().filter(|r| r != "ok").collect())
    }

    /// Whether the project is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.current_key().is_some()
    }

    /// The key of an encrypted project (kept in memory only).
    pub fn current_key(&self) -> Option<ProjectKey> {
        match self.inner.key.read() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Changes the password of an encrypted project in place. A project
    /// that is not encrypted is encrypted by saving an encrypted copy
    /// instead (SQLCipher cannot encrypt a plain file in place).
    pub fn rekey(&self, new: ProjectKey) -> Result<(), StorageError> {
        if self.current_key().is_none() {
            return Err(StorageError::InvalidQuery(
                "the project is not encrypted: save an encrypted copy instead".into(),
            ));
        }
        self.flush()?;
        let k = new.clone();
        self.write_unit(|reply| WriteCmd::Rekey(k, reply))?;
        match self.inner.key.write() {
            Ok(mut g) => *g = Some(new),
            Err(poisoned) => *poisoned.into_inner() = Some(new),
        }
        // Pooled readers still hold the old key.
        if let Ok(mut pool) = self.inner.readers.lock() {
            pool.clear();
        }
        Ok(())
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// The scope used by `chos_in_scope()` (shared, follows changes).
    pub(crate) fn shared_scope(&self) -> SharedScope {
        self.inner.scope.clone()
    }

    /// Sets the scope used for `in_scope` columns and scope filters.
    pub fn set_scope(&self, scope: Scope) {
        let scope = Arc::new(scope);
        match self.inner.scope.write() {
            Ok(mut g) => *g = scope,
            Err(poisoned) => *poisoned.into_inner() = scope,
        }
    }

    fn write_unit(&self, make: impl FnOnce(UnitReply) -> WriteCmd) -> Result<(), StorageError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.sender()?
            .send(make(reply_tx))
            .map_err(|_| StorageError::Closed)?;
        reply_rx.recv().map_err(|_| StorageError::Closed)?
    }

    /// Replaces the note and highlight of an exchange (blocking). Empty notes
    /// are stored as none.
    pub fn annotate(
        &self,
        id: ExchangeId,
        notes: Option<String>,
        highlight: Option<Highlight>,
    ) -> Result<(), StorageError> {
        let notes = notes
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        if notes
            .as_ref()
            .is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS)
        {
            return Err(StorageError::InvalidQuery(format!(
                "notes are limited to {MAX_NOTE_CHARS} characters"
            )));
        }
        self.write_unit(|reply| WriteCmd::Annotate {
            id,
            notes,
            highlight,
            reply,
        })
    }

    /// Sets (`mark = Some`) or clears a site map mark (blocking).
    pub fn set_site_mark(
        &self,
        scheme: String,
        host: String,
        port: u16,
        path: String,
        mark: Option<SiteMark>,
    ) -> Result<(), StorageError> {
        if !path.starts_with('/') || path.len() > 8192 {
            return Err(StorageError::InvalidQuery("invalid site map path".into()));
        }
        let clear = mark.is_none();
        let node = SiteMarkEntry {
            scheme,
            host,
            port,
            path,
            mark: mark.unwrap_or(SiteMark::Reviewed),
        };
        self.write_unit(|reply| WriteCmd::Mark { node, clear, reply })
    }

    fn sender(&self) -> Result<&SyncSender<WriteCmd>, StorageError> {
        self.inner.sender.as_ref().ok_or(if self.inner.read_only {
            StorageError::ReadOnly
        } else {
            StorageError::Closed
        })
    }

    /// Queues an exchange without waiting. Fails fast with
    /// [`StorageError::QueueFull`] instead of blocking the caller.
    pub fn submit(&self, ex: NewExchange) -> Result<(), StorageError> {
        match self
            .sender()?
            .try_send(WriteCmd::Insert(Box::new(ex), None))
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(StorageError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(StorageError::Closed),
        }
    }

    /// Queues a WebSocket event without waiting (like [`Self::submit`]).
    pub fn submit_ws(&self, event: WsEvent) -> Result<(), StorageError> {
        match self.sender()?.try_send(WriteCmd::Ws(Box::new(event))) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(StorageError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(StorageError::Closed),
        }
    }

    /// Stores an exchange and waits for its id (blocking).
    pub fn insert(&self, ex: NewExchange) -> Result<ExchangeId, StorageError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.sender()?
            .send(WriteCmd::Insert(Box::new(ex), Some(reply_tx)))
            .map_err(|_| StorageError::Closed)?;
        reply_rx.recv().map_err(|_| StorageError::Closed)?
    }

    /// Waits until everything queued so far is committed (blocking).
    pub fn flush(&self) -> Result<(), StorageError> {
        if self.inner.read_only {
            return Ok(());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender()?
            .send(WriteCmd::Flush(tx))
            .map_err(|_| StorageError::Closed)?;
        rx.recv().map_err(|_| StorageError::Closed)
    }

    /// Queues `f` to run on the writer thread inside its transaction
    /// without waiting (like [`Self::submit`]); its result is ignored.
    pub(crate) fn write_detached(
        &self,
        f: impl FnOnce(&Connection) -> Result<(), StorageError> + Send + 'static,
    ) -> Result<(), StorageError> {
        let job: WriteJob = Box::new(move |c| {
            let ok = f(c).is_ok();
            let finish: Finisher = Box::new(|_| {});
            (ok, finish)
        });
        match self.sender()?.try_send(WriteCmd::Run(job)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(StorageError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(StorageError::Closed),
        }
    }

    /// Runs `f` on the writer thread inside its transaction and waits.
    pub(crate) fn write<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StorageError> + Send + 'static,
    ) -> Result<T, StorageError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.sender()?
            .send(WriteCmd::Run(Box::new(move |c| {
                let result = f(c);
                let ok = result.is_ok();
                let finish: Finisher = Box::new(move |committed| {
                    let _ = reply_tx.send(if committed {
                        result
                    } else {
                        Err(StorageError::Closed)
                    });
                });
                (ok, finish)
            })))
            .map_err(|_| StorageError::Closed)?;
        reply_rx.recv().map_err(|_| StorageError::Closed)?
    }

    pub(crate) fn with_reader<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let pooled = self
            .inner
            .readers
            .lock()
            .map_err(|_| StorageError::Closed)?
            .pop();
        let conn = match pooled {
            Some(c) => c,
            None if self.inner.read_only => {
                let c = read_only_connection(&self.inner.path, self.current_key().as_ref())?;
                register_functions(&c, self.inner.scope.clone())?;
                c
            }
            None => {
                let c = Connection::open(&self.inner.path)?;
                ProjectKey::unlock(self.current_key().as_ref(), &c)?;
                configure(&c)?;
                c.pragma_update(None, "query_only", "ON")?;
                register_functions(&c, self.inner.scope.clone())?;
                c
            }
        };
        let result = f(&conn);
        if let Ok(mut pool) = self.inner.readers.lock()
            && pool.len() < MAX_IDLE_READERS
        {
            pool.push(conn);
        }
        result
    }

    /// One page of history matching `q`.
    pub fn history_page(&self, q: &HistoryQuery) -> Result<HistoryPage, StorageError> {
        if q.limit == 0 || q.limit > HistoryQuery::MAX_LIMIT {
            return Err(StorageError::InvalidQuery(format!(
                "limit must be 1..={}",
                HistoryQuery::MAX_LIMIT
            )));
        }
        let (where_sql, args) = build_filter(q)?;
        self.with_reader(|c| {
            let total: i64 = c.query_row(
                &format!("SELECT COUNT(*) FROM exchanges {where_sql}"),
                params_from_iter(args.iter()),
                |r| r.get(0),
            )?;
            let order = match q.order {
                HistoryOrder::Asc => "ASC",
                HistoryOrder::Desc => "DESC",
            };
            let sql = format!(
                "SELECT {ROW_COLUMNS} FROM exchanges {where_sql} ORDER BY {}{order} LIMIT {} OFFSET {}",
                match q.sort {
                    HistorySort::Id => "id ".to_string(),
                    s => format!("{} {order}, id ", s.column()),
                },
                q.limit,
                q.offset
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map(params_from_iter(args.iter()), map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(HistoryPage {
                total: u64::try_from(total).unwrap_or(0),
                rows,
            })
        })
    }

    /// A full exchange with raw request and response.
    pub fn exchange(&self, id: ExchangeId) -> Result<Option<StoredExchange>, StorageError> {
        self.with_reader(|c| load_exchange(c, id))
    }

    /// Several exchanges in one read transaction (much cheaper than one call
    /// per id). Missing ids are skipped; order follows `ids`.
    pub fn exchanges_bulk(&self, ids: &[ExchangeId]) -> Result<Vec<StoredExchange>, StorageError> {
        self.with_reader(|c| {
            let tx = c.unchecked_transaction()?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(ex) = load_exchange(&tx, *id)? {
                    out.push(ex);
                }
            }
            tx.finish()?;
            Ok(out)
        })
    }

    /// Ids of exchanges matching the filter part of `q`, newest first, at
    /// most `max`.
    pub fn matching_ids(
        &self,
        q: &HistoryQuery,
        max: u32,
    ) -> Result<Vec<ExchangeId>, StorageError> {
        let (where_sql, args) = build_filter(q)?;
        self.with_reader(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT id FROM exchanges {where_sql} ORDER BY id DESC LIMIT {max}"
            ))?;
            let ids = stmt
                .query_map(params_from_iter(args.iter()), |r| r.get(0))?
                .collect::<Result<Vec<ExchangeId>, _>>()?;
            Ok(ids)
        })
    }

    /// Endpoints matching the filter part of `q` (paging is ignored), with
    /// all stored marks.
    /// The marks on site map nodes (without building the map).
    pub fn site_marks(&self) -> Result<Vec<SiteMarkEntry>, StorageError> {
        self.with_reader(site_marks)
    }

    pub fn site_map(&self, q: &HistoryQuery) -> Result<SiteMap, StorageError> {
        let (where_sql, args) = build_filter(q)?;
        self.with_reader(|c| {
            let sql = format!(
                "SELECT scheme, host, port, p, COUNT(*), GROUP_CONCAT(DISTINCT method), \
                 GROUP_CONCAT(DISTINCT status), GROUP_CONCAT(DISTINCT mime), \
                 GROUP_CONCAT(DISTINCT source), MAX(q), MIN(started_at), MAX(started_at), MAX(id), \
                 chos_in_scope(scheme, host, port, p) \
                 FROM (SELECT *, {PATH_NO_QUERY} AS p, instr(path, '?') > 0 AS q FROM exchanges {where_sql}) \
                 GROUP BY scheme, host, port, p ORDER BY host, port, scheme, p LIMIT {}",
                SiteMap::MAX_ENTRIES + 1
            );
            let split = |v: Option<String>| -> Vec<String> {
                v.map(|s| s.split(',').map(str::to_string).collect())
                    .unwrap_or_default()
            };
            let mut stmt = c.prepare(&sql)?;
            let mut entries = stmt
                .query_map(params_from_iter(args.iter()), |r| {
                    let mut statuses: Vec<u16> = split(r.get(6)?)
                        .iter()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    statuses.sort_unstable();
                    let mut methods = split(r.get(5)?);
                    methods.sort();
                    let mut mimes = split(r.get(7)?);
                    mimes.sort();
                    let mut sources: Vec<Source> = split(r.get(8)?)
                        .iter()
                        .filter_map(|s| Source::parse(s))
                        .collect();
                    sources.sort_by_key(|s| s.as_str());
                    Ok(SiteMapEntry {
                        scheme: r.get(0)?,
                        host: r.get(1)?,
                        port: r.get(2)?,
                        path: r.get(3)?,
                        count: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
                        methods,
                        statuses,
                        mimes,
                        sources,
                        has_params: r.get(9)?,
                        first_seen: r.get(10)?,
                        last_seen: r.get(11)?,
                        last_id: r.get(12)?,
                        in_scope: r.get(13)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let truncated = entries.len() > SiteMap::MAX_ENTRIES as usize;
            entries.truncate(SiteMap::MAX_ENTRIES as usize);
            let marks = site_marks(c)?;
            Ok(SiteMap {
                entries,
                truncated,
                marks,
            })
        })
    }

    /// Aggregate counters.
    pub fn stats(&self) -> Result<ProjectStats, StorageError> {
        self.with_reader(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*), COUNT(DISTINCT host), \
                 COUNT(DISTINCT host || ':' || port || ' ' || \
                   CASE WHEN instr(path, '?') > 0 THEN substr(path, 1, instr(path, '?') - 1) ELSE path END), \
                 COALESCE(SUM(error IS NOT NULL), 0) FROM exchanges",
                [],
                |r| {
                    let n = |i: usize| r.get::<_, i64>(i).map(|v| u64::try_from(v).unwrap_or(0));
                    Ok(ProjectStats {
                        exchanges: n(0)?,
                        hosts: n(1)?,
                        endpoints: n(2)?,
                        errors: n(3)?,
                    })
                },
            )?)
        })
    }
}

/// Loads one exchange with its messages.
fn load_exchange(c: &Connection, id: ExchangeId) -> Result<Option<StoredExchange>, StorageError> {
    let found = c
        .prepare_cached(&format!(
            "SELECT {ROW_COLUMNS}, client_addr, listener, reason, request_blob, response_blob, \
             original_request_blob, original_response_blob FROM exchanges WHERE id = ?1"
        ))?
        .query_row([id], |r| {
            Ok((
                map_row(r)?,
                r.get::<_, Option<String>>(ROW_COLUMN_COUNT)?,
                r.get::<_, Option<String>>(ROW_COLUMN_COUNT + 1)?,
                r.get::<_, Option<String>>(ROW_COLUMN_COUNT + 2)?,
                r.get::<_, i64>(ROW_COLUMN_COUNT + 3)?,
                r.get::<_, Option<i64>>(ROW_COLUMN_COUNT + 4)?,
                r.get::<_, Option<i64>>(ROW_COLUMN_COUNT + 5)?,
                r.get::<_, Option<i64>>(ROW_COLUMN_COUNT + 6)?,
            ))
        })
        .optional()?;
    let Some((row, client_addr, listener, reason, req_blob, res_blob, orig_req, orig_res)) = found
    else {
        return Ok(None);
    };
    let load_opt = |id: Option<i64>| -> Result<Option<Vec<u8>>, StorageError> {
        match id {
            Some(b) => Ok(Some(blob::load(c, b)?.ok_or(StorageError::CorruptBlob(b))?)),
            None => Ok(None),
        }
    };
    let request = blob::load(c, req_blob)?.ok_or(StorageError::CorruptBlob(req_blob))?;
    Ok(Some(StoredExchange {
        row,
        client_addr,
        listener,
        reason,
        request,
        response: load_opt(res_blob)?,
        original_request: load_opt(orig_req)?,
        original_response: load_opt(orig_res)?,
    }))
}

const ROW_COLUMNS: &str = "id, started_at, duration_ms, source, scheme, method, host, port, path, \
    extension, url, http_version, status, mime, request_len, response_len, server_ip, tls, error, \
    notes, flags, highlight, chos_in_scope(scheme, host, port, path)";
const ROW_COLUMN_COUNT: usize = 23;
/// SQL expression: the `path` column without its query string.
const PATH_NO_QUERY: &str =
    "CASE WHEN instr(path, '?') > 0 THEN substr(path, 1, instr(path, '?') - 1) ELSE path END";

fn map_row(r: &Row<'_>) -> rusqlite::Result<HistoryRow> {
    let source: String = r.get(3)?;
    let flags_v: i64 = r.get(20)?;
    let to_u64 = |v: i64| u64::try_from(v).unwrap_or(0);
    Ok(HistoryRow {
        id: r.get(0)?,
        started_at: r.get(1)?,
        duration_ms: r.get(2)?,
        source: Source::parse(&source).unwrap_or(Source::Proxy),
        scheme: r.get(4)?,
        method: r.get(5)?,
        host: r.get(6)?,
        port: r.get(7)?,
        path: r.get(8)?,
        extension: r.get(9)?,
        url: r.get(10)?,
        http_version: r.get(11)?,
        status: r.get(12)?,
        mime: r.get(13)?,
        request_len: to_u64(r.get(14)?),
        response_len: r.get::<_, Option<i64>>(15)?.map(to_u64),
        server_ip: r.get(16)?,
        tls: r.get(17)?,
        error: r.get(18)?,
        notes: r.get(19)?,
        request_truncated: flags_v & flags::REQUEST_TRUNCATED != 0,
        response_truncated: flags_v & flags::RESPONSE_TRUNCATED != 0,
        cl_te_conflict: flags_v & flags::CL_TE_CONFLICT != 0,
        upgraded: flags_v & flags::UPGRADED != 0,
        edited: flags_v & (flags::EDITED_REQUEST | flags::EDITED_RESPONSE) != 0,
        dropped: flags_v & flags::DROPPED != 0,
        highlight: r
            .get::<_, Option<String>>(21)?
            .and_then(|h| Highlight::parse(&h)),
        in_scope: r.get(22)?,
    })
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

fn text_list(values: impl Iterator<Item = String>) -> Vec<Value> {
    values.map(Value::Text).collect()
}

fn build_filter(q: &HistoryQuery) -> Result<(String, Vec<Value>), StorageError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();
    if let Some(s) = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if q.search_regex {
            search_regex(s)?; // report a bad expression instead of matching nothing
            clauses.push("regexp(?, url)".into());
            args.push(Value::Text(s.to_string()));
        } else {
            clauses.push("url LIKE ? ESCAPE '\\'".into());
            args.push(Value::Text(format!("%{}%", escape_like(s))));
        }
    }
    if q.in_scope_only {
        clauses.push("chos_in_scope(scheme, host, port, path)".into());
    }
    if !q.mimes.is_empty() {
        clauses.push(format!("mime IN ({})", placeholders(q.mimes.len())));
        args.extend(text_list(q.mimes.iter().cloned()));
    }
    if !q.sources.is_empty() {
        clauses.push(format!("source IN ({})", placeholders(q.sources.len())));
        args.extend(text_list(q.sources.iter().map(|s| s.as_str().to_string())));
    }
    if !q.highlights.is_empty() {
        clauses.push(format!(
            "highlight IN ({})",
            placeholders(q.highlights.len())
        ));
        args.extend(text_list(
            q.highlights.iter().map(|h| h.as_str().to_string()),
        ));
    }
    if q.annotated_only {
        clauses.push("(highlight IS NOT NULL OR (notes IS NOT NULL AND notes <> ''))".into());
    }
    if let Some(site) = &q.site {
        clauses.push("scheme = ? AND host = ? AND port = ?".into());
        args.push(Value::Text(site.scheme.clone()));
        args.push(Value::Text(site.host.clone()));
        args.push(Value::Integer(i64::from(site.port)));
        if site.path.ends_with('/') {
            if site.path != "/" {
                clauses.push("substr(path, 1, length(?)) = ?".into());
                args.push(Value::Text(site.path.clone()));
                args.push(Value::Text(site.path.clone()));
            }
        } else {
            clauses.push(format!("{PATH_NO_QUERY} = ?"));
            args.push(Value::Text(site.path.clone()));
        }
    }
    if !q.methods.is_empty() {
        clauses.push(format!("method IN ({})", placeholders(q.methods.len())));
        args.extend(
            q.methods
                .iter()
                .map(|m| Value::Text(m.to_ascii_uppercase())),
        );
    }
    if !q.status_classes.is_empty() {
        let parts: Vec<String> = q
            .status_classes
            .iter()
            .filter(|c| **c <= 5)
            .map(|c| match c {
                0 => "status IS NULL".to_string(),
                c => format!(
                    "(status >= {} AND status < {})",
                    u32::from(*c) * 100,
                    (u32::from(*c) + 1) * 100
                ),
            })
            .collect();
        if !parts.is_empty() {
            clauses.push(format!("({})", parts.join(" OR ")));
        }
    }
    if !q.hide_extensions.is_empty() {
        clauses.push(format!(
            "(extension IS NULL OR extension NOT IN ({}))",
            placeholders(q.hide_extensions.len())
        ));
        args.extend(
            q.hide_extensions
                .iter()
                .map(|e| Value::Text(e.to_ascii_lowercase())),
        );
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    Ok((sql, args))
}

fn rekey(conn: &Connection, key: &ProjectKey) -> Result<(), StorageError> {
    conn.pragma_update(None, "rekey", key.secret())?;
    ProjectKey::unlock(None, conn)
}

fn writer_loop(mut conn: Connection, rx: Receiver<WriteCmd>, hook: Option<BatchHook>) {
    let mut next: Option<WriteCmd> = None;
    loop {
        let first = match next.take() {
            Some(cmd) => cmd,
            None => match rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => break,
            },
        };
        // A key change runs on its own, between transactions.
        if let WriteCmd::Rekey(key, reply) = first {
            let _ = reply.send(rekey(&conn, &key));
            continue;
        }
        let mut batch = vec![first];
        while batch.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(cmd @ WriteCmd::Rekey(..)) => {
                    next = Some(cmd);
                    break;
                }
                Ok(cmd) => batch.push(cmd),
                Err(_) => break,
            }
        }
        let mut replies: Vec<(Reply, Result<ExchangeId, StorageError>)> = Vec::new();
        let mut unit_replies: Vec<(UnitReply, Result<(), StorageError>)> = Vec::new();
        let mut finishers: Vec<(Finisher, bool)> = Vec::new();
        let mut flushes = Vec::new();
        let mut committed = InsertBatch {
            last_id: 0,
            count: 0,
            ws_events: 0,
        };
        let mut tx = match conn.transaction() {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!(error = %e, "cannot start write transaction; dropping batch");
                continue;
            }
        };
        for cmd in batch {
            match cmd {
                WriteCmd::Insert(ex, reply) => {
                    let result = insert_exchange(&tx, &ex);
                    match &result {
                        Ok(id) => {
                            committed.last_id = committed.last_id.max(*id);
                            committed.count += 1;
                        }
                        Err(e) => tracing::error!(error = %e, "failed to store exchange"),
                    }
                    if let Some(r) = reply {
                        replies.push((r, result));
                    }
                }
                WriteCmd::Annotate {
                    id,
                    notes,
                    highlight,
                    reply,
                } => {
                    let result = tx
                        .execute(
                            "UPDATE exchanges SET notes = ?2, highlight = ?3 WHERE id = ?1",
                            params![id, notes, highlight.map(Highlight::as_str)],
                        )
                        .map_err(StorageError::from)
                        .and_then(|n| {
                            if n == 0 {
                                Err(StorageError::NotFound(id))
                            } else {
                                Ok(())
                            }
                        });
                    unit_replies.push((reply, result));
                }
                WriteCmd::Mark { node, clear, reply } => {
                    let result = if clear {
                        tx.execute(
                            "DELETE FROM site_marks WHERE scheme = ?1 AND host = ?2 AND port = ?3 \
                             AND path = ?4",
                            params![node.scheme, node.host, node.port, node.path],
                        )
                    } else {
                        tx.execute(
                            "INSERT INTO site_marks (scheme, host, port, path, mark) \
                             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT DO UPDATE SET mark = ?5",
                            params![
                                node.scheme,
                                node.host,
                                node.port,
                                node.path,
                                node.mark.as_str()
                            ],
                        )
                    };
                    unit_replies.push((reply, result.map(|_| ()).map_err(StorageError::from)));
                }
                WriteCmd::Ws(event) => match crate::websocket::apply_event(&tx, &event) {
                    Ok(()) => committed.ws_events += 1,
                    Err(e) => tracing::error!(error = %e, "failed to store WebSocket event"),
                },
                WriteCmd::Run(f) => match tx.savepoint() {
                    Ok(sp) => {
                        let (ok, finish) = f(&sp);
                        // A failed write leaves no partial changes behind.
                        let kept = if ok { sp.commit().is_ok() } else { true };
                        finishers.push((finish, kept));
                    }
                    Err(e) => tracing::error!(error = %e, "cannot start a savepoint"),
                },
                WriteCmd::Flush(done) => flushes.push(done),
                WriteCmd::Rekey(_, reply) => {
                    // Never batched (see above).
                    unit_replies.push((reply, Err(StorageError::Closed)));
                }
            }
        }
        if let Err(e) = tx.commit() {
            tracing::error!(error = %e, "write transaction failed");
            for (r, _) in replies {
                let _ = r.send(Err(StorageError::Closed));
            }
            for (r, _) in unit_replies {
                let _ = r.send(Err(StorageError::Closed));
            }
            for (finish, _) in finishers {
                finish(false);
            }
            continue;
        }
        if (committed.count > 0 || committed.ws_events > 0)
            && let Some(h) = &hook
        {
            h(committed);
        }
        for (r, result) in replies {
            let _ = r.send(result);
        }
        for (r, result) in unit_replies {
            let _ = r.send(result);
        }
        for (finish, kept) in finishers {
            finish(kept);
        }
        for f in flushes {
            let _ = f.send(());
        }
    }
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)") {
        tracing::warn!(error = %e, "WAL checkpoint on close failed");
    }
}

fn insert_exchange(c: &Connection, ex: &NewExchange) -> Result<ExchangeId, StorageError> {
    let req_blob = blob::insert(c, &ex.request)?;
    let insert_opt = |data: &Option<Vec<u8>>| -> Result<Option<i64>, StorageError> {
        data.as_deref().map(|d| blob::insert(c, d)).transpose()
    };
    let res_blob = insert_opt(&ex.response)?;
    let orig_req_blob = insert_opt(&ex.original_request)?;
    let orig_res_blob = insert_opt(&ex.original_response)?;
    let to_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
    c.prepare_cached(
        "INSERT INTO exchanges (started_at, duration_ms, source, listener, client_addr, scheme, \
         method, host, port, path, extension, url, http_version, status, reason, mime, request_len, \
         response_len, server_ip, tls, error, notes, flags, request_blob, response_blob, \
         original_request_blob, original_response_blob, ws_key) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, \
         ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)",
    )?
    .execute(params![
        ex.started_at,
        ex.duration_ms,
        ex.source.as_str(),
        ex.listener,
        ex.client_addr,
        ex.scheme,
        ex.method,
        ex.host,
        ex.port,
        ex.path,
        ex.extension,
        ex.url,
        ex.http_version,
        ex.status,
        ex.reason,
        ex.mime,
        to_i64(ex.request_len),
        ex.response_len.map(to_i64),
        ex.server_ip,
        ex.tls,
        ex.error,
        ex.notes,
        ex.flags,
        req_blob,
        res_blob,
        orig_req_blob,
        orig_res_blob,
        ex.ws_key,
    ])?;
    Ok(c.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn sample(i: u32) -> NewExchange {
        NewExchange {
            started_at: 1_700_000_000_000 + i64::from(i),
            duration_ms: Some(12),
            source: Source::Proxy,
            listener: Some("default".into()),
            client_addr: Some("127.0.0.1:5000".into()),
            scheme: "http".into(),
            method: if i.is_multiple_of(2) {
                "GET".into()
            } else {
                "POST".into()
            },
            host: format!("h{}.test", i % 3),
            port: 80,
            path: if i.is_multiple_of(5) {
                format!("/static/a{i}.png")
            } else {
                format!("/api/{i}?q=1")
            },
            extension: i.is_multiple_of(5).then(|| "png".to_string()),
            url: format!("http://h{}.test/api/{i}?q=1", i % 3),
            http_version: "HTTP/1.1".into(),
            status: if i.is_multiple_of(7) {
                None
            } else {
                Some(if i.is_multiple_of(4) { 404 } else { 200 })
            },
            reason: Some("OK".into()),
            mime: Some("JSON".into()),
            request: format!("GET /api/{i} HTTP/1.1\r\nHost: h\r\n\r\n").into_bytes(),
            response: Some(vec![b'x'; 5000]),
            original_request: None,
            original_response: None,
            request_len: 30,
            response_len: Some(5000),
            server_ip: Some("127.0.0.1".into()),
            tls: false,
            error: if i.is_multiple_of(7) {
                Some("connect failed".into())
            } else {
                None
            },
            notes: None,
            flags: if i == 3 { flags::RESPONSE_TRUNCATED } else { 0 },
            ws_key: None,
        }
    }

    fn page(db: &ProjectDb, q: HistoryQuery) -> HistoryPage {
        db.history_page(&HistoryQuery { limit: 100, ..q }).unwrap()
    }

    #[test]
    fn insert_query_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let batches = Arc::new(AtomicU32::new(0));
        let b2 = batches.clone();
        let db = ProjectDb::open(
            &dir.path().join("p.chosproxy"),
            OpenOptions {
                on_insert: Some(Arc::new(move |b| {
                    b2.fetch_add(b.count, Ordering::SeqCst);
                })),
                ..Default::default()
            },
        )
        .unwrap();
        let id = db.insert(sample(0)).unwrap();
        for i in 1..30 {
            db.submit(sample(i)).unwrap();
        }
        db.flush().unwrap();
        assert_eq!(batches.load(Ordering::SeqCst), 30);

        let all = page(&db, HistoryQuery::default());
        assert_eq!(all.total, 30);
        assert_eq!(all.rows[0].id, 30, "newest first by default");

        let got = db.exchange(id).unwrap().unwrap();
        assert_eq!(got.request, sample(0).request);
        assert_eq!(got.response, sample(0).response);
        assert_eq!(got.row.extension.as_deref(), Some("png"));
        assert!(db.exchange(999).unwrap().is_none());

        let posts = page(
            &db,
            HistoryQuery {
                methods: vec!["post".into()],
                ..Default::default()
            },
        );
        assert_eq!(posts.total, 15);
        let missing = page(
            &db,
            HistoryQuery {
                status_classes: vec![0],
                ..Default::default()
            },
        );
        assert!(missing.rows.iter().all(|r| r.status.is_none()));
        let not_found = page(
            &db,
            HistoryQuery {
                status_classes: vec![4],
                ..Default::default()
            },
        );
        assert!(not_found.rows.iter().all(|r| r.status == Some(404)));
        let no_png = page(
            &db,
            HistoryQuery {
                hide_extensions: vec!["PNG".into()],
                ..Default::default()
            },
        );
        assert_eq!(no_png.total, 24);
        let search = page(
            &db,
            HistoryQuery {
                search: Some("/api/2?".into()),
                ..Default::default()
            },
        );
        assert_eq!(search.total, 1);
        let like_meta = page(
            &db,
            HistoryQuery {
                search: Some("%".into()),
                ..Default::default()
            },
        );
        assert_eq!(like_meta.total, 0, "LIKE wildcards must be escaped");
        let paged = db
            .history_page(&HistoryQuery {
                limit: 10,
                offset: 25,
                order: HistoryOrder::Asc,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(paged.rows.len(), 5);
        assert_eq!(paged.rows[0].id, 26);
        // Sorted by a column, ties in capture order.
        for (sort, order) in [
            (HistorySort::Host, HistoryOrder::Asc),
            (HistorySort::Status, HistoryOrder::Desc),
            (HistorySort::Length, HistoryOrder::Asc),
            (HistorySort::Method, HistoryOrder::Desc),
        ] {
            let rows = db
                .history_page(&HistoryQuery {
                    limit: 100,
                    sort,
                    order,
                    ..Default::default()
                })
                .unwrap()
                .rows;
            assert_eq!(rows.len(), 30);
            let key = |r: &HistoryRow| match sort {
                HistorySort::Host => (r.host.clone(), 0),
                HistorySort::Method => (r.method.clone(), 0),
                HistorySort::Status => (String::new(), i64::from(r.status.unwrap_or(0))),
                _ => (String::new(), r.response_len.map_or(-1, |l| l as i64)),
            };
            for w in rows.windows(2) {
                let (a, b) = (key(&w[0]), key(&w[1]));
                match order {
                    HistoryOrder::Asc => assert!(a <= b, "{sort:?}"),
                    HistoryOrder::Desc => assert!(a >= b, "{sort:?}"),
                }
                if a == b {
                    match order {
                        HistoryOrder::Asc => assert!(w[0].id < w[1].id),
                        HistoryOrder::Desc => assert!(w[0].id > w[1].id),
                    }
                }
            }
        }
        assert!(
            page(&db, HistoryQuery::default())
                .rows
                .iter()
                .any(|r| r.response_truncated)
        );

        let stats = db.stats().unwrap();
        assert_eq!(stats.exchanges, 30);
        assert_eq!(stats.hosts, 3);
        assert_eq!(stats.errors, 5);
    }

    #[test]
    fn stores_originals_of_edited_messages() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        let mut ex = sample(1);
        ex.original_request = Some(b"GET /before HTTP/1.1\r\n\r\n".to_vec());
        ex.flags = flags::EDITED_REQUEST;
        let id = db.insert(ex).unwrap();
        let got = db.exchange(id).unwrap().unwrap();
        assert!(got.row.edited && !got.row.dropped);
        assert_eq!(
            got.original_request.as_deref(),
            Some(&b"GET /before HTTP/1.1\r\n\r\n"[..])
        );
        assert!(got.original_response.is_none());
    }

    fn open_with(n: u32) -> (tempfile::TempDir, ProjectDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        for i in 0..n {
            db.submit(sample(i)).unwrap();
        }
        db.flush().unwrap();
        (dir, db)
    }

    fn scope_of(host: &str) -> Scope {
        Scope::compile(&chosproxy_core::ScopeConfig {
            include: vec![chosproxy_core::scope::ScopeRule {
                host: host.into(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn scope_column_and_filter_follow_the_current_scope() {
        let (_dir, db) = open_with(9);
        let all = page(&db, HistoryQuery::default());
        assert!(all.rows.iter().all(|r| !r.in_scope), "empty scope");
        let only = |db: &ProjectDb| {
            page(
                db,
                HistoryQuery {
                    in_scope_only: true,
                    ..Default::default()
                },
            )
        };
        assert_eq!(only(&db).total, 0);
        db.set_scope(scope_of("h1.test"));
        let in_scope = only(&db);
        assert_eq!(in_scope.total, 3);
        assert!(
            in_scope
                .rows
                .iter()
                .all(|r| r.host == "h1.test" && r.in_scope)
        );
        db.set_scope(Scope::default());
        assert_eq!(only(&db).total, 0, "scope changes apply to pooled readers");
    }

    #[test]
    fn regex_search_and_other_filters() {
        let (_dir, db) = open_with(12);
        let q = |search: &str| HistoryQuery {
            search: Some(search.into()),
            search_regex: true,
            ..Default::default()
        };
        assert_eq!(page(&db, q(r"/api/1\d\?")).total, 2, "10 and 11");
        assert_eq!(page(&db, q(r"(?-i)/API/")).total, 0);
        assert!(matches!(
            db.history_page(&HistoryQuery {
                limit: 10,
                ..q("(")
            }),
            Err(StorageError::InvalidQuery(_))
        ));
        let by_source = page(
            &db,
            HistoryQuery {
                sources: vec![Source::Repeater],
                ..Default::default()
            },
        );
        assert_eq!(by_source.total, 0);
        let by_mime = page(
            &db,
            HistoryQuery {
                mimes: vec!["JSON".into()],
                ..Default::default()
            },
        );
        assert_eq!(by_mime.total, 12);
    }

    #[test]
    fn annotations_and_highlight_filters() {
        let (_dir, db) = open_with(4);
        db.annotate(2, Some("  login form ".into()), Some(Highlight::Red))
            .unwrap();
        db.annotate(3, Some("   ".into()), Some(Highlight::Green))
            .unwrap();
        assert!(matches!(
            db.annotate(99, None, None),
            Err(StorageError::NotFound(99))
        ));
        assert!(
            db.annotate(1, Some("x".repeat(MAX_NOTE_CHARS + 1)), None)
                .is_err()
        );
        let row2 = db.exchange(2).unwrap().unwrap().row;
        assert_eq!(row2.notes.as_deref(), Some("login form"));
        assert_eq!(row2.highlight, Some(Highlight::Red));
        assert_eq!(db.exchange(3).unwrap().unwrap().row.notes, None);
        let annotated = page(
            &db,
            HistoryQuery {
                annotated_only: true,
                ..Default::default()
            },
        );
        assert_eq!(annotated.total, 2);
        let red = page(
            &db,
            HistoryQuery {
                highlights: vec![Highlight::Red],
                ..Default::default()
            },
        );
        assert_eq!(red.rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2]);
        db.annotate(2, None, None).unwrap();
        assert_eq!(
            page(
                &db,
                HistoryQuery {
                    annotated_only: true,
                    ..Default::default()
                }
            )
            .total,
            1
        );
    }

    #[test]
    fn site_map_groups_endpoints_and_keeps_marks() {
        let (_dir, db) = open_with(0);
        let add = |path: &str, method: &str, status: Option<u16>| {
            let mut ex = sample(1);
            ex.host = "lab.test".into();
            ex.path = path.into();
            ex.method = method.into();
            ex.status = status;
            ex.url = format!("http://lab.test{path}");
            db.submit(ex).unwrap();
        };
        add("/api/users?id=1", "GET", Some(200));
        add("/api/users?id=2", "GET", Some(404));
        add("/api/users", "POST", Some(201));
        add("/", "GET", Some(200));
        add("/static/app.js", "GET", None);
        db.flush().unwrap();
        db.set_scope(scope_of("lab.test"));

        let map = db.site_map(&HistoryQuery::default()).unwrap();
        assert!(!map.truncated);
        let paths: Vec<&str> = map.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["/", "/api/users", "/static/app.js"]);
        let users = &map.entries[1];
        assert_eq!(users.count, 3);
        assert_eq!(users.methods, vec!["GET", "POST"]);
        assert_eq!(users.statuses, vec![200, 201, 404]);
        assert_eq!(users.sources, vec![Source::Proxy]);
        assert!(users.has_params && users.in_scope);
        assert!(!map.entries[0].has_params);
        assert!(map.entries[2].statuses.is_empty());

        let node = |path: &str| HistoryQuery {
            site: Some(crate::model::SiteFilter {
                scheme: "http".into(),
                host: "lab.test".into(),
                port: 80,
                path: path.into(),
            }),
            ..Default::default()
        };
        assert_eq!(page(&db, node("/api/")).total, 3);
        assert_eq!(page(&db, node("/api/users")).total, 3);
        assert_eq!(
            page(&db, node("/api/user")).total,
            0,
            "endpoints match exactly"
        );
        assert_eq!(page(&db, node("/")).total, 5);
        assert_eq!(db.matching_ids(&node("/api/"), 2).unwrap().len(), 2);

        let mark = |path: &str, m: Option<SiteMark>| {
            db.set_site_mark("http".into(), "lab.test".into(), 80, path.into(), m)
        };
        mark("/api/", Some(SiteMark::Interesting)).unwrap();
        mark("/api/", Some(SiteMark::Reviewed)).unwrap();
        mark("/static/app.js", Some(SiteMark::Ignore)).unwrap();
        mark("/static/app.js", None).unwrap();
        assert!(mark("api", Some(SiteMark::Favorite)).is_err());
        let marks = db.site_map(&HistoryQuery::default()).unwrap().marks;
        assert_eq!(marks.len(), 1);
        assert_eq!(
            (marks[0].path.as_str(), marks[0].mark),
            ("/api/", SiteMark::Reviewed)
        );
    }

    #[test]
    fn rejects_bad_limits() {
        let dir = tempfile::tempdir().unwrap();
        let db = ProjectDb::open(&dir.path().join("p"), OpenOptions::default()).unwrap();
        assert!(
            db.history_page(&HistoryQuery {
                limit: 0,
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            db.history_page(&HistoryQuery {
                limit: 5000,
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn persists_across_reopen_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.chosproxy");
        {
            let db = ProjectDb::open(&path, OpenOptions::default()).unwrap();
            db.submit(sample(1)).unwrap();
        } // drop drains the queue
        let db = ProjectDb::open(&path, OpenOptions::default()).unwrap();
        assert_eq!(db.stats().unwrap().exchanges, 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn queue_full_is_reported_not_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let gate = Arc::new(std::sync::Barrier::new(2));
        let g2 = gate.clone();
        let db = ProjectDb::open(
            &dir.path().join("p"),
            OpenOptions {
                queue_capacity: Some(1),
                on_insert: Some(Arc::new(move |_| {
                    g2.wait();
                })),
                ..OpenOptions::default()
            },
        )
        .unwrap();
        db.submit(sample(1)).unwrap(); // writer takes it and blocks in the hook
        std::thread::sleep(std::time::Duration::from_millis(50));
        db.submit(sample(2)).unwrap(); // fills the queue
        assert!(matches!(db.submit(sample(3)), Err(StorageError::QueueFull)));
        gate.wait();
        gate.wait();
        db.flush().unwrap();
    }

    #[test]
    fn refuses_non_project_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE x(y)")
            .unwrap();
        assert!(matches!(
            ProjectDb::open(&path, OpenOptions::default()),
            Err(StorageError::NotAProject)
        ));
    }
}

#[cfg(test)]
mod read_only_tests {
    use super::*;

    #[test]
    fn read_only_projects_read_but_never_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.chosproxy");
        {
            let db = ProjectDb::open(&path, OpenOptions::default()).unwrap();
            db.set_setting("notes", Some("hello".into())).unwrap();
            db.flush().unwrap();
        }
        assert!(ProjectDb::quick_check(&path, None).unwrap().is_empty());
        let ro = ProjectDb::open(
            &path,
            OpenOptions {
                read_only: true,
                ..OpenOptions::default()
            },
        )
        .unwrap();
        assert!(ro.is_read_only());
        assert_eq!(ro.setting("notes").unwrap().as_deref(), Some("hello"));
        assert!(matches!(
            ro.set_setting("notes", None),
            Err(StorageError::ReadOnly)
        ));
        assert!(ro.flush().is_ok(), "nothing to flush");
        ro.history_page(&HistoryQuery {
            limit: 10,
            ..HistoryQuery::default()
        })
        .unwrap();
        // A copy can still be saved.
        let copy = dir.path().join("copy.chosproxy");
        ro.copy_to(&copy, false).unwrap();
        drop(ro);
        let reopened = ProjectDb::open(&copy, OpenOptions::default()).unwrap();
        assert_eq!(reopened.setting("notes").unwrap().as_deref(), Some("hello"));
        // Missing files and other files are refused.
        let ro_opts = || OpenOptions {
            read_only: true,
            ..OpenOptions::default()
        };
        assert!(ProjectDb::open(&dir.path().join("none"), ro_opts()).is_err());
        let other = dir.path().join("other.db");
        rusqlite::Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert!(matches!(
            ProjectDb::open(&other, ro_opts()),
            Err(StorageError::NotAProject)
        ));
    }
}
