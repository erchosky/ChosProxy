//! A small local HTTP server used by ChosProxy's integration tests and for
//! manual testing (`chosproxy-lab`).
//!
//! It is implemented with `hyper`, independently of ChosProxy's own HTTP
//! codec, so tests do not validate the codec against itself. It contains no
//! real vulnerabilities and binds to loopback unless explicitly told not to.

mod dom_pages;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::{Frame, Incoming};
use hyper::header::{self, HeaderValue};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

type Body = http_body_util::combinators::BoxBody<Bytes, Infallible>;

/// A running lab server.
#[derive(Debug)]
pub struct LabServer {
    addr: SocketAddr,
    tls: Option<LabTls>,
    shutdown: Option<oneshot::Sender<()>>,
}

/// TLS material of an HTTPS lab server.
#[derive(Debug, Clone)]
pub struct LabTls {
    /// Self-signed certificate (DER) valid for `localhost`, `127.0.0.1`, `::1`.
    pub cert_der: rustls::pki_types::CertificateDer<'static>,
}

async fn serve<S>(stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // HTTP/1.1, or HTTP/2 when the client starts with the HTTP/2 preface
    // (after ALPN `h2`, or h2c with prior knowledge).
    let builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
    // Each connection has a number, for what is tied to a connection (NTLM).
    static NEXT_CONN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let conn_id = ConnId(NEXT_CONN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let conn = builder.serve_connection_with_upgrades(
        TokioIo::new(stream),
        service_fn(move |mut req: Request<Incoming>| {
            req.extensions_mut().insert(conn_id);
            route(req)
        }),
    );
    if let Err(e) = conn.await {
        tracing::debug!(error = %e, "lab connection ended with error");
    }
}

fn self_signed(http2: bool) -> std::io::Result<(LabTls, Arc<rustls::ServerConfig>)> {
    let to_io = |e: &dyn std::fmt::Display| std::io::Error::other(e.to_string());
    let key = rcgen::KeyPair::generate().map_err(|e| to_io(&e))?;
    let names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    let cert = rcgen::CertificateParams::new(names)
        .and_then(|p| p.self_signed(&key))
        .map_err(|e| to_io(&e))?;
    let cert_der = cert.der().clone();
    let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into());
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| to_io(&e))?
    .with_no_client_auth()
    .with_single_cert(vec![cert_der.clone()], key_der)
    .map_err(|e| to_io(&e))?;
    config.alpn_protocols = if http2 {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    } else {
        vec![b"http/1.1".to_vec()]
    };
    Ok((LabTls { cert_der }, Arc::new(config)))
}

impl LabServer {
    /// Starts the HTTP lab on `addr` (use port 0 for an ephemeral port).
    pub async fn start(addr: SocketAddr) -> std::io::Result<LabServer> {
        LabServer::start_inner(addr, false, true).await
    }

    /// Starts an HTTPS lab with a fresh self-signed certificate.
    pub async fn start_tls(addr: SocketAddr) -> std::io::Result<LabServer> {
        LabServer::start_inner(addr, true, true).await
    }

    /// Starts an HTTPS lab that offers only HTTP/1.1 through ALPN.
    pub async fn start_tls_http1(addr: SocketAddr) -> std::io::Result<LabServer> {
        LabServer::start_inner(addr, true, false).await
    }

    async fn start_inner(addr: SocketAddr, tls: bool, http2: bool) -> std::io::Result<LabServer> {
        let listener = TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        let (tls_info, acceptor) = if tls {
            let (info, config) = self_signed(http2)?;
            (Some(info), Some(tokio_rustls::TlsAcceptor::from(config)))
        } else {
            (None, None)
        };
        let (tx, mut rx) = oneshot::channel();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { continue };
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            match acceptor {
                                Some(a) => match a.accept(stream).await {
                                    Ok(s) => serve(s).await,
                                    Err(e) => tracing::debug!(error = %e, "lab TLS handshake failed"),
                                },
                                None => serve(stream).await,
                            }
                        });
                    }
                }
            }
        });
        Ok(LabServer {
            addr,
            tls: tls_info,
            shutdown: Some(tx),
        })
    }

    /// Bound address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// TLS details if this is an HTTPS lab.
    pub fn tls(&self) -> Option<&LabTls> {
        self.tls.as_ref()
    }

    /// Base URL, e.g. `http://127.0.0.1:9090` or `https://127.0.0.1:9443`.
    pub fn url(&self) -> String {
        let scheme = if self.tls.is_some() { "https" } else { "http" };
        format!("{scheme}://{}", self.addr)
    }
}

impl Drop for LabServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

fn full(body: impl Into<Bytes>) -> Body {
    Full::new(body.into()).boxed()
}

