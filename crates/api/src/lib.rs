//! The automation API (M28, matrix PLATFORM-003, ADR-0038): a JSON API over
//! the engine on `127.0.0.1`, for scripts and the `chosproxy-cli` client.
//!
//! The rules that keep it safe:
//! - off unless the tester turns it on (`api.toml`), and it binds to
//!   loopback only;
//! - every request needs `Authorization: Bearer <token>` (compared in
//!   constant time); the token is in a private file (see [`token`]);
//! - requests with an `Origin` header (browsers) are refused, and the
//!   `Host` header must name the loopback listener (no DNS rebinding);
//! - bodies are limited; calls that change something are audited;
//! - it offers what the UI offers, with the same checks (scope, safe
//!   defaults): no new capability, no attack payloads.

pub mod routes;
pub mod token;

use bytes::Bytes;
use chosproxy_app::{AppError, Engine, ReportFormat, ReportOptions, ScanLaunch};
use chosproxy_core::ScopeConfig;
use chosproxy_crawler::CrawlConfig;
use chosproxy_storage::{FindingQuery, HistoryQuery, RepeaterTarget};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST, ORIGIN};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Largest request body accepted.
pub const MAX_BODY: usize = 10 * 1024 * 1024;
/// Version of the API paths (`/v1/…`).
pub const API_VERSION: u32 = 1;

/// Pause after a failed `accept`.
const ACCEPT_BACKOFF: std::time::Duration = std::time::Duration::from_millis(100);
/// Most event streams (`GET /v1/events`) open at once.
pub const MAX_EVENT_STREAMS: usize = 8;
/// Events that may wait for one slow client before it is dropped.
const EVENT_QUEUE: usize = 256;
/// A comment line is sent this often so a gone client is noticed.
const EVENT_PING: std::time::Duration = std::time::Duration::from_secs(15);

struct Ctx {
    engine: Engine,
    token: String,
    port: u16,
    /// Event streams open now.
    streams: std::sync::atomic::AtomicUsize,
    /// Becomes true when the server stops (ends the event streams).
    closed: tokio::sync::watch::Sender<bool>,
}

type Body = BoxBody<Bytes, Infallible>;

/// A response body fed through a channel (the event stream).
struct ChannelBody(tokio::sync::mpsc::Receiver<Bytes>);

impl hyper::body::Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Bytes>, Infallible>>> {
        self.0
            .poll_recv(cx)
            .map(|b| b.map(|b| Ok(hyper::body::Frame::data(b))))
    }
}

/// Counts an open event stream until it is dropped.
struct StreamSlot(Arc<Ctx>);

