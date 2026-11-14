//! A minimal Chrome DevTools Protocol client: attaches to every page and
//! frame of the lab browser, installs the instrumentation before the page's
//! scripts run, and turns binding calls into [`DomReport`]s.
//!
//! The browser listens on a random loopback port only while the DOM
//! Inspector is on; ChosProxy finds it in the profile's `DevToolsActivePort`
//! file (the profile directory is private to the user).

use crate::report::DomReport;
use chosproxy_websocket::{Assembler, Opcode, encode_frame, random_mask, read_frame};
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Why the DevTools session ended or could not start.
#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    /// The browser did not open its DevTools endpoint in time.
    #[error("the browser did not open its DevTools endpoint (is it a Chromium-family browser?)")]
    NoEndpoint,
    /// Network failure.
    #[error("DevTools connection failed: {0}")]
    Io(#[from] std::io::Error),
    /// The handshake or a message was not what DevTools sends.
    #[error("unexpected DevTools answer: {0}")]
    Protocol(String),
}

/// Reads the port and browser endpoint path Chromium writes to
/// `<profile>/DevToolsActivePort`, waiting up to `wait`.
pub async fn devtools_endpoint(profile: &Path, wait: Duration) -> Result<(u16, String), CdpError> {
    let file = profile.join("DevToolsActivePort");
    let deadline = Instant::now() + wait;
    loop {
        if let Ok(text) = tokio::fs::read_to_string(&file).await {
            let mut lines = text.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next())
                && let Ok(port) = port.trim().parse::<u16>()
                && path.starts_with("/devtools/browser/")
            {
                return Ok((port, path.trim().to_string()));
            }
        }
        if Instant::now() >= deadline {
            return Err(CdpError::NoEndpoint);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// What gets installed in every frame.
#[derive(Debug, Clone)]
pub struct Instrumentation {
    /// The script with this session's binding name and canary.
    pub script: String,
    /// Name of the DevTools binding (random per session).
    pub binding: String,
    /// The session canary.
    pub canary: String,
}

impl Instrumentation {
    /// A new session: random binding name and canary.
    pub fn new() -> Result<Instrumentation, CdpError> {
        let binding = format!("__chosproxy_{}", random_text(16)?);
        let canary = format!("chos{}", random_text(10)?);
        let script = crate::INSPECTOR_JS
            .replace("__CHOSPROXY_BINDING__", &binding)
            .replace("__CHOSPROXY_CANARY__", &canary);
        Ok(Instrumentation {
            script,
            binding,
            canary,
        })
    }
}

fn random_text(len: usize) -> Result<String, CdpError> {
    use ring::rand::SecureRandom;
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut raw = vec![0u8; len];
    ring::rand::SystemRandom::new()
        .fill(&mut raw)
        .map_err(|_| CdpError::Protocol("no secure random source".into()))?;
    Ok(raw
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(b & 31)]))
        .collect())
}

/// The writing side of a DevTools connection.
struct Client {
    writer: tokio::net::tcp::OwnedWriteHalf,
    next_id: u64,
}

impl Client {
    async fn send(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<(), CdpError> {
        self.next_id += 1;
        let mut message = json!({ "id": self.next_id, "method": method, "params": params });
        if let Some(s) = session {
            message["sessionId"] = Value::String(s.to_string());
        }
        let bytes = serde_json::to_vec(&message).map_err(|e| CdpError::Protocol(e.to_string()))?;
        self.writer
            .write_all(&encode_frame(Opcode::Text, &bytes, Some(random_mask())))
            .await?;
        Ok(())
    }
}

pub(crate) async fn connect(port: u16, path: &str) -> Result<TcpStream, CdpError> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let key = chosproxy_websocket::random_key();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16 * 1024 || stream.read(&mut byte).await? == 0 {
            return Err(CdpError::Protocol("no WebSocket handshake".into()));
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head);
    if !text.starts_with("HTTP/1.1 101") {
        return Err(CdpError::Protocol(
            text.lines().next().unwrap_or("").to_string(),
        ));
    }
    let expected = chosproxy_websocket::accept_key(&key);
    let accepted = text
        .lines()
        .filter_map(|l| l.split_once(':'))
        .any(|(n, v)| {
            n.trim().eq_ignore_ascii_case("sec-websocket-accept") && v.trim() == expected
        });
    if !accepted {
        return Err(CdpError::Protocol("wrong Sec-WebSocket-Accept".into()));
    }
    Ok(stream)
}

/// Target types the instrumentation goes into.
fn instrumented(kind: &str) -> bool {
    kind == "page" || kind == "iframe"
}