/// The lab's GraphQL schema, as an introspection answer: `Query.item(id)`,
/// `Query.items`, `Mutation.rename(id, name)` and an `Item` type.
const LAB_GRAPHQL_SCHEMA: &str = r#"{"data":{"__schema":{"queryType":{"name":"Query"},"mutationType":{"name":"Mutation"},"subscriptionType":null,"types":[
{"kind":"OBJECT","name":"Query","fields":[
 {"name":"item","isDeprecated":false,"args":[{"name":"id","type":{"kind":"NON_NULL","name":null,"ofType":{"kind":"SCALAR","name":"ID","ofType":null}},"defaultValue":null}],"type":{"kind":"OBJECT","name":"Item","ofType":null}},
 {"name":"items","isDeprecated":false,"args":[],"type":{"kind":"LIST","name":null,"ofType":{"kind":"OBJECT","name":"Item","ofType":null}}}]},
{"kind":"OBJECT","name":"Mutation","fields":[
 {"name":"rename","isDeprecated":false,"args":[{"name":"id","type":{"kind":"NON_NULL","name":null,"ofType":{"kind":"SCALAR","name":"ID","ofType":null}},"defaultValue":null},{"name":"name","type":{"kind":"SCALAR","name":"String","ofType":null},"defaultValue":null}],"type":{"kind":"OBJECT","name":"Item","ofType":null}}]},
{"kind":"OBJECT","name":"Item","fields":[
 {"name":"id","isDeprecated":false,"args":[],"type":{"kind":"SCALAR","name":"ID","ofType":null}},
 {"name":"name","isDeprecated":false,"args":[],"type":{"kind":"SCALAR","name":"String","ofType":null}}]},
{"kind":"SCALAR","name":"ID"},{"kind":"SCALAR","name":"String"}]}}}"#;

fn respond(status: StatusCode, content_type: &str, body: impl Into<Bytes>) -> Response<Body> {
    let mut res = Response::new(full(body));
    *res.status_mut() = status;
    if let Ok(v) = HeaderValue::from_str(content_type) {
        res.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    res
}

fn query_param(req: &Request<Incoming>, key: &str) -> Option<String> {
    req.uri().query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| v.to_string())
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_param_decoded(req: &Request<Incoming>, key: &str) -> Option<String> {
    query_param(req, key).map(|v| percent_decode(&v))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

static DYNAMIC_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn headers_json(req: &Request<Incoming>) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = req
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                serde_json::Value::String(String::from_utf8_lossy(v.as_bytes()).into_owned()),
            )
        })
        .collect();
    serde_json::Value::Object(map)
}

const INDEX: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>ChosProxy Lab</title></head>
<body>
<h1>ChosProxy local lab</h1>
<p>A harmless local server for exercising the proxy.</p>
<ul>
  <li><a href="/json">/json</a></li>
  <li><a href="/headers">/headers</a></li>
  <li><a href="/cookies/set">/cookies/set</a></li>
  <li><a href="/debug-token">/debug-token</a> (X-Debug-Token header, for the HelloPlugin example)</li>
  <li><a href="/token">/token</a> (random) and <a href="/token/sequential">/token/sequential</a> (counter): tokens for the Sequencer</li>
  <li><a href="/redirect?to=/json">/redirect</a></li>
  <li><a href="/chunked">/chunked</a></li>
  <li><a href="/gzip">/gzip</a></li>
  <li><a href="/large?bytes=200000">/large</a></li>
  <li><a href="/status/404">/status/404</a></li>
  <li><a href="/ws-demo">/ws-demo</a> (WebSocket echo at /ws)</li>
  <li>Unlinked content for content discovery (/admin/, /backup.zip, a soft-404 area under /soft/)</li>
  <li><a href="/dom/">/dom/</a> (intentionally vulnerable client-side pages for the DOM Inspector)</li>
</ul>
<form method="post" action="/form">
  <input name="name" value="alice"><input name="comment" value="hello">
  <button type="submit">Submit form</button>
</form>
<script src="/app.js"></script>
</body></html>
"#;

