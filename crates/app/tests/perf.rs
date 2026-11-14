//! Performance measurements on a large project. Ignored by default; run with
//! `cargo test --release -p chosproxy-app --test perf -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::print_stdout)]

use chosproxy_app::{Engine, SearchQuery};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_core::scope::ScopeRule;
use chosproxy_storage::{HistoryQuery, NewExchange, SiteFilter};
use std::time::Instant;

const N: u32 = 100_000;

fn exchange(i: u32) -> NewExchange {
    let host = format!("app{}.lab.test", i % 20);
    let path = match i % 7 {
        0 => format!("/static/img/{}.png", i % 500),
        1 => format!("/api/v1/users/{}?expand=profile", i % 3000),
        2 => format!("/api/v1/orders/{}/items", i % 5000),
        3 => "/".to_string(),
        4 => format!("/search?q=term{}", i % 1000),
        5 => format!("/assets/app.{}.js", i % 50),
        _ => format!("/account/settings/{}", i % 200),
    };
    let body = format!(
        "{{\"id\":{i},\"name\":\"user {i}\",\"email\":\"u{i}@lab.test\",\"items\":[{}]}}",
        (0..20).map(|n| n.to_string()).collect::<Vec<_>>().join(",")
    )
    .repeat(8);
    NewExchange {
        started_at: 1_700_000_000_000 + i64::from(i),
        duration_ms: Some(i64::from(i % 300)),
        source: Source::Proxy,
        listener: Some("default".into()),
        client_addr: Some("127.0.0.1:50000".into()),
        scheme: "https".into(),
        method: if i.is_multiple_of(5) { "POST".into() } else { "GET".into() },
        host: host.clone(),
        port: 443,
        path: path.clone(),
        extension: path.rsplit_once('.').map(|(_, e)| e.split('?').next().unwrap_or("").to_string()),
        url: format!("https://{host}{path}"),
        http_version: "HTTP/1.1".into(),
        status: Some(if i.is_multiple_of(13) { 404 } else { 200 }),
        reason: Some("OK".into()),
        mime: Some("JSON".into()),
        request: format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nCookie: session=abc{i}\r\n\r\n").into_bytes(),
        response: Some(
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
                .into_bytes(),
        ),
        original_request: None,
        original_response: None,
        request_len: 120,
        response_len: Some(body.len() as u64 + 80),
        server_ip: Some("127.0.0.1".into()),
        tls: true,
        error: None,
        notes: i.is_multiple_of(997).then(|| "interesting".to_string()),
        flags: 0,
        ws_key: None,
    }
}