/// Runs the session until the browser closes: attaches to every page and
/// frame, installs `inst` before page scripts run, opens `start_url` in the
/// first page, opens every URL received on `navigate` in that page, and
/// forwards reports (dropped when `reports` is full).
pub async fn run(
    port: u16,
    path: &str,
    inst: Instrumentation,
    mut start_url: Option<String>,
    mut navigate: mpsc::Receiver<String>,
    reports: mpsc::Sender<DomReport>,
) -> Result<(), CdpError> {
    let stream = connect(port, path).await?;
    let (read_half, writer) = stream.into_split();
    let mut client = Client { writer, next_id: 0 };
    // Frames are read in their own task: reading is not cancel-safe, so it
    // must never be one branch of a select.
    let (frames_tx, mut frames) =
        mpsc::channel::<Result<chosproxy_websocket::Message, String>>(256);
    let reader_task = tokio::spawn(async move {
        let mut reader = BufReader::new(read_half);
        let mut assembler = Assembler::default();
        loop {
            match read_frame(&mut reader).await {
                Ok(Some(frame)) => {
                    if let Some(m) = assembler.push(frame)
                        && frames_tx.send(Ok(m)).await.is_err()
                    {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    let _ = frames_tx.send(Err(e.to_string())).await;
                    return;
                }
            }
        }
    });
    let _abort_reader = AbortOnDrop(reader_task);
    let auto_attach =
        json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true });
    client
        .send("Target.setAutoAttach", auto_attach.clone(), None)
        .await?;
    // The first top-level page: where requested URLs are opened.
    let mut main_page: Option<String> = None;
    loop {
        let message = tokio::select! {
            m = frames.recv() => match m {
                Some(Ok(m)) => m,
                Some(Err(e)) => return Err(CdpError::Protocol(e)),
                None => return Ok(()),
            },
            Some(url) = navigate.recv() => {
                if let Some(page) = main_page.as_deref() {
                    client.send("Page.navigate", json!({ "url": url }), Some(page)).await?;
                }
                continue;
            }
        };
        match message.opcode {
            Opcode::Close => return Ok(()),
            Opcode::Ping => {
                client
                    .writer
                    .write_all(&encode_frame(
                        Opcode::Pong,
                        &message.payload,
                        Some(random_mask()),
                    ))
                    .await?;
                continue;
            }
            Opcode::Text => {}
            _ => continue,
        }
        let Ok(value) = serde_json::from_slice::<Value>(&message.payload) else {
            continue;
        };
        let params = &value["params"];
        match value["method"].as_str() {
            Some("Target.attachedToTarget") => {
                let Some(session) = params["sessionId"].as_str() else {
                    continue;
                };
                let kind = params["targetInfo"]["type"].as_str().unwrap_or("");
                let session = Some(session);
                if instrumented(kind) {
                    // Without the Page domain enabled, Chrome does not run
                    // the script in documents loaded after a navigation.
                    client.send("Page.enable", json!({}), session).await?;
                    client
                        .send(
                            "Runtime.addBinding",
                            json!({ "name": inst.binding }),
                            session,
                        )
                        .await?;
                    client
                        .send(
                            "Page.addScriptToEvaluateOnNewDocument",
                            json!({ "source": inst.script, "runImmediately": true }),
                            session,
                        )
                        .await?;
                    client.send("Runtime.enable", json!({}), session).await?;
                    // Out-of-process iframes of this page, paused until set up.
                    client
                        .send("Target.setAutoAttach", auto_attach.clone(), session)
                        .await?;
                }
                client
                    .send("Runtime.runIfWaitingForDebugger", json!({}), session)
                    .await?;
                if kind == "page" && main_page.is_none() {
                    main_page = session.map(str::to_string);
                }
                if kind == "page"
                    && let Some(url) = start_url.take()
                {
                    client
                        .send("Page.navigate", json!({ "url": url }), session)
                        .await?;
                }
            }
            Some("Target.detachedFromTarget")
                if params["sessionId"].as_str().is_some()
                    && params["sessionId"].as_str() == main_page.as_deref() =>
            {
                main_page = None;
            }
            Some("Runtime.bindingCalled")
                if params["name"].as_str() == Some(inst.binding.as_str()) =>
            {
                if let Some(payload) = params["payload"].as_str() {
                    match DomReport::parse(payload) {
                        Ok(report) => {
                            let _ = reports.try_send(report);
                        }
                        Err(e) => tracing::debug!(error = %e, "ignored DOM report"),
                    }
                }
            }
            _ => {}
        }
    }
}

/// Aborts a task when dropped.
pub(crate) struct AbortOnDrop(pub(crate) tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
