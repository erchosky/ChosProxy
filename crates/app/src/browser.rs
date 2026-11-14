//! Lab browser: an installed Chromium-family browser or Firefox launched with
//! a temporary profile routed through the proxy. The user's normal browser
//! profile and the system trust store are never modified.
//!
//! * Chromium-family: `--ignore-certificate-errors-spki-list` pins the
//!   interception leaf key for this profile only, so no CA import is needed.
//! * Firefox: a temporary profile with proxy preferences; the CA is imported
//!   into that profile with NSS `certutil` when available.

use serde::Serialize;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Browser engine family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BrowserKind {
    /// Chromium, Chrome, Brave, Edge.
    Chromium,
    /// Firefox / Firefox ESR.
    Firefox,
}

/// An installed browser that can be used as the lab browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowserInfo {
    /// Stable identifier (the executable path).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Engine family.
    pub kind: BrowserKind,
}

struct Candidate {
    name: &'static str,
    kind: BrowserKind,
    /// Absolute path, or a bare command looked up on `PATH`.
    program: &'static str,
}

const fn c(name: &'static str, kind: BrowserKind, program: &'static str) -> Candidate {
    Candidate {
        name,
        kind,
        program,
    }
}

#[cfg(target_os = "macos")]
const CANDIDATES: &[Candidate] = &[
    c(
        "Chromium",
        BrowserKind::Chromium,
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ),
    c(
        "Google Chrome",
        BrowserKind::Chromium,
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ),
    c(
        "Brave",
        BrowserKind::Chromium,
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ),
    c(
        "Microsoft Edge",
        BrowserKind::Chromium,
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ),
    c(
        "Firefox",
        BrowserKind::Firefox,
        "/Applications/Firefox.app/Contents/MacOS/firefox",
    ),
];

#[cfg(not(target_os = "macos"))]
const CANDIDATES: &[Candidate] = &[
    c("Chromium", BrowserKind::Chromium, "chromium"),
    c("Chromium", BrowserKind::Chromium, "chromium-browser"),
    c("Google Chrome", BrowserKind::Chromium, "google-chrome"),
    c(
        "Google Chrome",
        BrowserKind::Chromium,
        "google-chrome-stable",
    ),
    c("Brave", BrowserKind::Chromium, "brave-browser"),
    c("Microsoft Edge", BrowserKind::Chromium, "microsoft-edge"),
    c("Firefox ESR", BrowserKind::Firefox, "firefox-esr"),
    c("Firefox", BrowserKind::Firefox, "firefox"),
];

/// True if `p` is an executable file.
pub(crate) fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Finds `program` (absolute path or command name) using `path_var`.
fn resolve(program: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.is_absolute() {
        return is_executable(p).then(|| p.to_path_buf());
    }
    std::env::split_paths(path_var?)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

/// Installed browsers usable as lab browser, Chromium family first.
pub fn detect() -> Vec<BrowserInfo> {
    detect_with_path(std::env::var_os("PATH").as_deref())
}

fn detect_with_path(path_var: Option<&std::ffi::OsStr>) -> Vec<BrowserInfo> {
    let mut found: Vec<BrowserInfo> = Vec::new();
    for cand in CANDIDATES {
        if let Some(path) = resolve(cand.program, path_var) {
            // Several command names can point at the same binary.
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            let id = canonical.display().to_string();
            if !found.iter().any(|b| b.id == id) {
                found.push(BrowserInfo {
                    id,
                    name: cand.name.to_string(),
                    kind: cand.kind,
                });
            }
        }
    }
    found
}

/// Everything needed to start a lab browser.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// Proxy listener to use (loopback address).
    pub proxy: SocketAddr,
    /// Base64 SHA-256 of the interception leaf key (Chromium pin).
    pub spki_pin: String,
    /// CA certificate, PEM (Firefox import).
    pub ca_pem: String,
    /// Page to open.
    pub url: String,
    /// Temporary profile directory (created by the caller, deleted on exit).
    pub profile_dir: PathBuf,
    /// Open the DevTools endpoint on a random loopback port (DOM
    /// Inspector); the page is then opened through DevTools.
    pub devtools: bool,
}