fn time<T, E: std::fmt::Debug>(label: &str, f: impl FnOnce() -> Result<T, E>) -> T {
    let t = Instant::now();
    let out = f().unwrap();
    println!("{label:<44} {:>9.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance measurement"]
async fn large_project() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let mut cfg = chosproxy_core::config::ProxyConfig::default();
    cfg.listeners[0].start_on_open = false;
    engine.save_proxy_config(cfg).await.unwrap();
    let info = engine.new_temporary_project().await.unwrap();
    let db = chosproxy_storage::ProjectDb::open(
        std::path::Path::new(&format!("{}-bench", info.path)),
        Default::default(),
    )
    .unwrap();
    drop(db);
    engine.close_project().await.unwrap();

    // Fill a project file directly, then open it through the engine.
    let path = dir.path().join("big.chosproxy");
    {
        let db = chosproxy_storage::ProjectDb::open(&path, Default::default()).unwrap();
        time(&format!("insert {N} exchanges (queued)"), || {
            for i in 0..N {
                while db.submit(exchange(i)).is_err() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            db.flush()
        });
    }
    println!(
        "project size: {:.1} MB",
        std::fs::metadata(&path).unwrap().len() as f64 / 1e6
    );
    time("open project", || futures_block(engine.open_project(&path)));
    let mut scope = engine.scope_config();
    scope.include.push(ScopeRule {
        host: "app3.lab.test".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();

    let page = |q: HistoryQuery| HistoryQuery { limit: 200, ..q };
    time("history first page (no filter)", || {
        futures_block(engine.history_page(page(HistoryQuery::default())))
    });
    time("history last page (offset 99800)", || {
        futures_block(engine.history_page(page(HistoryQuery {
            offset: N - 200,
            ..Default::default()
        })))
    });
    time("history text search", || {
        futures_block(engine.history_page(page(HistoryQuery {
            search: Some("orders/42".into()),
            ..Default::default()
        })))
    });
    time("history regex search", || {
        futures_block(engine.history_page(page(HistoryQuery {
            search: Some(r"users/\d+7\?".into()),
            search_regex: true,
            ..Default::default()
        })))
    });
    time("history sorted by status (last page)", || {
        futures_block(engine.history_page(page(HistoryQuery {
            sort: chosproxy_storage::HistorySort::Status,
            offset: N - 200,
            ..Default::default()
        })))
    });
    time("history sorted by duration", || {
        futures_block(engine.history_page(page(HistoryQuery {
            sort: chosproxy_storage::HistorySort::Duration,
            offset: 50_000,
            ..Default::default()
        })))
    });
    time("history sorted by length", || {
        futures_block(engine.history_page(page(HistoryQuery {
            sort: chosproxy_storage::HistorySort::Length,
            offset: 50_000,
            ..Default::default()
        })))
    });
    time("history sorted by host", || {
        futures_block(engine.history_page(page(HistoryQuery {
            sort: chosproxy_storage::HistorySort::Host,
            ..Default::default()
        })))
    });
    time("history in-scope only", || {
        futures_block(engine.history_page(page(HistoryQuery {
            in_scope_only: true,
            ..Default::default()
        })))
    });
    time("history methods + status + static", || {
        futures_block(engine.history_page(page(HistoryQuery {
            methods: vec!["POST".into()],
            status_classes: vec![2],
            hide_extensions: vec!["png".into(), "js".into()],
            ..Default::default()
        })))
    });
    time("history site node filter", || {
        futures_block(engine.history_page(page(HistoryQuery {
            site: Some(SiteFilter {
                scheme: "https".into(),
                host: "app3.lab.test".into(),
                port: 443,
                path: "/api/".into(),
            }),
            ..Default::default()
        })))
    });
    time("dashboard (stats + findings)", || {
        futures_block(engine.dashboard())
    });
    time("site map (all)", || {
        futures_block(engine.site_map(HistoryQuery::default()))
    });
    time("site map (in scope)", || {
        futures_block(engine.site_map(HistoryQuery {
            in_scope_only: true,
            ..Default::default()
        }))
    });
    time("search bodies 'u4242@' (max 500 hits)", || {
        futures_block(engine.search(SearchQuery {
            text: "u4242@".into(),
            ..SearchQuery::default()
        }))
    });
    time("search rare text (full scan)", || {
        futures_block(engine.search(SearchQuery {
            text: "does-not-exist".into(),
            ..SearchQuery::default()
        }))
    });
    time("exchange detail", || {
        futures_block(engine.exchange_detail(12345))
    });
    time("export copy (VACUUM INTO)", || {
        futures_block(engine.export_project(&dir.path().join("copy"), false))
    });
    engine.close_project().await.unwrap();
}

fn futures_block<F: std::future::Future>(f: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
}

/// Many concurrent keep-alive clients through the proxy to the lab server.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "performance measurement"]
async fn proxy_under_load() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    const CLIENTS: usize = 64;
    const PER_CLIENT: usize = 100;

    let dir = tempfile::tempdir().unwrap();
    let lab = chosproxy_lab::LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let mut cfg = chosproxy_core::config::ProxyConfig::default();
    cfg.listeners[0].port = 0;
    engine.save_proxy_config(cfg).await.unwrap();
    engine.new_temporary_project().await.unwrap();
    let proxy: std::net::SocketAddr = match &engine.listeners().await[0].state {
        chosproxy_core::event::ListenerState::Running { address } => address.parse().unwrap(),
        other => panic!("{other:?}"),
    };
    let url = format!("{}/json", lab.url());

    let started = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..CLIENTS {
        let url = url.clone();
        tasks.push(tokio::spawn(async move {
            let stream = tokio::net::TcpStream::connect(proxy).await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut r = BufReader::new(r);
            let mut latencies = Vec::with_capacity(PER_CLIENT);
            for _ in 0..PER_CLIENT {
                let t = Instant::now();
                w.write_all(format!("GET {url} HTTP/1.1\r\nHost: lab\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).await.unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).await.unwrap();
                latencies.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            latencies
        }));
    }
    let mut all = Vec::new();
    for t in tasks {
        all.extend(t.await.unwrap());
    }
    let elapsed = started.elapsed().as_secs_f64();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let total = CLIENTS * PER_CLIENT;
    println!("requests                 {total}");
    println!(
        "throughput               {:.0} req/s",
        total as f64 / elapsed
    );
    println!("latency p50              {:.2} ms", all[total / 2]);
    println!("latency p99              {:.2} ms", all[total * 99 / 100]);
    // Every exchange must be stored, none dropped.
    let mut stored = 0;
    for _ in 0..500 {
        stored = engine
            .history_page(HistoryQuery {
                limit: 1,
                ..Default::default()
            })
            .await
            .unwrap()
            .total;
        if stored >= total as u64 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let dash = engine.dashboard().await.unwrap();
    println!("stored                   {stored}");
    println!("captures dropped         {}", dash.captures_dropped);
    assert_eq!(stored, total as u64);
    assert_eq!(dash.captures_dropped, 0);
    engine.close_project().await.unwrap();
}

/// Throughput of the M9–M12 analysis code on realistic messages.
#[test]
#[ignore = "performance measurement"]
fn analysis_throughput() {
    use chosproxy_http::ResponseHead;
    use chosproxy_http::content::split_message;

    let mut body = String::from("<html><head><title>Shop</title></head><body>");
    for i in 0..400 {
        body.push_str(&format!(
            "<a href=\"/p/{i}?ref=home\">Item {i}</a><img src=\"/img/{i}.png\">\
             <script>fetch('/api/items/{i}')</script>"
        ));
    }
    body.push_str(
        "<form action=/login method=post><input type=password name=p></form></body></html>",
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
         Set-Cookie: sid=abcdef0123456789; Path=/\r\nServer: nginx/1.25.3\r\n\r\n{body}",
        body.len()
    );
    let request = b"GET /p/1?q=x HTTP/1.1\r\nHost: shop.lab.test\r\nCookie: sid=1\r\n\r\n";
    let n = 5_000;
    let t = Instant::now();
    let mut issues = 0;
    for _ in 0..n {
        let msg = chosproxy_scanner::Message::new(
            "https",
            "shop.lab.test",
            443,
            "/p/1?q=x",
            request,
            Some(response.as_bytes()),
        );
        issues += chosproxy_scanner::analyze(&msg, |_| true).len();
    }
    let per = t.elapsed().as_secs_f64() * 1e6 / f64::from(n);
    println!(
        "passive checks, {} KB page           {per:>9.1} µs/exchange ({issues} issues)",
        response.len() / 1024
    );

    let page = url::Url::parse("https://shop.lab.test/").unwrap();
    let (head, body) = split_message(response.as_bytes());
    let head = ResponseHead::parse(head.to_vec()).unwrap();
    let text = String::from_utf8_lossy(body);
    let t = Instant::now();
    let mut links = 0;
    for _ in 0..1_000 {
        links += chosproxy_crawler::discover(&page, &head, &text, true).len();
    }
    println!(
        "crawler link discovery                     {:>9.1} µs/page ({} links)",
        t.elapsed().as_secs_f64() * 1e3,
        links / 1_000
    );

    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let tokens: Vec<String> = (0..chosproxy_sequencer::MAX_TOKENS)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            format!("{x:016x}{:016x}", x.rotate_left(29))
        })
        .collect();
    let _analysis = time("sequencer analysis, 100 000 × 32 hex", || {
        Ok::<_, String>(chosproxy_sequencer::analyze(&tokens))
    });
}