const WS_DEMO: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>WebSocket demo</title></head>
<body>
<h1>WebSocket echo</h1>
<form id="f"><input id="m" value="hello" autocomplete="off"> <button>Send</button></form>
<pre id="log"></pre>
<script>
const log = (t) => { document.getElementById('log').textContent += t + '\n'; };
const ws = new WebSocket((location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/ws');
ws.onopen = () => log('connected');
ws.onmessage = (e) => log('< ' + e.data);
ws.onclose = (e) => log('closed ' + e.code);
document.getElementById('f').onsubmit = (e) => {
  e.preventDefault();
  const v = document.getElementById('m').value;
  ws.send(v);
  log('> ' + v);
};
</script>
</body></html>
"#;

const HTML: &str = "text/html; charset=utf-8";

fn has_lab_session(req: &Request<Incoming>) -> bool {
    req.headers()
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.split(';').any(|c| c.trim() == "labsession=ok-1"))
}

/// The connection a request came on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ConnId(u64);

/// Challenges of `/auth/ntlm` waiting for their answer, by connection.
static NTLM_PENDING: std::sync::Mutex<Vec<(ConnId, [u8; 8])>> = std::sync::Mutex::new(Vec::new());

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn hmac_md5(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::Mac;
    match hmac::Hmac::<md5::Md5>::new_from_slice(key) {
        Ok(mut mac) => {
            mac.update(data);
            mac.finalize().into_bytes().to_vec()
        }
        Err(_) => Vec::new(),
    }
}

/// `/auth/ntlm`: NTLMv2 for user `lab`, password `secret` (any domain),
/// written from [MS-NLMP] independently of ChosProxy's client. The answer
/// must come on the connection that got the challenge.
fn ntlm_route(conn: Option<ConnId>, authorization: Option<&str>) -> Response<Body> {
    use base64::Engine as _;
    use md4::Digest as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let offer = |value: String| auth_response(false, value);
    let message = authorization
        .and_then(|v| v.strip_prefix("NTLM "))
        .and_then(|v| b64.decode(v.trim()).ok())
        .filter(|m| m.len() >= 12 && &m[..8] == b"NTLMSSP\0");
    let (Some(conn), Some(m)) = (conn, message) else {
        return offer("NTLM".into());
    };
    let mut pending = NTLM_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    match m[8] {
        1 => {
            let mut challenge = [0u8; 8];
            let _ =
                ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut challenge);
            pending.retain(|(c, _)| *c != conn);
            if pending.len() > 256 {
                pending.remove(0);
            }
            pending.push((conn, challenge));
            // Target information: the domain name "LAB", then the end.
            let mut info = vec![2, 0, 6, 0];
            info.extend_from_slice(&utf16le("LAB"));
            info.extend_from_slice(&[0, 0, 0, 0]);
            let mut c = Vec::new();
            c.extend_from_slice(b"NTLMSSP\0");
            c.extend_from_slice(&2u32.to_le_bytes());
            c.extend_from_slice(&[0; 8]);
            c.extend_from_slice(&0x0080_0201u32.to_le_bytes());
            c.extend_from_slice(&challenge);
            c.extend_from_slice(&[0; 8]);
            c.extend_from_slice(&(info.len() as u16).to_le_bytes());
            c.extend_from_slice(&(info.len() as u16).to_le_bytes());
            c.extend_from_slice(&48u32.to_le_bytes());
            c.extend_from_slice(&info);
            offer(format!("NTLM {}", b64.encode(c)))
        }
        3 => {
            let Some(at) = pending.iter().position(|(c, _)| *c == conn) else {
                // No challenge on this connection: start again.
                return offer("NTLM".into());
            };
            let (_, challenge) = pending.remove(at);
            let field = |at: usize| -> Option<&[u8]> {
                let len = usize::from(u16::from_le_bytes([*m.get(at)?, *m.get(at + 1)?]));
                let off = u32::from_le_bytes([
                    *m.get(at + 4)?,
                    *m.get(at + 5)?,
                    *m.get(at + 6)?,
                    *m.get(at + 7)?,
                ]) as usize;
                m.get(off..off + len)
            };
            let ok = (|| {
                let nt = field(20)?;
                let domain = field(28)?;
                let user = field(36)?;
                if user != utf16le("lab") || nt.len() < 16 {
                    return None;
                }
                let mut identity = utf16le("LAB");
                identity.extend_from_slice(domain);
                let key = hmac_md5(&md4::Md4::digest(utf16le("secret")), &identity);
                let mut signed = challenge.to_vec();
                signed.extend_from_slice(&nt[16..]);
                (hmac_md5(&key, &signed) == nt[..16]).then_some(())
            })()
            .is_some();
            auth_response(ok, "NTLM".into())
        }
        _ => offer("NTLM".into()),
    }
}

/// Fixed nonce of `/auth/digest` (a lab, not a real server).
const LAB_NONCE: &str = "lab-nonce-1";

