//! Content discovery (matrix DISCOVERY-001/002): finds unlinked folders and
//! files by requesting likely names under a base folder, and tells real
//! hits from "soft 404" pages by comparing every answer with what the site
//! returns for names that cannot exist.
//!
//! Pure logic only: sending, scope and storage are the engine's job.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;
use url::Url;

/// Common folder and file names (no credentials, no exploit strings).
pub const BUILTIN_WORDS: &[&str] = &[
    "admin",
    "administrator",
    "api",
    "app",
    "apps",
    "assets",
    "auth",
    "backup",
    "backups",
    "bin",
    "blog",
    "build",
    "cache",
    "cgi-bin",
    "config",
    "conf",
    "console",
    "content",
    "cp",
    "css",
    "dashboard",
    "data",
    "db",
    "debug",
    "default",
    "demo",
    "dev",
    "dist",
    "doc",
    "docs",
    "download",
    "downloads",
    "dump",
    "edit",
    "export",
    "files",
    "fonts",
    "graphql",
    "health",
    "help",
    "home",
    "images",
    "img",
    "import",
    "inc",
    "include",
    "includes",
    "index",
    "info",
    "internal",
    "js",
    "lib",
    "libs",
    "log",
    "login",
    "logs",
    "manage",
    "management",
    "media",
    "metrics",
    "misc",
    "monitor",
    "new",
    "old",
    "panel",
    "phpmyadmin",
    "private",
    "profile",
    "public",
    "register",
    "release",
    "reports",
    "rest",
    "scripts",
    "search",
    "secure",
    "server",
    "server-status",
    "service",
    "services",
    "settings",
    "setup",
    "shop",
    "signin",
    "signup",
    "src",
    "stage",
    "staging",
    "static",
    "stats",
    "status",
    "storage",
    "swagger",
    "system",
    "temp",
    "test",
    "testing",
    "tests",
    "tmp",
    "tools",
    "upload",
    "uploads",
    "user",
    "users",
    "v1",
    "v2",
    "vendor",
    "web",
    "webadmin",
    "wp-admin",
    "wp-content",
    "wp-includes",
    "www",
];

/// Well-known files checked once per folder (exact names).
pub const BUILTIN_FILES: &[&str] = &[
    ".DS_Store",
    ".env",
    ".git/HEAD",
    ".gitignore",
    ".htaccess",
    ".svn/entries",
    ".well-known/security.txt",
    "crossdomain.xml",
    "humans.txt",
    "openapi.json",
    "package.json",
    "phpinfo.php",
    "robots.txt",
    "security.txt",
    "sitemap.xml",
    "swagger.json",
    "web.config",
];

/// Extensions tried after each word by default (empty = folders only).
pub const DEFAULT_EXTENSIONS: &[&str] = &["php", "html", "txt", "bak", "zip", "json"];

/// What a discovery task may do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct DiscoveryConfig {
    /// Folder to start from (absolute http/https URL).
    pub base_url: String,
    /// Use the built-in names.
    pub builtin_words: bool,
    /// Extra names from the tester, one per entry.
    pub custom_words: Vec<String>,
    /// Also try names already seen in the site map for this host.
    pub site_words: bool,
    /// Extensions tried after each name (without the dot).
    pub extensions: Vec<String>,
    /// Try the well-known files in every folder.
    pub well_known_files: bool,
    /// Search folders that are found, down to `max_depth`.
    pub recursive: bool,
    /// Folder levels below the base searched when recursive.
    pub max_depth: u32,
    /// Requests sent at most.
    pub max_requests: u32,
    /// Requests in flight at once.
    pub concurrency: u32,
    /// Requests started per second (0 = no limit).
    pub requests_per_second: u32,
    /// Seconds to wait for each response.
    pub timeout_secs: u32,
    /// Send matching cookies from the project cookie jar.
    pub use_cookie_jar: bool,
    /// User-Agent header.
    pub user_agent: String,
}

