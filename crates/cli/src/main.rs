//! `chosproxy-cli` (M28, matrix PLATFORM-004): run ChosProxy headless and
//! drive it through the automation API.
//!
//! `chosproxy-cli serve` starts the engine, a project and the API on
//! loopback (and the proxy listeners set to start with a project), without
//! a window. The other commands are clients of the API, of `serve` or of a
//! desktop app with the API turned on.

use chosproxy_api::{ApiServer, token};
use chosproxy_app::Engine;
use chosproxy_core::config::{ApiConfig, ConfigStore};
use chosproxy_core::paths::AppPaths;
use serde_json::{Value, json};
use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const USAGE: &str = "\
chosproxy-cli — ChosProxy without a window, and a client for its automation API.

Headless:
  chosproxy-cli serve [--project FILE | --new FILE] [--port N]
                      [--password-file FILE|-]
      Starts the engine, a project (temporary unless --project or --new),
      the proxy listeners that start with a project, and the API on
      127.0.0.1. Runs until Ctrl-C. An encrypted project's password comes
      from CHOSPROXY_PROJECT_PASSWORD or the first line of --password-file
      (`-` reads standard input); with --new it creates the project
      encrypted. Never put a password on the command line.

Client (talks to `serve` or to the desktop app with the API on):
  status                          engine, project and listeners
  history [--limit N] [--search TEXT]
  show ID                         one exchange
  findings
  scope                           show the scope
  scope add URL | scope exclude URL
  send URL [--file REQUEST.txt]   one request (GET by default), like Repeater
  crawl URL...                    start a crawl
  scan URL [--config NAME]        start a scan (default: the light one)
  tasks | task ID | cancel ID
  wait ID [--timeout SECONDS]     until the task ends (exit 1 if it failed)
  report [--format markdown|html|json] [--output FILE]
  import-api FILE [--base-url URL]
  events                          engine events, one JSON object per line,
                                  until Ctrl-C or the engine stops
  openapi                         the API description (OpenAPI 3.1)

Options for every client command:
  --port N        API port (default: CHOSPROXY_API_PORT, api.toml, 8790)
  --token TOKEN   API token (default: CHOSPROXY_API_TOKEN, then the token
                  file in the data directory)
";

fn out(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

fn err(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// Command-line arguments: positional words and `--name value` options.
struct Args {
    words: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn parse(raw: impl Iterator<Item = String>) -> Result<Args, String> {
        let mut words = Vec::new();
        let mut options = Vec::new();
        let mut raw = raw.peekable();
        while let Some(a) = raw.next() {
            if let Some(name) = a.strip_prefix("--") {
                let value = raw
                    .next()
                    .ok_or_else(|| format!("--{name} needs a value"))?;
                options.push((name.to_string(), value));
            } else if a == "-o" {
                let value = raw.next().ok_or("-o needs a file")?;
                options.push(("output".into(), value));
            } else {
                words.push(a);
            }
        }
        Ok(Args { words, options })
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            err(&e);
            return ExitCode::from(2);
        }
    };
    let Some(command) = args.words.first().cloned() else {
        out(USAGE);
        return ExitCode::SUCCESS;
    };
    let result = match command.as_str() {
        "help" | "-h" => {
            out(USAGE);
            Ok(())
        }
        "serve" => serve(&args).await,
        _ => client(&command, &args).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            err(&format!("error: {e}"));
            ExitCode::FAILURE
        }
    }
}

fn paths() -> Result<AppPaths, String> {
    AppPaths::resolve()
        .ok_or_else(|| "cannot find the configuration directories (is HOME set?)".into())
}

/// An encrypted project's password: `CHOSPROXY_PROJECT_PASSWORD`, or the
/// first line of `--password-file` (`-` = standard input).
fn project_password(args: &Args) -> Result<Option<String>, String> {
    if let Some(file) = args.opt("password-file") {
        let text = if file == "-" {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("cannot read the password: {e}"))?;
            line
        } else {
            std::fs::read_to_string(file).map_err(|e| format!("cannot read {file}: {e}"))?
        };
        let first = text.lines().next().unwrap_or_default().to_string();
        return Ok(Some(first).filter(|p| !p.is_empty()));
    }
    Ok(std::env::var("CHOSPROXY_PROJECT_PASSWORD")
        .ok()
        .filter(|p| !p.is_empty()))
}