fn auth_response(ok: bool, challenge: String) -> Response<Body> {
    if ok {
        return respond(StatusCode::OK, "text/plain", "authenticated as lab");
    }
    let mut res = respond(
        StatusCode::UNAUTHORIZED,
        "text/plain",
        "credentials required",
    );
    if let Ok(v) = HeaderValue::from_str(&challenge) {
        res.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    res
}

fn sha256_hex(s: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, s.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Checks a Digest answer for user `lab`, password `secret` (SHA-256, qop=auth).
fn digest_ok(header: &str, method: &str) -> bool {
    let Some(rest) = header.strip_prefix("Digest ") else {
        return false;
    };
    let mut fields = std::collections::HashMap::new();
    for part in rest.split(", ") {
        if let Some((k, v)) = part.split_once('=') {
            fields.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    let get = |k: &str| fields.get(k).map(String::as_str).unwrap_or("");
    if get("username") != "lab" || get("nonce") != LAB_NONCE || get("qop") != "auth" {
        return false;
    }
    let ha1 = sha256_hex("lab:lab:secret");
    let ha2 = sha256_hex(&format!("{method}:{}", get("uri")));
    let expected = sha256_hex(&format!(
        "{ha1}:{LAB_NONCE}:{}:{}:auth:{ha2}",
        get("nc"),
        get("cnonce")
    ));
    get("response") == expected
}

async fn route(req: Request<Incoming>) -> Result<Response<Body>, Infallible> {
    let path = req.uri().path().to_string();
    if path == "/ws" {
        return Ok(websocket_upgrade(req));
    }
    let res = match (req.method().clone(), path.as_str()) {
        (Method::GET, "/") => respond(StatusCode::OK, "text/html; charset=utf-8", INDEX),
        (Method::GET, "/app.js") => respond(
            StatusCode::OK,
            "application/javascript",
            "fetch('/json').then(r => r.json()).then(d => console.log(d));\n\
             // A sink for the JavaScript analysis to report (never called).\n\
             function labRender(html) { document.getElementById('lab-out').innerHTML = html; }\n\
             //# sourceMappingURL=app.js.map\n",
        ),
        (Method::GET, "/ws-demo") => respond(StatusCode::OK, "text/html; charset=utf-8", WS_DEMO),
        (Method::GET, "/json") => respond(
            StatusCode::OK,
            "application/json",
            r#"{"id":1,"name":"lab","items":[1,2,3],"nested":{"ok":true}}"#,
        ),
        (Method::GET, "/js-links") => respond(
            StatusCode::OK,
            "text/html; charset=utf-8",
            "<!doctype html><title>JS links</title><p>Links made by a script:</p>\
             <script>for (const p of ['/js-only', '/js-only-2']) { const a = document.createElement('a'); \
             a.href = p; a.textContent = p; document.body.appendChild(a); }</script>",
        ),
        (Method::GET, "/js-only" | "/js-only-2") => respond(
            StatusCode::OK,
            "text/html; charset=utf-8",
            "<!doctype html><title>Only reachable through a script</title><a href=\"/json\">json</a>",
        ),
        (Method::POST, "/graphql") => {
            let body = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => Bytes::new(),
            };
            if body.windows(8).any(|w| w == b"__schema") {
                respond(StatusCode::OK, "application/json", LAB_GRAPHQL_SCHEMA)
            } else {
                respond(StatusCode::OK, "application/json", r#"{"data":{}}"#)
            }
        }
        (Method::POST, "/ai/api/chat" | "/ai/v1/chat/completions") => {
            // A stand-in model: it answers with what it was sent (the last
            // message), so tests can see exactly what reached a model.
            let openai = path.ends_with("/completions");
            let body = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => Bytes::new(),
            };
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let last = request["messages"]
                .as_array()
                .and_then(|m| m.last())
                .and_then(|m| m["content"].as_str())
                .unwrap_or("")
                .to_string();
            let answer = format!(
                "echo ({}): {last}",
                request["model"].as_str().unwrap_or("?")
            );
            if request["stream"] == true {
                // Streamed: a few characters per piece, in the server's format.
                // The model "slow" waits between pieces (to test cancelling).
                let slow = request["model"].as_str() == Some("slow");
                let chars: Vec<char> = answer.chars().collect();
                let mut pieces: Vec<Bytes> = chars
                    .chunks(5)
                    .map(|c| {
                        let piece: String = c.iter().collect();
                        Bytes::from(if openai {
                            format!(
                                "data: {}\n\n",
                                serde_json::json!({"choices": [{"delta": {"content": piece}}]})
                            )
                        } else {
                            format!(
                                "{}\n",
                                serde_json::json!({"message": {"role": "assistant", "content": piece}, "done": false})
                            )
                        })
                    })
                    .collect();
                pieces.push(Bytes::from(if openai {
                    "data: [DONE]\n\n".to_string()
                } else {
                    format!(
                        "{}\n",
                        serde_json::json!({"message": {"role": "assistant", "content": ""}, "done": true})
                    )
                }));
                let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(1);
                tokio::spawn(async move {
                    for piece in pieces {
                        if slow {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        if tx.send(piece).await.is_err() {
                            break;
                        }
                    }
                });
                let body = ChannelBody(rx);
                let mut res = Response::new(BodyExt::boxed(body));
                res.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(if openai {
                        "text/event-stream"
                    } else {
                        "application/x-ndjson"
                    }),
                );
                return Ok(res);
            }
            let reply = if openai {
                serde_json::json!({"choices": [{"message": {"role": "assistant", "content": answer}}]})
            } else {
                serde_json::json!({"message": {"role": "assistant", "content": answer}, "done": true})
            };
            respond(StatusCode::OK, "application/json", reply.to_string())
        }
        (Method::POST, "/agent/api/chat" | "/agent/v1/chat/completions") => {
            // A scripted stand-in agent model (M30): it plans and searches,
            // then asks to send a request back to the lab, then calls a tool
            // that does not exist and reads the scope, then answers. The
            // model "text-only" refuses native tools, like small local
            // models, and writes fenced calls instead.
            let openai = path.ends_with("/completions");
            let lab_host = req
                .headers()
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("127.0.0.1")
                .to_string();
            let body = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => Bytes::new(),
            };
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let text_only = request["model"].as_str() == Some("text-only");
            if text_only && request.get("tools").is_some() {
                return Ok(respond(
                    StatusCode::BAD_REQUEST,
                    "application/json",
                    r#"{"error":"registry.ollama.ai/library/text-only does not support tools"}"#,
                ));
            }
            let messages = request["messages"].as_array().cloned().unwrap_or_default();
            let results: Vec<&str> = messages
                .iter()
                .filter(|m| {
                    m["role"] == "tool"
                        || m["content"]
                            .as_str()
                            .is_some_and(|c| c.starts_with("Result of the tool"))
                })
                .filter_map(|m| m["content"].as_str())
                .collect();
            let (host, port) = match lab_host.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(80)),
                None => (lab_host.clone(), 80),
            };
            let calls: Vec<(&str, serde_json::Value)> = match results.len() {
                0 => vec![
                    (
                        "update_plan",
                        serde_json::json!({"steps": [{"title": "Look at the history", "done": false}, {"title": "Check the home page", "done": false}]}),
                    ),
                    (
                        "search_history",
                        serde_json::json!({"search": "/", "limit": 5}),
                    ),
                ],
                1 | 2 => vec![(
                    "send_request",
                    serde_json::json!({
                        "scheme": "http",
                        "host": host,
                        "port": port,
                        "request": format!("GET /?agent=1 HTTP/1.1\r\nHost: {lab_host}\r\n\r\n"),
                        "reason": "See what the home page returns."
                    }),
                )],
                3 => vec![
                    ("run_shell", serde_json::json!({"command": "id"})),
                    ("get_scope", serde_json::json!({})),
                ],
                _ => Vec::new(),
            };
            let answer = if calls.is_empty() {
                format!(
                    "Done after {} tool results. Last result: {}",
                    results.len(),
                    results
                        .last()
                        .map(|r| r.chars().take(300).collect::<String>())
                        .unwrap_or_default()
                )
            } else if text_only {
                calls
                    .iter()
                    .map(|(name, args)| {
                        format!(
                            "```tool\n{}\n```",
                            serde_json::json!({"name": name, "arguments": args})
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            let native: Vec<serde_json::Value> = if text_only {
                Vec::new()
            } else {
                calls
                    .iter()
                    .enumerate()
                    .map(|(i, (name, args))| {
                        if openai {
                            serde_json::json!({"id": format!("lab{}_{i}", results.len()), "type": "function", "function": {"name": name, "arguments": args.to_string()}})
                        } else {
                            serde_json::json!({"function": {"name": name, "arguments": args}})
                        }
                    })
                    .collect()
            };
            let mut message = serde_json::json!({"role": "assistant", "content": answer});
            if !native.is_empty() {
                message["tool_calls"] = serde_json::Value::Array(native);
            }
            let reply = if openai {
                serde_json::json!({"choices": [{"message": message}]})
            } else {
                serde_json::json!({"message": message, "done": true})
            };
            respond(StatusCode::OK, "application/json", reply.to_string())
        }
        (Method::POST, "/session/login") => {
            let body = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => Bytes::new(),
            };
            if body.as_ref() == b"user=lab&pass=secret" {
                let mut res = respond(StatusCode::OK, "text/plain", "logged in");
                res.headers_mut().insert(
                    header::SET_COOKIE,
                    HeaderValue::from_static("labsession=ok-1; Path=/; HttpOnly"),
                );
                res
            } else {
                respond(
                    StatusCode::UNAUTHORIZED,
                    "text/plain",
                    "wrong user or password",
                )
            }
        }
        (Method::GET, "/session/token") => {
            if has_lab_session(&req) {
                respond(
                    StatusCode::OK,
                    "text/html",
                    r#"<form><input type="hidden" name="csrf" value="csrf-123"></form>"#,
                )
            } else {
                respond(StatusCode::UNAUTHORIZED, "text/plain", "please log in")
            }
        }
        (Method::GET, "/session/api") => {
            let csrf_ok =
                req.headers().get("x-csrf").and_then(|v| v.to_str().ok()) == Some("csrf-123");
            if !has_lab_session(&req) {
                respond(StatusCode::UNAUTHORIZED, "text/plain", "please log in")
            } else if !csrf_ok {
                respond(StatusCode::FORBIDDEN, "text/plain", "missing CSRF token")
            } else {
                respond(StatusCode::OK, "application/json", r#"{"secret":"area"}"#)
            }
        }
        (Method::GET, "/auth/basic") => {
            let ok = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                == Some("Basic bGFiOnNlY3JldA==");
            auth_response(ok, "Basic realm=\"lab\"".to_string())
        }
        (Method::GET, "/auth/ntlm") => ntlm_route(
            req.extensions().get::<ConnId>().copied(),
            req.headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
        ),
        (Method::GET, "/auth/digest") => {
            let ok = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| digest_ok(v, req.method().as_str()));
            auth_response(
                ok,
                format!(
                    "Digest realm=\"lab\", qop=\"auth\", algorithm=SHA-256, nonce=\"{LAB_NONCE}\", opaque=\"o\""
                ),
            )
        }
        (_, "/version") => {
            let version = match req.version() {
                hyper::Version::HTTP_2 => "HTTP/2",
                hyper::Version::HTTP_10 => "HTTP/1.0",
                _ => "HTTP/1.1",
            };
            let method = req.method().to_string();
            let body = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => Bytes::new(),
            };
            respond(
                StatusCode::OK,
                "application/json",
                serde_json::json!({
                    "version": version,
                    "method": method,
                    "body": String::from_utf8_lossy(&body),
                })
                .to_string(),
            )
        }
        (Method::GET, "/headers") => respond(
            StatusCode::OK,
            "application/json",
            headers_json(&req).to_string(),
        ),
        (Method::GET, "/token") | (Method::GET, "/token/sequential") => {
            let value = if path == "/token" {
                let mut bytes = [0u8; 16];
                // A lab: a failure leaves zeros, which the sequencer flags.
                let _ =
                    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes);
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            } else {
                static COUNTER: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(100_000);
                COUNTER
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string()
            };
            let mut res = respond(StatusCode::OK, "text/plain", "token issued\n");
            if let Ok(cookie) = HeaderValue::from_str(&format!("token={value}; Path=/; HttpOnly")) {
                res.headers_mut().append(header::SET_COOKIE, cookie);
            }
            res
        }
        (Method::GET, "/dom/" | "/dom") => respond(StatusCode::OK, HTML, dom_pages::INDEX),
        (Method::GET, "/dom/hash") => respond(StatusCode::OK, HTML, dom_pages::HASH),
        (Method::GET, "/dom/search") => respond(StatusCode::OK, HTML, dom_pages::SEARCH),
        (Method::GET, "/dom/redirect") => respond(StatusCode::OK, HTML, dom_pages::REDIRECT),
        (Method::GET, "/dom/message") => respond(StatusCode::OK, HTML, dom_pages::MESSAGE),
        (Method::GET, "/dom/pollution") => respond(StatusCode::OK, HTML, dom_pages::POLLUTION),
        (Method::GET, "/dom/clobber") => respond(StatusCode::OK, HTML, dom_pages::CLOBBER),
        (Method::GET, "/debug-token") => {
            // For the HelloPlugin example: a response with a debug token header.
            let mut res = respond(StatusCode::OK, "text/plain", "debug profiler enabled\n");
            res.headers_mut()
                .insert("x-debug-token", HeaderValue::from_static("a1b2c3"));
            res
        }
        (Method::GET, "/cookies/set") => {
            let mut res = respond(StatusCode::OK, "text/plain", "cookies set\n");
            for c in [
                "session=lab-session-value; Path=/; HttpOnly; SameSite=Lax",
                "theme=dark; Path=/",
            ] {
                res.headers_mut()
                    .append(header::SET_COOKIE, HeaderValue::from_static(c));
            }
            res
        }
        (Method::GET, "/redirect") => {
            let to = query_param(&req, "to").unwrap_or_else(|| "/".into());
            let mut res = respond(StatusCode::FOUND, "text/plain", "redirecting\n");
            if let Ok(v) = HeaderValue::from_str(&to) {
                res.headers_mut().insert(header::LOCATION, v);
            }
            res
        }
        (Method::GET, "/chunked") => {
            let pieces = ["first,", "second,", "third"].into_iter().map(Bytes::from);
            let mut res = Response::new(ChunkedBody(pieces).boxed());
            res.headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
            res
        }
        (Method::GET, "/gzip") => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            let body = r#"{"compressed":true,"message":"hello from gzip"}"#;
            let gz = enc
                .write_all(body.as_bytes())
                .and_then(|()| enc.finish())
                .unwrap_or_default();
            let mut res = respond(StatusCode::OK, "application/json", gz);
            res.headers_mut()
                .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            res
        }
        (Method::GET, "/large") => {
            let n: usize = query_param(&req, "bytes")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100_000)
                .min(64 * 1024 * 1024);
            respond(StatusCode::OK, "application/octet-stream", vec![b'L'; n])
        }
        (Method::GET, "/slow") => {
            let ms: u64 = query_param(&req, "ms")
                .and_then(|v| v.parse().ok())
                .unwrap_or(500)
                .min(30_000);
            tokio::time::sleep(Duration::from_millis(ms)).await;
            respond(StatusCode::OK, "text/plain", "slow response\n")
        }
        (Method::POST, "/echo") | (Method::PUT, "/echo") | (Method::POST, "/form") => {
            let method = req.method().to_string();
            let headers = headers_json(&req);
            let body = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => Bytes::new(),
            };
            let json = serde_json::json!({
                "method": method,
                "headers": headers,
                "body": String::from_utf8_lossy(&body),
                "length": body.len(),
            });
            respond(StatusCode::OK, "application/json", json.to_string())
        }
        // Content discovery targets: unlinked content, and an area that
        // answers "not found" with status 200 (soft 404).
        (Method::GET, "/admin/") => respond(
            StatusCode::OK,
            HTML,
            "<!doctype html><title>Lab admin</title><h1>Lab administration (demo)</h1>",
        ),
        (Method::GET, "/admin") => {
            let mut res = respond(StatusCode::MOVED_PERMANENTLY, "text/plain", "moved\n");
            res.headers_mut()
                .insert(header::LOCATION, HeaderValue::from_static("/admin/"));
            res
        }
        (Method::GET, "/backup.zip") => {
            respond(StatusCode::OK, "application/zip", "PK\x03\x04 demo")
        }
        (Method::GET, "/soft/panel/") => respond(
            StatusCode::OK,
            HTML,
            "<!doctype html><title>Panel</title><h1>Control panel</h1><p>Welcome back.</p>",
        ),
        (Method::GET, p) if p.starts_with("/soft/") => respond(
            StatusCode::OK,
            HTML,
            format!("<!doctype html><title>Oops</title><p>The page {p} could not be found.</p>"),
        ),
        // A server asking clients to slow down (resource pool back-off).
        (Method::GET, "/busy") => {
            let secs: u64 = query_param(&req, "s")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1)
                .min(120);
            let mut res = respond(StatusCode::TOO_MANY_REQUESTS, "text/plain", "slow down\n");
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
            res
        }
        (Method::GET, p) if p.starts_with("/status/") => {
            let code = p["/status/".len()..]
                .parse::<u16>()
                .ok()
                .and_then(|c| StatusCode::from_u16(c).ok())
                .unwrap_or(StatusCode::BAD_REQUEST);
            respond(code, "text/plain", format!("status {}\n", code.as_u16()))
        }
        // Deterministic active-audit lab endpoints (M13).
        (Method::GET, "/vuln/xss") => {
            let q = query_param_decoded(&req, "q").unwrap_or_default();
            respond(
                StatusCode::OK,
                HTML,
                format!("<!doctype html><title>Search</title><p>Results for: {q}</p>"),
            )
        }
        (Method::GET, "/safe/xss") => {
            let q = html_escape(&query_param_decoded(&req, "q").unwrap_or_default());
            respond(
                StatusCode::OK,
                HTML,
                format!("<!doctype html><title>Search</title><p>Results for: {q}</p>"),
            )
        }
        (Method::GET, "/safe/xss-json") => {
            let q = query_param_decoded(&req, "q").unwrap_or_default();
            respond(
                StatusCode::OK,
                "application/json",
                serde_json::json!({ "query": q, "results": [] }).to_string(),
            )
        }
        (Method::GET, "/vuln/sqli-error") => {
            let id = query_param_decoded(&req, "id").unwrap_or_default();
            if id.contains('\'') {
                respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "text/plain",
                    "Database error: SQLITE_ERROR: near \"'\": syntax error\n",
                )
            } else {
                respond(
                    StatusCode::OK,
                    "application/json",
                    r#"[{"id":1,"name":"Widget"}]"#,
                )
            }
        }
        (Method::GET, "/vuln/sqli-bool") => {
            let id = query_param_decoded(&req, "id").unwrap_or_default();
            if id.contains("AND 1=2")
                || id.contains("AND '1'='2")
                || id.ends_with('\'')
                || id.ends_with("\"\"")
            {
                respond(StatusCode::OK, "application/json", "[]")
            } else if id == "1" || id == "abc" || id.contains("OR 1=1") || id.contains("OR '1'='1")
            {
                respond(
                    StatusCode::OK,
                    "application/json",
                    r#"[{"id":1,"name":"Widget","category":"tools"},{"id":2,"name":"Gadget","category":"tools"}]"#,
                )
            } else {
                respond(StatusCode::OK, "application/json", "[]")
            }
        }
        (Method::GET, "/safe/sqli") => respond(
            StatusCode::OK,
            "application/json",
            r#"[{"id":1,"name":"Widget"}]"#,
        ),
        (Method::GET, "/safe/error500") => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain",
            "Internal Server Error: upstream service temporarily unavailable\n",
        ),
        (Method::GET, "/safe/dynamic") => {
            let seq = DYNAMIC_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let filler = "x".repeat((seq as usize * 37) % 120);
            respond(
                StatusCode::OK,
                HTML,
                format!("<!doctype html><title>Dynamic</title><p>tick={seq} {filler}</p>"),
            )
        }
        (Method::GET, "/vuln/redirect") => {
            let next = query_param_decoded(&req, "next").unwrap_or_else(|| "/".into());
            let mut res = respond(StatusCode::FOUND, "text/plain", "redirecting\n");
            if let Ok(v) = HeaderValue::from_str(&next) {
                res.headers_mut().insert(header::LOCATION, v);
            }
            res
        }
        (Method::GET, "/safe/redirect") => {
            let next = query_param_decoded(&req, "next").unwrap_or_else(|| "/".into());
            let safe_target = if next.starts_with('/') && !next.starts_with("//") {
                next.as_str()
            } else {
                "/home"
            };
            let mut res = respond(StatusCode::FOUND, "text/plain", "redirecting\n");
            if let Ok(v) = HeaderValue::from_str(safe_target) {
                res.headers_mut().insert(header::LOCATION, v);
            }
            res
        }
        (Method::GET, "/vuln/cors") => {
            let origin = req
                .headers()
                .get(header::ORIGIN)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let mut res = respond(StatusCode::OK, "application/json", r#"{"secret":"ok"}"#);
            if let Some(o) = origin
                && let Ok(v) = HeaderValue::from_str(&o)
            {
                res.headers_mut()
                    .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
                res.headers_mut().insert(
                    header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                    HeaderValue::from_static("true"),
                );
            }
            res
        }
        (Method::GET, "/safe/cors") => {
            let mut res = respond(StatusCode::OK, "application/json", r#"{"public":"ok"}"#);
            res.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("https://trusted.example"),
            );
            res
        }
        (Method::GET, "/vuln/oast") => {
            let cb = query_param_decoded(&req, "callback").unwrap_or_default();
            if let Some(rest) = cb
                .strip_prefix("http://127.0.0.1:")
                .or_else(|| cb.strip_prefix("http://localhost:"))
            {
                let (port_str, path_part) = match rest.split_once('/') {
                    Some((p, tail)) => (p, format!("/{tail}")),
                    None => (rest, "/".to_string()),
                };
                if let Ok(port) = port_str.parse::<u16>()
                    && let Ok(Ok(mut stream)) = tokio::time::timeout(
                        Duration::from_secs(2),
                        tokio::net::TcpStream::connect(("127.0.0.1", port)),
                    )
                    .await
                {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let raw_req = format!(
                        "GET {path_part} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(raw_req.as_bytes()).await;
                    let mut buf = [0u8; 256];
                    let _ = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf))
                        .await;
                }
            }
            respond(StatusCode::OK, "text/plain", "fetched\n")
        }
        (Method::GET, "/protected/xss") => {
            let authed = has_lab_session(&req)
                || req
                    .headers()
                    .get("x-session-token")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v == "valid-token");
            if !authed {
                respond(StatusCode::UNAUTHORIZED, "text/plain", "session required\n")
            } else {
                let q = query_param_decoded(&req, "q").unwrap_or_default();
                respond(
                    StatusCode::OK,
                    HTML,
                    format!("<!doctype html><title>Protected</title><p>Welcome: {q}</p>"),
                )
            }
        }
        (Method::GET, "/api/items") => {
            let filter = query_param_decoded(&req, "filter").unwrap_or_default();
            if filter.contains('\'') {
                respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "text/plain",
                    "Database error: SQLITE_ERROR: near \"'\": syntax error\n",
                )
            } else {
                respond(
                    StatusCode::OK,
                    "application/json",
                    r#"[{"id":1,"name":"Item"}]"#,
                )
            }
        }
        _ => respond(StatusCode::NOT_FOUND, "text/plain", "not found\n"),
    };
    Ok(res)
}