impl Default for DiscoveryConfig {
    fn default() -> DiscoveryConfig {
        DiscoveryConfig {
            base_url: String::new(),
            builtin_words: true,
            custom_words: Vec::new(),
            site_words: true,
            extensions: DEFAULT_EXTENSIONS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            well_known_files: true,
            recursive: true,
            max_depth: 2,
            max_requests: 2000,
            concurrency: 4,
            requests_per_second: 10,
            timeout_secs: 15,
            use_cookie_jar: true,
            user_agent: "Mozilla/5.0 (compatible; ChosProxy Discovery)".into(),
        }
    }
}

fn valid_word(w: &str) -> bool {
    !w.is_empty()
        && w.len() <= 100
        && !w.contains("..")
        && !w.starts_with('/')
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.~/".contains(c))
}

impl DiscoveryConfig {
    /// Checks the settings; returns the base folder (ending in `/`).
    pub fn validate(&self) -> Result<Url, String> {
        let mut base =
            Url::parse(self.base_url.trim()).map_err(|_| "enter the base URL".to_string())?;
        if base.scheme() != "http" && base.scheme() != "https" {
            return Err("the base URL must be http or https".into());
        }
        base.set_query(None);
        base.set_fragment(None);
        if !base.path().ends_with('/') {
            let folder = match base.path().rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/"),
                None => "/".into(),
            };
            base.set_path(&folder);
        }
        let ranges = [
            (self.max_requests, 1, 100_000, "maximum requests"),
            (self.concurrency, 1, 32, "concurrency"),
            (self.requests_per_second, 0, 1000, "requests per second"),
            (self.timeout_secs, 1, 300, "timeout"),
            (self.max_depth, 0, 5, "depth"),
        ];
        for (v, min, max, name) in ranges {
            if !(min..=max).contains(&v) {
                return Err(format!("{name} must be {min}–{max}"));
            }
        }
        if self.custom_words.len() > 50_000 {
            return Err("at most 50 000 custom names".into());
        }
        if let Some(bad) = self
            .custom_words
            .iter()
            .find(|w| !valid_word(w.trim()) && !w.trim().is_empty())
        {
            return Err(format!(
                "invalid name {bad:?} (letters, digits and -_.~/ only)"
            ));
        }
        if let Some(bad) = self.extensions.iter().find(|e| {
            let e = e.trim().trim_start_matches('.');
            e.is_empty() || e.len() > 10 || !e.chars().all(|c| c.is_ascii_alphanumeric())
        }) {
            return Err(format!("invalid extension {bad:?}"));
        }
        if !self.builtin_words
            && !self.site_words
            && !self.well_known_files
            && self.custom_words.is_empty()
        {
            return Err("choose at least one source of names".into());
        }
        Ok(base)
    }

    /// The names to try (deduplicated, in a stable order).
    pub fn words(&self, site: &[String]) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        let mut add = |w: &str| {
            let w = w.trim().trim_matches('/');
            if valid_word(w) && seen.insert(w.to_string()) {
                out.push(w.to_string());
            }
        };
        if self.builtin_words {
            BUILTIN_WORDS.iter().for_each(|w| add(w));
        }
        self.custom_words.iter().for_each(|w| add(w));
        if self.site_words {
            site.iter().for_each(|w| add(w));
        }
        out
    }
}

/// Folder or file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "DiscoveryKind")]
pub enum Kind {
    /// A folder (`…/name/`).
    Folder,
    /// A file (`…/name.ext` or a well-known file).
    File,
}

/// One path to request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// URL.
    pub url: Url,
    /// Folder or file.
    pub kind: Kind,
    /// Folder levels below the base.
    pub depth: u32,
}

/// The candidates under `folder` (which ends in `/`).
pub fn candidates(
    folder: &Url,
    words: &[String],
    extensions: &[String],
    well_known: bool,
    depth: u32,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut push = |path: &str, kind: Kind| {
        if let Ok(url) = folder.join(path) {
            out.push(Candidate { url, kind, depth });
        }
    };
    if well_known {
        for f in BUILTIN_FILES {
            push(f, Kind::File);
        }
    }
    for w in words {
        if w.contains('.') {
            push(w, Kind::File);
            continue;
        }
        push(&format!("{w}/"), Kind::Folder);
        for e in extensions {
            push(
                &format!("{w}.{}", e.trim().trim_start_matches('.')),
                Kind::File,
            );
        }
    }
    out
}

