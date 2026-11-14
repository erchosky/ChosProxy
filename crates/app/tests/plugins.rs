//! Plugins end to end with the HelloPlugin example (needs `python3`).

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, PluginPermission as P, PluginStatus};
use chosproxy_core::config::ProxyConfig;
use chosproxy_core::event::ListenerState;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::model::Highlight;
use chosproxy_storage::{FindingQuery, HistoryQuery};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn proxy_addr(engine: &Engine) -> SocketAddr {
    match &engine.listeners().await[0].state {
        ListenerState::Running { address } => address.parse().unwrap(),
        other => panic!("listener not running: {other:?}"),
    }
}

async fn fetch(proxy: SocketAddr, url: &str) {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(format!("GET {url} HTTP/1.1\r\nHost: lab\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
}

async fn status(engine: &Engine) -> PluginStatus {
    engine.plugins().await.unwrap()[0].status
}

async fn wait_status(engine: &Engine, want: PluginStatus) {
    for _ in 0..100 {
        if status(engine).await == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "plugin never reached {want:?}: {:?} {:?}",
        engine.plugins().await.unwrap()[0],
        engine.plugin_logs("hello-plugin")
    );
}

async fn wait_for<T>(mut check: impl AsyncFnMut() -> Option<T>) -> T {
    for _ in 0..100 {
        if let Some(v) = check().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("condition never met");
}

#[tokio::test]
async fn hello_plugin_reports_findings_within_its_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let mut cfg = ProxyConfig::default();
    cfg.listeners[0].port = 0;
    engine.save_proxy_config(cfg).await.unwrap();
    engine.new_temporary_project().await.unwrap();

    // Installed, nothing granted or running until the tester decides.
    assert!(engine.plugins().await.unwrap().is_empty());
    let installed = engine.plugins_install_example().await.unwrap();
    let list = engine.plugins().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        (list[0].id.as_str(), list[0].status),
        ("hello-plugin", PluginStatus::Disabled)
    );
    assert_eq!(list[0].requested.len(), 3);
    assert!(list[0].granted.is_empty());

    // Enabled without project.write: it reads traffic and adds findings, but
    // its annotations are refused.
    engine
        .plugin_set(
            "hello-plugin",
            true,
            vec![P::NetworkRead, P::ScannerPassive],
        )
        .await
        .unwrap();
    wait_status(&engine, PluginStatus::Running).await;
    let proxy = proxy_addr(&engine).await;
    fetch(proxy, &format!("{}/debug-token", lab.url())).await;
    fetch(proxy, &format!("{}/json", lab.url())).await;

    let findings = wait_for(async || {
        let rows = engine
            .findings(FindingQuery {
                source: Some("plugin".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        (!rows.is_empty()).then_some(rows)
    })
    .await;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, "Debug token exposed");
    assert!(findings[0].url.ends_with("/debug-token"));
    let audit = engine.audit_log(50).await.unwrap();
    assert!(audit.iter().any(|a| a.action == "plugin.enabled"));

    // Granting project.write lets it highlight the request.
    engine
        .plugin_set(
            "hello-plugin",
            true,
            vec![P::NetworkRead, P::ScannerPassive, P::ProjectWrite],
        )
        .await
        .unwrap();
    wait_status(&engine, PluginStatus::Running).await;
    fetch(proxy, &format!("{}/debug-token", lab.url())).await;
    let highlighted = wait_for(async || {
        let page = engine
            .history_page(HistoryQuery {
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        page.rows
            .into_iter()
            .find(|r| r.highlight == Some(Highlight::Yellow))
    })
    .await;
    assert!(highlighted.notes.unwrap().contains("HelloPlugin"));

    // A plugin whose files change must be approved again.
    std::fs::write(installed.join("hello.py"), "print('changed')\n").unwrap();
    assert_eq!(status(&engine).await, PluginStatus::NeedsApproval);
    assert!(
        engine
            .plugin_set("hello-plugin", true, vec![P::NetworkSend])
            .await
            .is_err(),
        "only requested permissions can be granted"
    );

    // Disabling stops it.
    engine
        .plugin_set("hello-plugin", false, vec![])
        .await
        .unwrap();
    assert_eq!(status(&engine).await, PluginStatus::Disabled);
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn broken_plugins_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let plugins = engine.plugins_dir();
    let bad = plugins.join("bad-one");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(
        bad.join("plugin.toml"),
        "id = \"other\"\nname = \"x\"\nversion = \"1\"\napi = 1\ncommand = [\"x\"]\n",
    )
    .unwrap();
    let silent = plugins.join("silent");
    std::fs::create_dir_all(&silent).unwrap();
    std::fs::write(
        silent.join("plugin.toml"),
        "id = \"silent\"\nname = \"Silent\"\nversion = \"1\"\napi = 1\ncommand = [\"python3\", \"-c\", \"import sys; sys.exit(3)\"]\n",
    )
    .unwrap();
    let list = engine.plugins().await.unwrap();
    assert_eq!(list[0].status, PluginStatus::Invalid);
    assert!(list[0].message.as_deref().unwrap().contains("named after"));

    // A plugin that tries what it was not granted is refused, and it shows.
    let rude = plugins.join("rude");
    std::fs::create_dir_all(&rude).unwrap();
    std::fs::write(
        rude.join("plugin.toml"),
        "id = \"rude\"\nname = \"Rude\"\nversion = \"1\"\napi = 1\npermissions = [\"project.write\"]\n\
         command = [\"python3\", \"rude.py\"]\n",
    )
    .unwrap();
    std::fs::write(
        rude.join("rude.py"),
        r#"import json, sys
sys.stdin.readline()
for m in [{"type": "ready"}, {"type": "annotate", "exchangeId": 1, "highlight": "red"},
          {"type": "finding", "exchangeId": 1, "key": "k", "kind": "K", "title": "T"}, "not json"]:
    print(m if isinstance(m, str) else json.dumps(m), flush=True)
sys.stdin.read()
"#,
    )
    .unwrap();
    engine.plugin_set("rude", true, vec![]).await.unwrap();
    let mut logs = Vec::new();
    for _ in 0..100 {
        logs = engine.plugin_logs("rude");
        if logs
            .iter()
            .filter(|l| l.message.starts_with("Refused"))
            .count()
            == 2
            && logs
                .iter()
                .any(|l| l.message.starts_with("invalid message"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let text: Vec<&str> = logs.iter().map(|l| l.message.as_str()).collect();
    assert!(text.iter().any(|m| m.contains("project.write")), "{text:?}");
    assert!(
        text.iter().any(|m| m.contains("scanner.passive")),
        "{text:?}"
    );
    assert!(
        text.iter().any(|m| m.starts_with("invalid message")),
        "{text:?}"
    );
    let audit = engine.audit_log(50).await.unwrap();
    assert_eq!(
        audit.iter().filter(|a| a.action == "plugin.denied").count(),
        2
    );
    engine.plugin_set("rude", false, vec![]).await.unwrap();

    engine.plugin_set("silent", true, vec![]).await.unwrap();
    for _ in 0..100 {
        if engine.plugins().await.unwrap()[2].status == PluginStatus::Crashed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let silent = &engine.plugins().await.unwrap()[2];
    assert_eq!(silent.status, PluginStatus::Crashed);
    assert!(
        silent.message.as_deref().unwrap().contains("exit"),
        "{:?}",
        silent.message
    );
}

const PROBE_V2: &str = r#"import json, sys
host, port = sys.argv[1], int(sys.argv[2])
def send(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()
for line in sys.stdin:
    m = json.loads(line)
    t = m.get("type")
    if t == "hello":
        send({"type": "ready"})
        send({"type": "registerAction", "action": "probe", "label": "Probe it"})
        send({"type": "send", "id": 1, "scheme": "http", "host": host, "port": port,
              "request": "GET /json HTTP/1.1\nHost: lab\n\n"})
    elif t == "result":
        if not m["ok"]:
            send({"type": "log", "level": "warning", "message": "refused: " + m["error"]})
        elif m["id"] == 1:
            send({"type": "log", "message": "sent status %s" % m["data"].get("status")})
            send({"type": "query", "id": 2, "query": {"what": "history", "search": "/json"}})
        elif m["id"] == 2:
            send({"type": "log", "message": "history %d" % m["data"]["total"]})
    elif t == "action":
        send({"type": "output", "call": m["call"], "text": "probe: " + m["exchange"]["url"]})
    elif t == "shutdown":
        break
"#;

#[tokio::test]
async fn api_v2_plugins_send_query_and_add_actions() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    let plugin_dir = engine.plugins_dir().join("probe-v2");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("probe.py"), PROBE_V2).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        format!(
            "id = \"probe-v2\"\nname = \"Probe\"\nversion = \"1.0\"\napi = 2\n\
             command = [\"python3\", \"probe.py\", \"127.0.0.1\", \"{}\"]\n\
             permissions = [\"network.send\", \"project.read\", \"ui.extend\"]\n",
            lab.addr().port()
        ),
    )
    .unwrap();
    let logs = |e: &Engine| {
        e.plugin_logs("probe-v2")
            .into_iter()
            .map(|l| l.message)
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Without network.send its request is refused (and it is told why).
    engine
        .plugin_set("probe-v2", true, vec![P::ProjectRead, P::UiExtend])
        .await
        .unwrap();
    wait_for(async || {
        logs(&engine)
            .contains("refused: network.send not granted")
            .then_some(())
    })
    .await;

    // With every permission: the request goes out and is stored, the
    // history query sees it, and the menu action answers.
    engine
        .plugin_set(
            "probe-v2",
            true,
            vec![P::NetworkSend, P::ProjectRead, P::UiExtend],
        )
        .await
        .unwrap();
    wait_for(async || logs(&engine).contains("history 1").then_some(())).await;
    assert!(
        logs(&engine).contains("sent status 200"),
        "{}",
        logs(&engine)
    );
    let rows = engine
        .history_page(HistoryQuery {
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, chosproxy_core::Source::Plugin);
    assert!(
        rows[0]
            .notes
            .clone()
            .unwrap_or_default()
            .contains("probe-v2")
    );

    let actions = wait_for(async || {
        let a = engine.plugin_actions();
        (!a.is_empty()).then_some(a)
    })
    .await;
    assert_eq!(
        (actions[0].label.as_str(), actions[0].plugin_name.as_str()),
        ("Probe it", "Probe")
    );
    let out = engine
        .plugin_run_action("probe-v2", "probe", rows[0].id)
        .await
        .unwrap();
    assert_eq!(out, format!("probe: {}/json", lab.url()));
    engine.close_project().await.unwrap();
}

#[tokio::test]
async fn signed_plugins_install_from_a_catalog_and_require_trust() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();

    // A publisher signs a plugin in a catalog folder.
    let catalog = dir.path().join("catalog").join("quiet");
    std::fs::create_dir_all(&catalog).unwrap();
    std::fs::write(
        catalog.join("quiet.py"),
        "import json, sys\nfor line in sys.stdin:\n    m = json.loads(line)\n    if m['type'] == 'hello':\n        print(json.dumps({'type': 'ready'}), flush=True)\n    elif m['type'] == 'shutdown':\n        break\n",
    )
    .unwrap();
    std::fs::write(
        catalog.join("plugin.toml"),
        "id = \"quiet\"\nname = \"Quiet\"\nversion = \"1.0\"\napi = 2\ncommand = [\"python3\", \"quiet.py\"]\npermissions = []\n",
    )
    .unwrap();
    let (key, public) = chosproxy_plugin::generate_key().unwrap();
    chosproxy_plugin::sign_dir(&catalog, &key, "Lab Team").unwrap();

    // Installed from the catalog: disabled, signed by an unknown key.
    let info = engine.plugins_install_from(&catalog).await.unwrap();
    assert_eq!(info.status, PluginStatus::Disabled);
    assert_eq!(info.signature.state, "untrusted");
    assert_eq!(info.signature.publisher.as_deref(), Some("Lab Team"));
    assert!(
        engine.plugins_install_from(&catalog).await.is_err(),
        "no silent replace"
    );

    // Only trusted publishers may run: enabling is not enough…
    engine.plugins_require_signed(true).await.unwrap();
    engine.plugin_set("quiet", true, vec![]).await.unwrap();
    let quiet =
        |list: Vec<chosproxy_app::PluginInfo>| list.into_iter().find(|p| p.id == "quiet").unwrap();
    let p = quiet(engine.plugins().await.unwrap());
    assert_eq!(p.status, PluginStatus::NeedsApproval);
    assert!(p.message.unwrap_or_default().contains("trusted publisher"));

    // …until its key is trusted.
    engine
        .plugin_trust(public.clone(), "Lab Team".into(), true)
        .await
        .unwrap();
    let running = wait_for(async || {
        let p = quiet(engine.plugins().await.unwrap());
        (p.status == PluginStatus::Running).then_some(p)
    })
    .await;
    assert_eq!(running.signature.state, "trusted");

    // A changed file breaks the signature.
    let installed = engine.plugins_dir().join("quiet");
    std::fs::write(installed.join("quiet.py"), "print('changed')\n").unwrap();
    let p = quiet(engine.plugins().await.unwrap());
    assert_eq!(p.signature.state, "invalid");
    assert_ne!(p.status, PluginStatus::Running);
    engine.close_project().await.unwrap();
}

const HOOK_PLUGIN: &str = r#"
import json, sys
def send(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()
for line in sys.stdin:
    m = json.loads(line)
    t = m.get("type")
    if t == "hello":
        send({"type": "ready"})
        send({"type": "registerHook", "hosts": ["127.0.0.1"], "responses": True})
    elif t == "hook":
        call, url, raw = m["call"], m["request"]["url"], m["request"]["raw"]
        if "response" in m:
            if "/chunked" in url and "first,second,third" in m["response"] and "GET /chunked" in raw:
                send({"type": "hookResult", "call": call, "action": "replace",
                      "response": "HTTP/1.1 200 OK\nContent-Type: text/plain\n\nrewritten by the plugin"})
            elif "/status/418" in url:
                send({"type": "hookResult", "call": call, "action": "block", "reason": "no tea"})
            else:
                send({"type": "hookResult", "call": call, "action": "continue"})
        elif "/blocked" in url:
            send({"type": "hookResult", "call": call, "action": "block", "reason": "not on my watch"})
        elif "/answered" in url:
            send({"type": "hookResult", "call": call, "action": "respond",
                  "response": "HTTP/1.1 200 OK\nContent-Type: text/plain\n\nfrom the plugin"})
        elif "/json" in url:
            send({"type": "hookResult", "call": call, "action": "replace",
                  "request": raw.replace("GET /json", "GET /headers", 1)})
        elif "/silent" in url:
            pass
        else:
            send({"type": "hookResult", "call": call, "action": "continue"})
    elif t == "shutdown":
        break
"#;

/// A request through the proxy; the whole response as text.
async fn fetch_text(proxy: SocketAddr, url: &str) -> String {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(format!("GET {url} HTTP/1.1\r\nHost: lab\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn plugin_hooks_change_block_and_answer_in_scope_requests() {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    let mut cfg = ProxyConfig::default();
    cfg.listeners[0].port = 0;
    engine.save_proxy_config(cfg).await.unwrap();
    engine.new_temporary_project().await.unwrap();
    let plugin_dir = engine.plugins_dir().join("hook-test");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("hook.py"), HOOK_PLUGIN).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        "id = \"hook-test\"\nname = \"Hook\"\nversion = \"1.0\"\napi = 2\n\
         command = [\"python3\", \"hook.py\"]\npermissions = [\"proxy.intercept\"]\n",
    )
    .unwrap();
    let logs = |e: &Engine| {
        e.plugin_logs("hook-test")
            .into_iter()
            .map(|l| l.message)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let proxy = proxy_addr(&engine).await;
    let base = lab.url().to_string();

    // Not granted: the hook is refused and traffic is untouched.
    engine.plugin_set("hook-test", true, vec![]).await.unwrap();
    wait_for(async || {
        logs(&engine)
            .contains("needs the proxy.intercept permission")
            .then_some(())
    })
    .await;
    assert!(
        fetch_text(proxy, &format!("{base}/blocked"))
            .await
            .contains("404")
    );

    engine
        .plugin_set("hook-test", true, vec![P::ProxyIntercept])
        .await
        .unwrap();
    wait_for(async || {
        logs(&engine)
            .contains("Request hook registered")
            .then_some(())
    })
    .await;

    // No scope defined: hooks do not apply.
    assert!(
        fetch_text(proxy, &format!("{base}/blocked"))
            .await
            .contains("404")
    );

    let mut scope = engine.scope_config();
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: "127.0.0.1".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();

    let blocked = fetch_text(proxy, &format!("{base}/blocked")).await;
    assert!(blocked.starts_with("HTTP/1.1 403"), "{blocked}");
    assert!(blocked.contains("not on my watch"), "{blocked}");
    let answered = fetch_text(proxy, &format!("{base}/answered")).await;
    assert!(answered.starts_with("HTTP/1.1 200"), "{answered}");
    assert!(answered.ends_with("from the plugin"), "{answered}");
    let replaced = fetch_text(proxy, &format!("{base}/json")).await;
    assert!(
        replaced.to_ascii_lowercase().contains("connection"),
        "{replaced}"
    );
    let start = std::time::Instant::now();
    let silent = fetch_text(proxy, &format!("{base}/silent")).await;
    assert!(silent.contains("404"), "fails open: {silent}");
    assert!(start.elapsed() >= Duration::from_millis(1900));
    // Responses (M31b): rewritten, blocked, or left alone.
    let rewritten = fetch_text(proxy, &format!("{base}/chunked")).await;
    assert!(rewritten.starts_with("HTTP/1.1 200"), "{rewritten}");
    assert!(
        rewritten.ends_with("rewritten by the plugin"),
        "{rewritten}"
    );
    let teapot = fetch_text(proxy, &format!("{base}/status/418")).await;
    assert!(teapot.starts_with("HTTP/1.1 502"), "{teapot}");
    assert!(teapot.contains("no tea"), "{teapot}");
    // Out of scope (another host name for the lab): not shown to the hook.
    let other = format!("http://localhost:{}/blocked", lab.addr().port());
    assert!(fetch_text(proxy, &other).await.contains("404"));

    // The history says what happened.
    let rows = wait_for(async || {
        let rows = engine
            .history_page(HistoryQuery {
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap()
            .rows;
        (rows.len() >= 9).then_some(rows)
    })
    .await;
    let note = |path: &str| {
        rows.iter()
            .filter(|r| r.path == path && r.host == "127.0.0.1")
            .filter_map(|r| r.notes.clone())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    assert!(note("/blocked").contains("Blocked by plugin hook-test: not on my watch"));
    assert!(note("/answered").contains("Answered by plugin hook-test"));
    assert!(note("/headers").contains("Plugin hook-test changed the request"));
    assert!(note("/silent").contains("did not decide within 2 s"));
    assert!(note("/chunked").contains("Plugin hook-test changed the response"));
    assert!(note("/status/418").contains("Response blocked by plugin hook-test: no tea"));
    assert!(!note("/headers").contains("response"));
    let answered_row = rows.iter().find(|r| r.path == "/answered").unwrap();
    assert_eq!(answered_row.status, Some(200));
    engine.close_project().await.unwrap();
}