async fn serve(args: &Args) -> Result<(), String> {
    let paths = paths()?;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();
    let engine = Engine::new(paths.clone()).map_err(|e| e.to_string())?;
    let password = project_password(args)?;
    let project = match (args.opt("project"), args.opt("new")) {
        (Some(p), _) => {
            engine
                .open_project_with(std::path::Path::new(p), password.as_deref())
                .await
        }
        (None, Some(p)) => {
            engine
                .create_project_with(std::path::Path::new(p), password.as_deref())
                .await
        }
        (None, None) => engine.new_temporary_project().await,
    }
    .map_err(|e| e.to_string())?;
    let token = token::load_or_create(&paths.data_dir).map_err(|e| e.to_string())?;
    let port = match args.opt("port") {
        Some(p) => p.parse().map_err(|_| format!("{p:?} is not a port"))?,
        None => engine.api_config().port,
    };
    let server = ApiServer::start(engine.clone(), port, token)
        .await
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    out(&format!("API listening on http://{}", server.addr()));
    out(&format!("Project: {} ({})", project.name, project.path));
    for l in engine.listeners().await {
        out(&format!(
            "Proxy listener {} on {}:{} ({:?})",
            l.config.id, l.config.bind_address, l.config.port, l.state
        ));
    }
    out(&format!(
        "Token file: {}",
        token::token_path(&paths.data_dir).display()
    ));
    let _ = std::io::stdout().flush();
    tokio::signal::ctrl_c().await.map_err(|e| e.to_string())?;
    out("Stopping.");
    server.stop().await;
    engine.close_project().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Where the API is and how to authenticate.
struct Client {
    port: u16,
    token: String,
}

impl Client {
    fn from(args: &Args) -> Result<Client, String> {
        let paths = paths()?;
        let port = match args
            .opt("port")
            .map(str::to_string)
            .or_else(|| std::env::var("CHOSPROXY_API_PORT").ok())
        {
            Some(p) => p.parse().map_err(|_| format!("{p:?} is not a port"))?,
            None => {
                ConfigStore::new(paths.config_dir.clone())
                    .load::<ApiConfig>()
                    .value
                    .port
            }
        };
        let token = args
            .opt("token")
            .map(str::to_string)
            .or_else(|| std::env::var("CHOSPROXY_API_TOKEN").ok())
            .or_else(|| token::read(&paths.data_dir))
            .ok_or("no API token: start `chosproxy-cli serve`, turn the API on in the app, or pass --token")?;
        Ok(Client { port, token })
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
            .await
            .map_err(|e| format!("cannot reach the API on 127.0.0.1:{}: {e}", self.port))?;
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.port,
            self.token,
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .map_err(|e| e.to_string())?;
        let raw = String::from_utf8_lossy(&raw);
        let status: u16 = raw
            .get(9..12)
            .and_then(|s| s.parse().ok())
            .ok_or("bad answer from the API")?;
        let text = raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        if status == 200 {
            Ok(value)
        } else {
            Err(value["error"]
                .as_str()
                .map_or_else(|| format!("HTTP {status}"), str::to_string))
        }
    }
}

impl Client {
    /// Follows `GET /v1/events` and prints each event as one JSON line.
    async fn events(&self) -> Result<(), String> {
        use tokio::io::AsyncBufReadExt;
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
            .await
            .map_err(|e| format!("cannot reach the API on 127.0.0.1:{}: {e}", self.port))?;
        let request = format!(
            "GET /v1/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\
             Accept: text/event-stream\r\n\r\n",
            self.port, self.token
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut lines = tokio::io::BufReader::new(stream).lines();
        let status = lines
            .next_line()
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        if status.get(9..12) != Some("200") {
            // The error body follows the head.
            let mut rest = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                rest.push_str(&line);
            }
            let message = rest
                .find('{')
                .and_then(|i| serde_json::from_str::<Value>(&rest[i..]).ok())
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| format!("the API answered: {status}"));
            return Err(message);
        }
        // Chunk sizes, comments and `event:` lines are skipped: the data
        // line carries the whole event.
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            if let Some(data) = line.strip_prefix("data: ") {
                out(data);
            }
        }
        Ok(())
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn need<'a>(args: &'a Args, index: usize, what: &str) -> Result<&'a str, String> {
    args.words
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {what} (see `chosproxy-cli help`)"))
}