/// Command-line arguments for a Chromium-family browser.
pub fn chromium_args(spec: &LaunchSpec, running_as_root: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        format!("--user-data-dir={}", spec.profile_dir.display()).into(),
        format!("--proxy-server=http://{}", spec.proxy).into(),
        // Also proxy localhost targets (local labs such as Juice Shop).
        "--proxy-bypass-list=<-loopback>".into(),
        format!("--ignore-certificate-errors-spki-list={}", spec.spki_pin).into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        // Keep browser background chatter out of the project history.
        "--disable-background-networking".into(),
        "--disable-component-update".into(),
        "--disable-sync".into(),
        "--disable-default-apps".into(),
        "--disable-domain-reliability".into(),
        "--disable-client-side-phishing-detection".into(),
        "--disable-search-engine-choice-screen".into(),
        "--no-pings".into(),
        "--disable-features=Translate,OptimizationHints,OptimizationGuideModelDownloading,\
         MediaRouter,AutofillServerCommunication,InterestFeedContentSuggestions"
            .into(),
    ];
    if running_as_root {
        // Chromium refuses to start as root without this.
        args.push("--no-sandbox".into());
    }
    if spec.devtools {
        // Loopback only, random port; found in <profile>/DevToolsActivePort.
        args.push("--remote-debugging-port=0".into());
        args.push("--remote-debugging-address=127.0.0.1".into());
    }
    args.push("--new-window".into());
    // With DevTools the start page is opened once instrumentation is ready.
    args.push(if spec.devtools {
        "about:blank".into()
    } else {
        spec.url.clone().into()
    });
    args
}

/// `user.js` preferences for a Firefox lab profile.
pub fn firefox_prefs(spec: &LaunchSpec) -> String {
    let host = spec.proxy.ip().to_string();
    let port = spec.proxy.port();
    let prefs: Vec<(&str, String)> = vec![
        ("network.proxy.type", "1".into()),
        ("network.proxy.http", format!("{host:?}")),
        ("network.proxy.http_port", port.to_string()),
        ("network.proxy.ssl", format!("{host:?}")),
        ("network.proxy.ssl_port", port.to_string()),
        ("network.proxy.share_proxy_settings", "true".into()),
        ("network.proxy.no_proxies_on", "\"\"".into()),
        ("network.proxy.allow_hijacking_localhost", "true".into()),
        ("browser.shell.checkDefaultBrowser", "false".into()),
        ("browser.aboutwelcome.enabled", "false".into()),
        (
            "browser.startup.homepage_override.mstone",
            "\"ignore\"".into(),
        ),
        ("datareporting.policy.dataSubmissionEnabled", "false".into()),
        ("toolkit.telemetry.enabled", "false".into()),
        ("app.normandy.enabled", "false".into()),
        ("network.captive-portal-service.enabled", "false".into()),
        ("network.connectivity-service.enabled", "false".into()),
        ("browser.safebrowsing.malware.enabled", "false".into()),
        ("browser.safebrowsing.phishing.enabled", "false".into()),
        ("extensions.update.enabled", "false".into()),
    ];
    prefs
        .into_iter()
        .map(|(k, v)| format!("user_pref({k:?}, {v});\n"))
        .collect()
}

/// Command-line arguments for Firefox.
pub fn firefox_args(spec: &LaunchSpec) -> Vec<OsString> {
    vec![
        "--no-remote".into(),
        "--new-instance".into(),
        "--profile".into(),
        spec.profile_dir.clone().into_os_string(),
        spec.url.clone().into(),
    ]
}