fn random_name() -> String {
    use ring::rand::SecureRandom;
    let mut raw = [0u8; 12];
    let _ = ring::rand::SystemRandom::new().fill(&mut raw);
    raw.iter().map(|b| char::from(b'a' + b % 26)).collect()
}

/// Paths that cannot exist, used to learn the site's "not found" answers.
pub fn probes(folder: &Url, extensions: &[String]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for _ in 0..2 {
        if let Ok(url) = folder.join(&format!("{}/", random_name())) {
            out.push(Candidate {
                url,
                kind: Kind::Folder,
                depth: 0,
            });
        }
    }
    let ext = extensions
        .first()
        .map_or("html", |e| e.trim().trim_start_matches('.'));
    for e in [ext, "html"] {
        if let Ok(url) = folder.join(&format!("{}.{e}", random_name())) {
            out.push(Candidate {
                url,
                kind: Kind::File,
                depth: 0,
            });
        }
    }
    out
}

/// What a response looks like, independent of the name requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// Status code (0 = no response).
    pub status: u16,
    /// Body length.
    pub length: usize,
    /// Hash of the body with the requested name, digits and spacing removed.
    pub shape: u64,
    /// Redirect target with the requested name replaced.
    pub location: Option<String>,
}

fn last_segment(url: &Url) -> String {
    url.path()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

impl Fingerprint {
    /// The fingerprint of a response to `url`.
    pub fn of(url: &Url, status: u16, location: Option<&str>, body: &[u8]) -> Fingerprint {
        let name = last_segment(url);
        // Pages often echo the requested path: remove it, then the name.
        let text = String::from_utf8_lossy(body).replace(url.path(), "");
        let text = if name.is_empty() {
            text
        } else {
            text.replace(&name, "")
        };
        let normalized: String = text
            .chars()
            .filter(|c| !c.is_ascii_digit() && !c.is_whitespace())
            .collect();
        let digest = ring::digest::digest(&ring::digest::SHA256, normalized.as_bytes());
        let mut shape_bytes = [0u8; 8];
        shape_bytes.copy_from_slice(&digest.as_ref()[..8]);
        Fingerprint {
            status,
            length: body.len(),
            shape: u64::from_le_bytes(shape_bytes),
            location: location.map(|l| {
                if name.is_empty() {
                    l.to_string()
                } else {
                    l.replace(&name, "{}")
                }
            }),
        }
    }

    fn similar(&self, other: &Fingerprint) -> bool {
        if self.status != other.status || self.location != other.location {
            return false;
        }
        if self.shape == other.shape {
            return true;
        }
        // Large dynamic pages (ads, timestamps) may differ slightly; small
        // ones must have the same shape.
        let (a, b) = (self.length as f64, other.length as f64);
        a.max(b) >= 2000.0 && (a - b).abs() <= a.max(b) * 0.02
    }
}

/// The site's "not found" answers, per kind.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    folder: Vec<Fingerprint>,
    file: Vec<Fingerprint>,
}

/// How an answer was judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not there (404/410, or looks like the site's "not found" page).
    Missing,
    /// There (or protected: 401/403).
    Found,
    /// A folder redirect (`/name` → `/name/`): the folder exists.
    FolderRedirect,
}

impl Baseline {
    /// Adds a probe answer.
    pub fn learn(&mut self, kind: Kind, f: Fingerprint) {
        match kind {
            Kind::Folder => self.folder.push(f),
            Kind::File => self.file.push(f),
        }
    }

    /// Judges the answer `f` to a candidate.
    pub fn judge(&self, candidate: &Candidate, f: &Fingerprint) -> Verdict {
        if f.status == 0 || f.status == 404 || f.status == 410 || f.status >= 500 {
            return Verdict::Missing;
        }
        let known = match candidate.kind {
            Kind::Folder => &self.folder,
            Kind::File => &self.file,
        };
        if known.iter().any(|b| b.similar(f)) {
            return Verdict::Missing;
        }
        if (300..400).contains(&f.status)
            && candidate.kind == Kind::File
            && f.location.as_deref().is_some_and(|l| l.ends_with("{}/"))
        {
            return Verdict::FolderRedirect;
        }
        Verdict::Found
    }
}