/// A body fed through a channel, piece by piece (chunked on the wire).
#[derive(Debug)]
struct ChannelBody(tokio::sync::mpsc::Receiver<Bytes>);

impl hyper::body::Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.0.poll_recv(cx).map(|b| b.map(|b| Ok(Frame::data(b))))
    }
}

/// A body that yields frames from an iterator. Its length is unknown up
/// front, so hyper sends it with chunked transfer coding.
#[derive(Debug)]
struct ChunkedBody<I>(I);

impl<I> hyper::body::Body for ChunkedBody<I>
where
    I: Iterator<Item = Bytes> + Unpin,
{
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        std::task::Poll::Ready(self.0.next().map(|b| Ok(Frame::data(b))))
    }
}

// ---------------------------------------------------------------------------
// WebSocket echo (`/ws`), written independently of ChosProxy's codec.

fn ws_accept(key: &str) -> String {
    use base64::Engine as _;
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
    );
    base64::engine::general_purpose::STANDARD.encode(digest.as_ref())
}

/// Answers the handshake and echoes messages: it greets with `welcome`,
/// replies `echo: <text>` to text, mirrors binary data, answers pings and
/// close frames. The `X-Lab-Extensions-Offered` response header tells whether
/// the client offered extensions (none are ever accepted).
fn websocket_upgrade(mut req: Request<Incoming>) -> Response<Body> {
    let key = req
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let is_ws = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let (Some(key), true) = (key, is_ws) else {
        return respond(
            StatusCode::BAD_REQUEST,
            "text/plain",
            "expected a WebSocket handshake\n",
        );
    };
    let offered = req.headers().contains_key("sec-websocket-extensions");
    let on_upgrade = hyper::upgrade::on(&mut req);
    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => ws_echo(TokioIo::new(upgraded)).await,
            Err(e) => tracing::debug!(error = %e, "lab websocket upgrade failed"),
        }
    });
    let mut res = Response::new(Full::new(Bytes::new()).boxed());
    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let h = res.headers_mut();
    h.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    h.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    if let Ok(v) = HeaderValue::from_str(&ws_accept(key.trim())) {
        h.insert("sec-websocket-accept", v);
    }
    h.insert(
        "x-lab-extensions-offered",
        HeaderValue::from_static(if offered { "yes" } else { "no" }),
    );
    res
}