async fn client(command: &str, args: &Args) -> Result<(), String> {
    let c = Client::from(args)?;
    match command {
        "status" => out(&pretty(&c.call("GET", "/v1/status", None).await?)),
        "history" => {
            let mut path = format!("/v1/history?limit={}", args.opt("limit").unwrap_or("50"));
            if let Some(s) = args.opt("search") {
                path.push_str("&search=");
                path.push_str(&encode(s));
            }
            let page = c.call("GET", &path, None).await?;
            for r in page["rows"].as_array().into_iter().flatten() {
                out(&format!(
                    "{:>6}  {:<7} {:>3}  {}",
                    r["id"],
                    r["method"].as_str().unwrap_or(""),
                    r["status"].as_u64().map_or("—".into(), |s| s.to_string()),
                    r["url"].as_str().unwrap_or("")
                ));
            }
            out(&format!("{} in all", page["total"]));
        }
        "show" => {
            let id = need(args, 1, "an exchange id")?;
            out(&pretty(
                &c.call("GET", &format!("/v1/history/{id}"), None).await?,
            ));
        }
        "findings" => {
            let list = c.call("GET", "/v1/findings", None).await?;
            for f in list.as_array().into_iter().flatten() {
                out(&format!(
                    "{:>5}  {:<8} {:<10} {}  {}",
                    f["id"],
                    f["severity"].as_str().unwrap_or(""),
                    f["status"].as_str().unwrap_or(""),
                    f["title"].as_str().unwrap_or(""),
                    f["url"].as_str().unwrap_or("")
                ));
            }
        }
        "scope" => match args.words.get(1).map(String::as_str) {
            None => out(&pretty(&c.call("GET", "/v1/scope", None).await?)),
            Some(action @ ("add" | "exclude")) => {
                let url = need(args, 2, "a URL")?;
                let body = json!({ "url": url, "exclude": action == "exclude", "path": true });
                out(&pretty(&c.call("POST", "/v1/scope/add", Some(body)).await?));
            }
            Some(other) => return Err(format!("unknown scope action {other:?}")),
        },
        "send" => {
            let url = url_target(need(args, 1, "a URL")?)?;
            let request = match args.opt("file") {
                Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
                None => format!("GET {} HTTP/1.1\nHost: {}\n\n", url.path, url.host_header),
            };
            let body = json!({ "target": url.target, "request": request });
            out(&pretty(&c.call("POST", "/v1/send", Some(body)).await?));
        }
        "crawl" => {
            let urls: Vec<&String> = args.words.iter().skip(1).collect();
            if urls.is_empty() {
                return Err("give at least one start URL".into());
            }
            let task = c
                .call("POST", "/v1/crawl", Some(json!({ "startUrls": urls })))
                .await?;
            out(&format!(
                "Task {} started: {}",
                task["id"],
                task["name"].as_str().unwrap_or("")
            ));
        }
        "scan" => {
            let url = need(args, 1, "a start URL")?;
            let wanted = args.opt("config").unwrap_or("Crawl and audit (light)");
            let configs = c.call("GET", "/v1/scan/configs", None).await?;
            let config = configs
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| {
                    c["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
                })
                .cloned()
                .ok_or_else(|| format!("no scan configuration named {wanted:?}"))?;
            let task = c
                .call(
                    "POST",
                    "/v1/scan",
                    Some(json!({ "startUrls": [url], "config": config })),
                )
                .await?;
            out(&format!(
                "Task {} started: {}",
                task["id"],
                task["name"].as_str().unwrap_or("")
            ));
        }
        "events" => c.events().await?,
        "openapi" => out(&pretty(&c.call("GET", "/v1/openapi.json", None).await?)),
        "tasks" => {
            let list = c.call("GET", "/v1/tasks", None).await?;
            for t in list.as_array().into_iter().flatten() {
                out(&format!(
                    "{:>5}  {:<10} {:<10} {:>6} req  {}",
                    t["id"],
                    t["kind"].as_str().unwrap_or(""),
                    t["status"].as_str().unwrap_or(""),
                    t["requests"],
                    t["name"].as_str().unwrap_or("")
                ));
            }
        }
        "task" => {
            let id = need(args, 1, "a task id")?;
            out(&pretty(
                &c.call("GET", &format!("/v1/tasks/{id}"), None).await?,
            ));
        }
        "cancel" => {
            let id = need(args, 1, "a task id")?;
            out(&pretty(
                &c.call("POST", &format!("/v1/tasks/{id}/cancel"), None)
                    .await?,
            ));
        }
        "wait" => {
            let id = need(args, 1, "a task id")?;
            let limit = args
                .opt("timeout")
                .and_then(|t| t.parse().ok())
                .unwrap_or(3600u64);
            let start = Instant::now();
            loop {
                let detail = c.call("GET", &format!("/v1/tasks/{id}"), None).await?;
                let status = detail["row"]["status"].as_str().unwrap_or("").to_string();
                if !matches!(status.as_str(), "queued" | "running" | "paused") {
                    for line in detail["logs"].as_array().into_iter().flatten() {
                        out(line["message"].as_str().unwrap_or(""));
                    }
                    out(&format!("Task {id}: {status}"));
                    return if status == "failed" {
                        Err(detail["row"]["lastError"]
                            .as_str()
                            .unwrap_or("the task failed")
                            .to_string())
                    } else {
                        Ok(())
                    };
                }
                if start.elapsed() > Duration::from_secs(limit) {
                    return Err(format!("task {id} still {status} after {limit} s"));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        "report" => {
            let format = args.opt("format").unwrap_or("markdown");
            let report = c
                .call("POST", "/v1/report", Some(json!({ "format": format })))
                .await?;
            let content = report["content"].as_str().unwrap_or("");
            match args.opt("output") {
                Some(f) => {
                    std::fs::write(f, content).map_err(|e| format!("{f}: {e}"))?;
                    out(&format!(
                        "Report with {} finding(s) written to {f}",
                        report["findings"]
                    ));
                }
                None => out(content),
            }
        }
        "import-api" => {
            let file = need(args, 1, "a definition file")?;
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            let name = std::path::Path::new(file)
                .file_name()
                .map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
            let body = json!({ "name": name, "text": text, "baseUrl": args.opt("base-url") });
            out(&pretty(
                &c.call("POST", "/v1/api-definitions", Some(body)).await?,
            ));
        }
        other => {
            return Err(format!(
                "unknown command {other:?} (see `chosproxy-cli help`)"
            ));
        }
    }
    Ok(())
}

/// A URL split for `send`: the target, the path with query, the Host.
struct UrlTarget {
    target: Value,
    path: String,
    host_header: String,
}

fn url_target(text: &str) -> Result<UrlTarget, String> {
    let (scheme, rest) = text
        .split_once("://")
        .ok_or_else(|| format!("{text:?} is not an http(s) URL"))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("{text:?} is not an http(s) URL"));
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || authority.starts_with('[') => (
            h.trim_matches(['[', ']']).to_string(),
            p.parse().map_err(|_| format!("bad port in {text:?}"))?,
        ),
        _ => (
            authority.trim_matches(['[', ']']).to_string(),
            if scheme == "https" { 443 } else { 80 },
        ),
    };
    if host.is_empty() {
        return Err(format!("no host in {text:?}"));
    }
    Ok(UrlTarget {
        target: json!({ "scheme": scheme, "host": host, "port": port }),
        path: path.split('#').next().unwrap_or("/").to_string(),
        host_header: authority.to_string(),
    })
}

/// Percent-encodes a query value.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn parses_arguments_and_urls() {
        let a = Args::parse(
            ["history", "--limit", "5", "x", "-o", "f.md"]
                .iter()
                .map(|s| (*s).to_string()),
        )
        .unwrap();
        assert_eq!(a.words, ["history", "x"]);
        assert_eq!(a.opt("limit"), Some("5"));
        assert_eq!(a.opt("output"), Some("f.md"));
        assert!(Args::parse(["--port"].iter().map(|s| (*s).to_string())).is_err());

        let u = url_target("http://127.0.0.1:9090/a?b=1#frag").unwrap();
        assert_eq!(
            u.target,
            json!({"scheme": "http", "host": "127.0.0.1", "port": 9090})
        );
        assert_eq!(
            (u.path.as_str(), u.host_header.as_str()),
            ("/a?b=1", "127.0.0.1:9090")
        );
        let u = url_target("https://lab.test").unwrap();
        assert_eq!(u.target["port"], 443);
        assert_eq!(u.path, "/");
        assert!(url_target("ftp://x/").is_err());
        assert!(url_target("nothing").is_err());
        assert_eq!(encode("a b&c"), "a%20b%26c");
    }
}
