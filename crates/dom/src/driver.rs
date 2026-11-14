//! Driving the lab browser for crawling (M24, matrix CRAWL-002): open a
//! URL in the browser's page, wait until it has loaded and settled, and
//! read the links of the *rendered* document — including those scripts
//! added. The browser's traffic goes through the ChosProxy listener like
//! any browsing, so it is captured in the history.

use crate::cdp::CdpError;
use chosproxy_websocket::{Assembler, Opcode, encode_frame, random_mask, read_frame};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// How long one DevTools call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Collects absolute http(s) URLs of links, frames and forms.
const LINKS_JS: &str = r#"JSON.stringify(Array.from(new Set(
  Array.from(document.querySelectorAll('a[href],area[href],iframe[src],frame[src],form[action]'))
    .map(e => e.href || e.src || e.action)
    .filter(u => typeof u === 'string' && /^https?:/.test(u))
    .map(u => u.split('#')[0])
)).slice(0, 2000))"#;

/// What a page showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageVisit {
    /// URL after redirects.
    pub url: String,
    /// Document title.
    pub title: String,
    /// Links found in the rendered document.
    pub links: Vec<String>,
}

/// A connection to the browser, attached to one page.
pub struct Driver {
    writer: tokio::net::tcp::OwnedWriteHalf,
    messages: mpsc::Receiver<Value>,
    next_id: u64,
    session: String,
    _reader: crate::cdp::AbortOnDrop,
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver").finish_non_exhaustive()
    }
}

impl Driver {
    /// Connects to the DevTools endpoint and attaches to the first page.
    pub async fn connect(port: u16, path: &str) -> Result<Driver, CdpError> {
        let stream = crate::cdp::connect(port, path).await?;
        let (read_half, writer) = stream.into_split();
        let (tx, messages) = mpsc::channel::<Value>(512);
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut assembler = Assembler::default();
            while let Ok(Some(frame)) = read_frame(&mut reader).await {
                if let Some(m) = assembler.push(frame)
                    && m.opcode == Opcode::Text
                    && let Ok(v) = serde_json::from_slice::<Value>(&m.payload)
                    && tx.send(v).await.is_err()
                {
                    return;
                }
            }
        });
        let mut d = Driver {
            writer,
            messages,
            next_id: 0,
            session: String::new(),
            _reader: crate::cdp::AbortOnDrop(reader),
        };
        let targets = d.call("Target.getTargets", json!({}), false).await?;
        let page = targets["targetInfos"]
            .as_array()
            .and_then(|t| t.iter().find(|t| t["type"] == "page"))
            .and_then(|t| t["targetId"].as_str())
            .map(str::to_string);
        let target = match page {
            Some(t) => t,
            None => d
                .call(
                    "Target.createTarget",
                    json!({ "url": "about:blank" }),
                    false,
                )
                .await?["targetId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        };
        let attached = d
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
                false,
            )
            .await?;
        d.session = attached["sessionId"]
            .as_str()
            .ok_or_else(|| CdpError::Protocol("no session for the page".into()))?
            .to_string();
        d.call("Page.enable", json!({}), true).await?;
        d.call("Runtime.enable", json!({}), true).await?;
        Ok(d)
    }

    /// Sends a command and waits for its answer (events are skipped).
    async fn call(
        &mut self,
        method: &str,
        params: Value,
        in_page: bool,
    ) -> Result<Value, CdpError> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({ "id": id, "method": method, "params": params });
        if in_page {
            message["sessionId"] = Value::String(self.session.clone());
        }
        let bytes = serde_json::to_vec(&message).map_err(|e| CdpError::Protocol(e.to_string()))?;
        self.writer
            .write_all(&encode_frame(Opcode::Text, &bytes, Some(random_mask())))
            .await?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let m = tokio::time::timeout(left, self.messages.recv())
                .await
                .map_err(|_| CdpError::Protocol(format!("{method} timed out")))?
                .ok_or_else(|| CdpError::Protocol("the browser closed".into()))?;
            if m["id"].as_u64() == Some(id) {
                if let Some(err) = m.get("error") {
                    return Err(CdpError::Protocol(format!("{method}: {err}")));
                }
                return Ok(m["result"].clone());
            }
        }
    }

    async fn eval(&mut self, expression: &str) -> Result<Value, CdpError> {
        let r = self
            .call(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true }),
                true,
            )
            .await?;
        Ok(r["result"]["value"].clone())
    }

    /// Opens `url`, waits until the document has loaded (at most `wait`)
    /// and has settled, and reads its links.
    pub async fn visit(&mut self, url: &str, wait: Duration) -> Result<PageVisit, CdpError> {
        let nav = self
            .call("Page.navigate", json!({ "url": url }), true)
            .await?;
        if let Some(e) = nav["errorText"].as_str()
            && !e.is_empty()
        {
            return Err(CdpError::Protocol(format!("{url}: {e}")));
        }
        let deadline = Instant::now() + wait;
        // `about:blank` reports "complete" before the navigation commits.
        tokio::time::sleep(Duration::from_millis(200)).await;
        while self.eval("document.readyState").await?.as_str() != Some("complete")
            && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        // Scripts often add links right after load.
        tokio::time::sleep(Duration::from_millis(600)).await;
        let links = self.eval(LINKS_JS).await?;
        let links: Vec<String> = links
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        Ok(PageVisit {
            url: self
                .eval("location.href")
                .await?
                .as_str()
                .unwrap_or(url)
                .to_string(),
            title: self
                .eval("document.title")
                .await?
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect(),
            links,
        })
    }
}