impl Drop for StreamSlot {
    fn drop(&mut self) {
        self.0
            .streams
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// `GET /v1/events`: engine events as Server-Sent Events. Each event is
/// `event: <type>` and `data: <the event as JSON>`. A client that does not
/// read fast enough (its queue is full) is disconnected rather than slowing
/// the engine; a client that missed events gets `event: lagged`.
fn events(ctx: &Arc<Ctx>) -> Response<Body> {
    use std::sync::atomic::Ordering;
    if ctx.streams.fetch_add(1, Ordering::SeqCst) >= MAX_EVENT_STREAMS {
        ctx.streams.fetch_sub(1, Ordering::SeqCst);
        return fail(
            StatusCode::TOO_MANY_REQUESTS,
            "too many event streams are open",
        );
    }
    let slot = StreamSlot(ctx.clone());
    let mut rx = ctx.engine.subscribe();
    let mut closed = ctx.closed.subscribe();
    let (tx, body) = tokio::sync::mpsc::channel::<Bytes>(EVENT_QUEUE);
    tokio::spawn(async move {
        let _slot = slot;
        let mut ping = tokio::time::interval(EVENT_PING);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let _ = tx.try_send(Bytes::from_static(b": connected\n\n"));
        loop {
            let line = tokio::select! {
                _ = closed.changed() => break,
                () = tx.closed() => break,
                _ = ping.tick() => Bytes::from_static(b": ping\n\n"),
                event = rx.recv() => match event {
                    Ok(event) => {
                        let Ok(data) = serde_json::to_value(&event) else { continue };
                        let kind = data["type"].as_str().unwrap_or("event").to_string();
                        Bytes::from(format!("event: {kind}\ndata: {data}\n\n"))
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        Bytes::from(format!("event: lagged\ndata: {{\"missed\":{n}}}\n\n"))
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            };
            // A full queue means the client is not reading: drop it.
            if tx.try_send(line).is_err() {
                break;
            }
        }
    });
    let mut res = Response::new(ChannelBody(body).boxed());
    res.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("text/event-stream"),
    );
    // The connection ends with the stream.
    res.headers_mut().insert(
        hyper::header::CONNECTION,
        hyper::header::HeaderValue::from_static("close"),
    );
    res.headers_mut().insert(
        "cache-control",
        hyper::header::HeaderValue::from_static("no-store"),
    );
    res
}

/// A running API server; stops when dropped.
#[derive(Debug)]
pub struct ApiServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl ApiServer {
    /// Listens on `127.0.0.1:port` (0 picks a free port) with `token`.
    pub async fn start(engine: Engine, port: u16, token: String) -> std::io::Result<ApiServer> {
        if token.len() < 32 {
            return Err(std::io::Error::other("the API token is too short"));
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let addr = listener.local_addr()?;
        let ctx = Arc::new(Ctx {
            engine,
            token,
            port: addr.port(),
            streams: std::sync::atomic::AtomicUsize::new(0),
            closed: tokio::sync::watch::channel(false).0,
        });
        let (tx, mut rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => {
                        let _ = ctx.closed.send(true);
                        break;
                    }
                    accepted = listener.accept() => {
                        let (stream, peer) = match accepted {
                            Ok(x) => x,
                            Err(e) => {
                                // Typically out of file descriptors: wait
                                // instead of spinning on the error.
                                tracing::warn!(error = %e, "API accept failed");
                                tokio::time::sleep(ACCEPT_BACKOFF).await;
                                continue;
                            }
                        };
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let mut closed = ctx.closed.subscribe();
                            let service = service_fn(move |req| handle(ctx.clone(), req));
                            let conn = http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service);
                            tokio::pin!(conn);
                            tokio::select! {
                                served = &mut conn => {
                                    if let Err(e) = served {
                                        tracing::debug!(error = %e, "API connection ended with an error");
                                    }
                                }
                                // The API was turned off: a connection kept
                                // open must not go on being served.
                                _ = closed.changed() => {
                                    conn.as_mut().graceful_shutdown();
                                    let _ = tokio::time::timeout(
                                        std::time::Duration::from_secs(2),
                                        conn,
                                    )
                                    .await;
                                }
                            }
                        });
                    }
                }
            }
        });
        tracing::info!(%addr, "automation API listening");
        Ok(ApiServer {
            addr,
            shutdown: Some(tx),
            task: Some(task),
        })
    }

    /// The address it listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops accepting connections.
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// An error answer.
struct Failure {
    status: StatusCode,
    message: String,
}

impl Failure {
    fn new(status: StatusCode, message: impl Into<String>) -> Failure {
        Failure {
            status,
            message: message.into(),
        }
    }
}