fn ws_server_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode];
    if payload.len() < 126 {
        out.push(payload.len() as u8);
    } else if payload.len() <= 0xffff {
        out.push(126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

async fn ws_echo<S>(mut io: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    if io.write_all(&ws_server_frame(1, b"welcome")).await.is_err() {
        return;
    }
    let mut message = Vec::new();
    let mut message_opcode = 1;
    loop {
        let mut h = [0u8; 2];
        if io.read_exact(&mut h).await.is_err() {
            return;
        }
        let (fin, opcode, masked) = (h[0] & 0x80 != 0, h[0] & 0x0f, h[1] & 0x80 != 0);
        let len = match h[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                if io.read_exact(&mut b).await.is_err() {
                    return;
                }
                usize::from(u16::from_be_bytes(b))
            }
            127 => {
                let mut b = [0u8; 8];
                if io.read_exact(&mut b).await.is_err() {
                    return;
                }
                match usize::try_from(u64::from_be_bytes(b)) {
                    Ok(n) if n <= 16 * 1024 * 1024 => n,
                    _ => return,
                }
            }
            n => usize::from(n),
        };
        let mut mask = [0u8; 4];
        if masked && io.read_exact(&mut mask).await.is_err() {
            return;
        }
        let mut payload = vec![0u8; len];
        if io.read_exact(&mut payload).await.is_err() {
            return;
        }
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        let reply = match opcode {
            8 => {
                let _ = io.write_all(&ws_server_frame(8, &payload)).await;
                let _ = io.shutdown().await;
                return;
            }
            9 => Some(ws_server_frame(10, &payload)),
            10 => None,
            0..=2 => {
                if opcode != 0 {
                    message_opcode = opcode;
                    message.clear();
                }
                message.extend_from_slice(&payload);
                if !fin {
                    continue;
                }
                let body = std::mem::take(&mut message);
                Some(if message_opcode == 1 {
                    let mut text = b"echo: ".to_vec();
                    text.extend_from_slice(&body);
                    ws_server_frame(1, &text)
                } else {
                    ws_server_frame(2, &body)
                })
            }
            _ => None,
        };
        if let Some(r) = reply
            && io.write_all(&r).await.is_err()
        {
            return;
        }
    }
}