/// Prepares a Firefox profile: preferences, and the CA imported with NSS
/// `certutil` if it is installed. Returns a warning when the CA could not be
/// imported automatically.
pub fn prepare_firefox_profile(spec: &LaunchSpec) -> std::io::Result<Option<String>> {
    std::fs::write(spec.profile_dir.join("user.js"), firefox_prefs(spec))?;
    let Some(certutil) = resolve("certutil", std::env::var_os("PATH").as_deref()) else {
        return Ok(Some(
            "Firefox does not trust the local CA in this profile (NSS certutil not found; \
             install libnss3-tools). Open http://chosproxy/ in the lab browser to import it."
                .into(),
        ));
    };
    let ca_file = spec.profile_dir.join("chosproxy-ca.pem");
    std::fs::write(&ca_file, &spec.ca_pem)?;
    let db = format!("sql:{}", spec.profile_dir.display());
    let run = |args: &[&std::ffi::OsStr]| {
        std::process::Command::new(&certutil)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let ok = run(&[
        "-N".as_ref(),
        "-d".as_ref(),
        db.as_ref(),
        "--empty-password".as_ref(),
    ]) && run(&[
        "-A".as_ref(),
        "-n".as_ref(),
        "ChosProxy Local CA".as_ref(),
        "-t".as_ref(),
        "C,,".as_ref(),
        "-i".as_ref(),
        ca_file.as_os_str(),
        "-d".as_ref(),
        db.as_ref(),
    ]);
    let _ = std::fs::remove_file(&ca_file);
    Ok((!ok).then(|| {
        "Importing the local CA into the Firefox profile failed; open http://chosproxy/ in the \
         lab browser to import it manually."
            .to_string()
    }))
}

/// True when the process runs as root (Chromium then needs `--no-sandbox`).
pub fn running_as_root() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> LaunchSpec {
        LaunchSpec {
            proxy: "127.0.0.1:8080".parse().unwrap(),
            spki_pin: "PIN=".into(),
            ca_pem: "-----BEGIN CERTIFICATE-----\n".into(),
            url: "http://chosproxy/".into(),
            profile_dir: PathBuf::from("/tmp/profile x"),
            devtools: false,
        }
    }

    #[test]
    fn devtools_opens_a_loopback_port_and_a_blank_page() {
        let args: Vec<String> = chromium_args(
            &LaunchSpec {
                devtools: true,
                ..spec()
            },
            false,
        )
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect();
        assert!(args.contains(&"--remote-debugging-port=0".to_string()));
        assert!(args.contains(&"--remote-debugging-address=127.0.0.1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("about:blank"));
        let plain: Vec<String> = chromium_args(&spec(), false)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert!(!plain.iter().any(|a| a.starts_with("--remote-debugging")));
    }

    #[test]
    fn chromium_arguments_are_isolated_and_pinned() {
        let args: Vec<String> = chromium_args(&spec(), false)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert!(args.contains(&"--user-data-dir=/tmp/profile x".to_string()));
        assert!(args.contains(&"--proxy-server=http://127.0.0.1:8080".to_string()));
        assert!(args.contains(&"--proxy-bypass-list=<-loopback>".to_string()));
        assert!(args.contains(&"--ignore-certificate-errors-spki-list=PIN=".to_string()));
        assert!(!args.contains(&"--no-sandbox".to_string()));
        assert_eq!(args.last().unwrap(), "http://chosproxy/");
        assert!(chromium_args(&spec(), true).contains(&"--no-sandbox".into()));
    }

    #[test]
    fn firefox_profile_routes_everything_through_the_proxy() {
        let prefs = firefox_prefs(&spec());
        assert!(prefs.contains("user_pref(\"network.proxy.type\", 1);"));
        assert!(prefs.contains("user_pref(\"network.proxy.ssl\", \"127.0.0.1\");"));
        assert!(prefs.contains("user_pref(\"network.proxy.ssl_port\", 8080);"));
        assert!(prefs.contains("allow_hijacking_localhost\", true"));
        let args = firefox_args(&spec());
        assert_eq!(args[0], "--no-remote");
        assert!(args.contains(&OsString::from("/tmp/profile x")));
    }

    #[cfg(unix)]
    #[test]
    fn detects_browsers_on_path_once() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("chromium");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&exe, dir.path().join("chromium-browser")).unwrap();
        std::fs::write(dir.path().join("firefox"), "not executable").unwrap();
        let found = detect_with_path(Some(dir.path().as_os_str()));
        if cfg!(target_os = "macos") {
            // macOS uses fixed application paths, not PATH.
            assert!(found.iter().all(|b| b.id.starts_with("/Applications/")));
        } else {
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].kind, BrowserKind::Chromium);
        }
    }
}