impl From<AppError> for Failure {
    fn from(e: AppError) -> Failure {
        let status = match &e {
            AppError::Invalid(_) => StatusCode::BAD_REQUEST,
            AppError::UnknownExchange(_) | AppError::UnknownListener(_) => StatusCode::NOT_FOUND,
            AppError::NoProject => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Failure::new(status, e.to_string())
    }
}

type Answer = Result<Value, Failure>;

fn respond(status: StatusCode, body: &Value) -> Response<Body> {
    let mut res = Response::new(Full::new(Bytes::from(body.to_string())).boxed());
    *res.status_mut() = status;
    res.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    res.headers_mut().insert(
        "cache-control",
        hyper::header::HeaderValue::from_static("no-store"),
    );
    res
}

fn fail(status: StatusCode, message: &str) -> Response<Body> {
    respond(status, &json!({ "error": message }))
}

/// `Host` must name this listener on loopback.
fn host_allowed(req: &Request<Incoming>, port: u16) -> bool {
    let Some(host) = req.headers().get(HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .contains(&host)
}

fn authorized(req: &Request<Incoming>, token: &str) -> bool {
    let Some(given) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
    else {
        return false;
    };
    given.len() == token.len() && bool::from(given.as_bytes().ct_eq(token.as_bytes()))
}

async fn handle(ctx: Arc<Ctx>, req: Request<Incoming>) -> Result<Response<Body>, Infallible> {
    if req.headers().contains_key(ORIGIN) {
        return Ok(fail(
            StatusCode::FORBIDDEN,
            "requests from web pages (with an Origin header) are not accepted",
        ));
    }
    if !host_allowed(&req, ctx.port) {
        return Ok(fail(
            StatusCode::FORBIDDEN,
            "the Host header must be 127.0.0.1 or localhost",
        ));
    }
    if !authorized(&req, &ctx.token) {
        return Ok(fail(StatusCode::UNAUTHORIZED, "missing or wrong API token"));
    }
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    if method == Method::GET && path == "/v1/events" {
        return Ok(events(&ctx));
    }
    if method == Method::GET && path == "/v1/openapi.json" {
        return Ok(respond(StatusCode::OK, &routes::openapi(ctx.port)));
    }
    let query: Vec<(String, String)> = req
        .uri()
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => {
            return Ok(fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                "the request body is too large",
            ));
        }
    };
    let answer = route(&ctx.engine, &method, &path, &query, &body).await;
    Ok(match answer {
        Ok(value) => respond(StatusCode::OK, &value),
        Err(f) => fail(f.status, &f.message),
    })
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, Failure> {
    let body = if body.is_empty() {
        b"{}".as_slice()
    } else {
        body
    };
    serde_json::from_slice(body)
        .map_err(|e| Failure::new(StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")))
}

fn to_value<T: serde::Serialize>(value: T) -> Answer {
    serde_json::to_value(value)
        .map_err(|e| Failure::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn id_of(segment: &str) -> Result<i64, Failure> {
    segment
        .parse()
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, format!("{segment:?} is not an id")))
}

fn param<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathBody {
    path: String,
    /// Password of an encrypted project (M31); never logged.
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeAdd {
    url: String,
    #[serde(default)]
    exclude: bool,
    #[serde(default)]
    path: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendBody {
    target: RepeaterTarget,
    request: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefinitionBody {
    name: String,
    text: String,
    #[serde(default)]
    base_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportBody {
    #[serde(default)]
    format: Option<ReportFormat>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    include_evidence: Option<bool>,
}

async fn route(
    engine: &Engine,
    method: &Method,
    path: &str,
    query: &[(String, String)],
    body: &[u8],
) -> Answer {
    let Some(rest) = path.strip_prefix("/v1/") else {
        return Err(Failure::new(
            StatusCode::NOT_FOUND,
            "unknown path (the API lives under /v1/)",
        ));
    };
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match (method, segments.as_slice()) {
        (&Method::GET, ["status"]) => Ok(json!({
            "api": API_VERSION,
            "version": env!("CARGO_PKG_VERSION"),
            "project": engine.project(),
            "listeners": engine.listeners().await,
        })),

        // Projects.
        (&Method::POST, ["project", "temporary"]) => {
            engine.audit_automation("project", "temporary project");
            to_value(engine.new_temporary_project().await?)
        }
        (&Method::POST, ["project", "open"]) => {
            let b: PathBody = parse(body)?;
            to_value(
                engine
                    .open_project_with(std::path::Path::new(&b.path), b.password.as_deref())
                    .await?,
            )
        }
        (&Method::POST, ["project", "create"]) => {
            let b: PathBody = parse(body)?;
            to_value(
                engine
                    .create_project_with(std::path::Path::new(&b.path), b.password.as_deref())
                    .await?,
            )
        }
        (&Method::POST, ["project", "close"]) => {
            engine.close_project().await?;
            Ok(json!({ "closed": true }))
        }

        // Proxy listeners.
        (&Method::GET, ["listeners"]) => to_value(engine.listeners().await),
        (&Method::POST, ["listeners", id, action]) => {
            match *action {
                "start" => engine.start_listener(id).await?,
                "stop" => engine.stop_listener(id).await?,
                _ => return Err(Failure::new(StatusCode::NOT_FOUND, "use start or stop")),
            }
            engine.audit_automation("listener", &format!("{action} {id}"));
            to_value(engine.listeners().await)
        }

        // Traffic and findings.
        (&Method::GET, ["history"]) => {
            let limit = param(query, "limit")
                .and_then(|l| l.parse().ok())
                .unwrap_or(100);
            let offset = param(query, "offset")
                .and_then(|l| l.parse().ok())
                .unwrap_or(0);
            to_value(
                engine
                    .history_page(HistoryQuery {
                        limit: limit.clamp(1, HistoryQuery::MAX_LIMIT),
                        offset,
                        search: param(query, "search").map(str::to_string),
                        in_scope_only: param(query, "inScope") == Some("true"),
                        ..Default::default()
                    })
                    .await?,
            )
        }
        (&Method::POST, ["history", "query"]) => {
            let mut q: HistoryQuery = parse(body)?;
            q.limit = q.limit.clamp(1, HistoryQuery::MAX_LIMIT);
            to_value(engine.history_page(q).await?)
        }
        (&Method::GET, ["history", id]) => to_value(engine.exchange_detail(id_of(id)?).await?),
        (&Method::GET, ["findings"]) => to_value(engine.findings(FindingQuery::default()).await?),
        (&Method::POST, ["findings", "query"]) => {
            let q: FindingQuery = parse(body)?;
            to_value(engine.findings(q).await?)
        }

        // Scope.
        (&Method::GET, ["scope"]) => to_value(engine.scope_config()),
        (&Method::PUT, ["scope"]) => {
            let scope: ScopeConfig = parse(body)?;
            let saved = engine.save_scope_config(scope)?;
            engine.audit_automation("scope", "scope replaced");
            to_value(saved)
        }
        (&Method::POST, ["scope", "add"]) => {
            let b: ScopeAdd = parse(body)?;
            let saved = engine.add_url_to_scope(&b.url, b.exclude, b.path)?;
            engine.audit_automation(
                "scope",
                &format!("{} {}", if b.exclude { "excluded" } else { "added" }, b.url),
            );
            to_value(saved)
        }

        // Tools.
        (&Method::POST, ["crawl"]) => {
            let config: CrawlConfig = parse(body)?;
            let task = engine.crawl_start(config).await?;
            engine.audit_automation("crawl", &task.name);
            to_value(task)
        }
        (&Method::GET, ["scan", "configs"]) => to_value(engine.scan_configs()),
        (&Method::POST, ["scan"]) => {
            let launch: ScanLaunch = parse(body)?;
            let task = engine.scan_start(launch).await?;
            engine.audit_automation("scan", &task.name);
            to_value(task)
        }
        (&Method::POST, ["send"]) => {
            let b: SendBody = parse(body)?;
            let sent = engine.automation_send(b.target, &b.request).await?;
            to_value(sent)
        }
        (&Method::GET, ["api-definitions"]) => to_value(engine.api_definitions().await?),
        (&Method::POST, ["api-definitions"]) => {
            let b: DefinitionBody = parse(body)?;
            to_value(engine.api_import_text(&b.name, &b.text, b.base_url).await?)
        }
        (&Method::GET, ["api-definitions", id]) => {
            to_value(engine.api_definition(id_of(id)?).await?)
        }

        // Tasks.
        (&Method::GET, ["tasks"]) => to_value(engine.tasks().await?),
        (&Method::GET, ["tasks", id]) => to_value(engine.task_detail(id_of(id)?).await?),
        (&Method::POST, ["tasks", id, action]) => {
            let id = id_of(id)?;
            match *action {
                "pause" => engine.task_pause(id)?,
                "resume" => engine.task_resume(id)?,
                "cancel" => engine.task_cancel(id)?,
                _ => {
                    return Err(Failure::new(
                        StatusCode::NOT_FOUND,
                        "use pause, resume or cancel",
                    ));
                }
            }
            to_value(engine.task_detail(id).await?.row)
        }

        // Reports.
        (&Method::POST, ["report"]) => {
            let b: ReportBody = parse(body)?;
            let format = b.format.unwrap_or(ReportFormat::Markdown);
            let findings = engine.findings(FindingQuery::default()).await?;
            let options = ReportOptions {
                title: b.title.unwrap_or_else(|| "Security assessment".into()),
                client: String::new(),
                author: String::new(),
                executive_summary: String::new(),
                methodology: String::new(),
                finding_ids: findings.iter().map(|f| f.id).collect(),
                include_evidence: b.include_evidence.unwrap_or(true),
                redact_secrets: true,
                max_evidence_kib: 64,
            };
            let dir = engine.paths().temp_dir.clone();
            chosproxy_core::fs::create_private_dir(&dir).map_err(AppError::Io)?;
            let file = dir.join(format!("api-report-{}.tmp", std::process::id()));
            let written = engine.export_report(options, format, &file).await?;
            let content = std::fs::read_to_string(&written).map_err(AppError::Io);
            let _ = std::fs::remove_file(&written);
            Ok(json!({ "format": format, "findings": findings.len(), "content": content? }))
        }

        _ => Err(Failure::new(
            StatusCode::NOT_FOUND,
            format!("no {method} /v1/{rest}"),
        )),
    }
}
