//! `chosproxy-cli` end to end: `serve` runs headless in a scratch home, and
//! the client commands drive it against the lab.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_lab::LabServer;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_chosproxy-cli");

struct Serve(Child);

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts `serve` and returns it with the API port it printed.
fn serve(home: &Path) -> (Serve, u16) {
    let mut child = Command::new(BIN)
        .args(["serve", "--port", "0"])
        .env("CHOSPROXY_HOME", home)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let first = lines.next().unwrap().unwrap();
    let port: u16 = first
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("unexpected first line {first:?}"));
    // Keep reading in the background so the pipe never fills.
    std::thread::spawn(move || for _ in lines {});
    (Serve(child), port)
}

fn run(home: &Path, port: u16, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(BIN)
        .args(args)
        .args(["--port", &port.to_string()])
        .env("CHOSPROXY_HOME", home)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_headless_and_drive_it_from_the_command_line() {
    let home = tempfile::tempdir().unwrap();
    // No proxy listener in this scratch home (8080 may be in use).
    std::fs::create_dir_all(home.path().join("config")).unwrap();
    std::fs::write(home.path().join("config/proxy.toml"), "listeners = []\n").unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let lab_url = format!("http://{}/", lab.addr());
    let home_path = home.path().to_path_buf();
    let lab_url2 = lab_url.clone();

    tokio::task::spawn_blocking(move || {
        let home = home_path.as_path();
        let (_serve, port) = serve(home);

        let (ok, out, _) = run(home, port, &["status"]);
        assert!(ok);
        assert!(out.contains("\"api\": 1"), "{out}");

        let (ok, out, _) = run(home, port, &["openapi"]);
        assert!(ok);
        assert!(
            out.contains("\"openapi\": \"3.1.0\"") && out.contains("/v1/events"),
            "{out}"
        );

        // A wrong token is refused.
        let (ok, _, err) = run(home, port, &["status", "--token", "0".repeat(64).as_str()]);
        assert!(!ok);
        assert!(err.contains("token"), "{err}");

        let (ok, _, err) = run(home, port, &["scope", "add", &lab_url2]);
        assert!(ok, "{err}");
        let (ok, out, err) = run(home, port, &["send", &format!("{lab_url2}json")]);
        assert!(ok, "{err}");
        assert!(out.contains("\"status\": 200"), "{out}");
        let (ok, out, _) = run(home, port, &["history", "--search", "json"]);
        assert!(ok);
        assert!(out.contains("/json") && out.contains("1 in all"), "{out}");

        let (ok, out, err) = run(home, port, &["crawl", &lab_url2]);
        assert!(ok, "{err}");
        let id = out.split_whitespace().nth(1).unwrap().to_string();
        let (ok, out, err) = run(home, port, &["wait", &id, "--timeout", "60"]);
        assert!(ok, "{err}");
        assert!(out.contains(&format!("Task {id}: completed")), "{out}");
        let (ok, out, _) = run(home, port, &["tasks"]);
        assert!(ok && out.contains("crawl"), "{out}");

        let report = home.join("report.md");
        let (ok, out, err) = run(
            home,
            port,
            &["report", "--output", report.to_str().unwrap()],
        );
        assert!(ok, "{err}");
        assert!(out.contains("written to"), "{out}");
        assert!(std::fs::read_to_string(&report).unwrap().starts_with('#'));

        let (ok, _, err) = run(home, port, &["nonsense"]);
        assert!(!ok && err.contains("unknown command"));
    })
    .await
    .unwrap();
    drop(lab_url);
}