/// Path segments of URLs (for `site_words`).
pub fn words_from_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out = BTreeSet::new();
    for p in paths {
        for seg in p.split(['/', '?', '#']) {
            let seg = seg.split('.').next().unwrap_or("");
            if seg.len() >= 3
                && seg.len() <= 40
                && valid_word(seg)
                && !seg.chars().all(|c| c.is_ascii_digit())
            {
                out.insert(seg.to_ascii_lowercase());
            }
            if out.len() >= 2000 {
                break;
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn validates_and_builds_candidates() {
        let config = DiscoveryConfig {
            base_url: "https://lab.test/app/page.php?x=1".into(),
            ..DiscoveryConfig::default()
        };
        let base = config.validate().unwrap();
        assert_eq!(base.as_str(), "https://lab.test/app/");
        let words = config.words(&["shop".into(), "../etc".into(), "admin".into()]);
        assert!(words.contains(&"shop".to_string()) && !words.iter().any(|w| w.contains("..")));
        assert_eq!(words.iter().filter(|w| *w == "admin").count(), 1);
        let c = candidates(
            &base,
            &["admin".into(), "notes.txt".into()],
            &["php".into()],
            true,
            0,
        );
        let urls: Vec<&str> = c.iter().map(|c| c.url.as_str()).collect();
        assert!(urls.contains(&"https://lab.test/app/admin/"));
        assert!(urls.contains(&"https://lab.test/app/admin.php"));
        assert!(urls.contains(&"https://lab.test/app/notes.txt"));
        assert!(urls.contains(&"https://lab.test/app/robots.txt"));
        assert!(
            DiscoveryConfig {
                custom_words: vec!["a b".into()],
                ..config.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            DiscoveryConfig {
                base_url: "ftp://x/".into(),
                ..config
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn soft_404_pages_are_not_hits() {
        let base = Url::parse("http://lab.test/").unwrap();
        let mut baseline = Baseline::default();
        // The site answers 200 "Page /xyz not found" for everything.
        for p in probes(&base, &["php".into()]) {
            let body = format!("<h1>Page {} not found</h1> id 4711", p.url.path());
            baseline.learn(p.kind, Fingerprint::of(&p.url, 200, None, body.as_bytes()));
        }
        let missing = Candidate {
            url: base.join("admin/").unwrap(),
            kind: Kind::Folder,
            depth: 0,
        };
        let soft = Fingerprint::of(
            &missing.url,
            200,
            None,
            b"<h1>Page /admin/ not found</h1> id 4712",
        );
        assert_eq!(baseline.judge(&missing, &soft), Verdict::Missing);
        let real = Fingerprint::of(
            &missing.url,
            200,
            None,
            b"<h1>Administration</h1><form>...</form>",
        );
        assert_eq!(baseline.judge(&missing, &real), Verdict::Found);
        assert_eq!(
            baseline.judge(&missing, &Fingerprint::of(&missing.url, 404, None, b"")),
            Verdict::Missing
        );
        assert_eq!(
            baseline.judge(&missing, &Fingerprint::of(&missing.url, 403, None, b"no")),
            Verdict::Found
        );
        let file = Candidate {
            url: base.join("admin").unwrap(),
            kind: Kind::File,
            depth: 0,
        };
        let redirect = Fingerprint::of(&file.url, 301, Some("http://lab.test/admin/"), b"");
        assert_eq!(baseline.judge(&file, &redirect), Verdict::FolderRedirect);
    }

    #[test]
    fn words_from_site_paths() {
        let words =
            words_from_paths(["/shop/items/42", "/api/v1/users?id=1", "/a/b.js"].into_iter());
        assert!(words.contains(&"shop".to_string()) && words.contains(&"users".to_string()));
        assert!(!words.contains(&"42".to_string()) && !words.contains(&"a".to_string()));
    }
}
