//! The URLs waiting to be fetched, breadth first, each fetched once.

use crate::discover::FoundKind;
use std::collections::{HashMap, HashSet, VecDeque};
use url::Url;

/// Most URLs remembered (queued or seen) by one crawl.
const MAX_KNOWN: usize = 200_000;

/// File extensions a crawler does not fetch (no links inside, or binary).
const STATIC: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "ico", "webp", "bmp", "tif", "tiff", "avif", "woff",
    "woff2", "ttf", "otf", "eot", "css", "pdf", "zip", "gz", "tgz", "tar", "rar", "7z", "bz2",
    "mp3", "mp4", "m4a", "avi", "mov", "webm", "ogg", "wav", "flac", "exe", "dmg", "iso", "msi",
    "apk", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "map",
];

/// A parsed http(s) URL without its fragment, or `None`.
pub fn normalize(text: &str) -> Option<Url> {
    let mut url = Url::parse(text.trim()).ok()?;
    if (url.scheme() != "http" && url.scheme() != "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    Some(url)
}

/// A URL to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What to fetch.
    pub url: Url,
    /// Links followed from a start URL to reach it.
    pub depth: u32,
    /// How it was found.
    pub kind: FoundKind,
    /// The stored exchange of the page it was found on (none for start
    /// URLs and the robots.txt/sitemap of a start site).
    pub via: Option<i64>,
}

/// Why a URL was or was not queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// Queued.
    Added,
    /// Already queued or fetched.
    Seen,
    /// Beyond the maximum depth.
    TooDeep,
    /// Its path or query contains a word to avoid.
    Avoided,
    /// Outside the crawl's scope.
    OutOfScope,
    /// Its path already had enough different query strings.
    TooManyVariants,
    /// Enough URLs of the same structural pattern were queued already.
    Similar,
    /// A static file (image, font, archive…).
    Static,
    /// The crawl remembers too many URLs already.
    Full,
}

/// Breadth-first queue with de-duplication and limits.
#[derive(Debug)]
pub struct Frontier {
    queue: VecDeque<Entry>,
    known: HashSet<String>,
    variants: HashMap<String, u32>,
    similar: HashMap<String, u32>,
    max_depth: u32,
    max_variants: u32,
    max_similar: u32,
    avoid: Vec<String>,
}

/// The structural pattern of a URL (see `chosproxy_scanner::fingerprint`).
pub fn pattern(url: &Url) -> String {
    let origin = format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or(0)
    );
    chosproxy_scanner::fingerprint::url_fingerprint("GET", &origin, url.path(), url.query())
}

fn path_key(url: &Url) -> String {
    format!(
        "{}://{}:{}{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or(0),
        url.path()
    )
}

fn is_static(url: &Url) -> bool {
    let last = url.path().rsplit('/').next().unwrap_or_default();
    match last.rsplit_once('.') {
        Some((_, ext)) => STATIC.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

impl Frontier {
    /// An empty frontier.
    pub fn new(max_depth: u32, max_variants: u32, avoid: &[String]) -> Frontier {
        Frontier {
            queue: VecDeque::new(),
            known: HashSet::new(),
            variants: HashMap::new(),
            similar: HashMap::new(),
            max_depth,
            max_variants: max_variants.max(1),
            max_similar: 0,
            avoid: avoid
                .iter()
                .map(|w| w.trim().to_ascii_lowercase())
                .filter(|w| !w.is_empty())
                .collect(),
        }
    }

    /// Fetches at most `limit` URLs per structural pattern (0 = no limit).
    pub fn with_max_similar(mut self, limit: u32) -> Frontier {
        self.max_similar = limit;
        self
    }

    /// Queues a start URL (depth 0; words to avoid do not apply: the tester
    /// chose it).
    pub fn seed(&mut self, url: Url) -> Push {
        self.add(url, 0, FoundKind::Link, None, false)
    }

    /// Queues a found URL if it is new, allowed and within the limits.
    pub fn push(
        &mut self,
        url: Url,
        depth: u32,
        kind: FoundKind,
        allowed: impl Fn(&Url) -> bool,
    ) -> Push {
        self.push_via(url, depth, kind, None, allowed)
    }

    /// The same, remembering the exchange of the page it was found on.
    pub fn push_via(
        &mut self,
        url: Url,
        depth: u32,
        kind: FoundKind,
        via: Option<i64>,
        allowed: impl Fn(&Url) -> bool,
    ) -> Push {
        if !allowed(&url) {
            return Push::OutOfScope;
        }
        self.add(url, depth, kind, via, true)
    }

    fn add(
        &mut self,
        mut url: Url,
        depth: u32,
        kind: FoundKind,
        via: Option<i64>,
        check_avoid: bool,
    ) -> Push {
        url.set_fragment(None);
        let key = url.to_string();
        if self.known.contains(&key) {
            return Push::Seen;
        }
        if depth > self.max_depth {
            return Push::TooDeep;
        }
        if is_static(&url) {
            return Push::Static;
        }
        if check_avoid {
            let target =
                format!("{}?{}", url.path(), url.query().unwrap_or_default()).to_ascii_lowercase();
            if self.avoid.iter().any(|w| target.contains(w.as_str())) {
                return Push::Avoided;
            }
        }
        if self.known.len() >= MAX_KNOWN {
            return Push::Full;
        }
        if url.query().is_some() {
            let count = self.variants.entry(path_key(&url)).or_insert(0);
            if *count >= self.max_variants {
                return Push::TooManyVariants;
            }
            *count += 1;
        }
        if self.max_similar > 0 {
            // Seeds count towards their pattern but are always fetched.
            let count = self.similar.entry(pattern(&url)).or_insert(0);
            if *count >= self.max_similar && check_avoid {
                return Push::Similar;
            }
            *count += 1;
        }
        self.known.insert(key);
        self.queue.push_back(Entry {
            url,
            depth,
            kind,
            via,
        });
        Push::Added
    }

    /// The next URL, oldest first.
    pub fn pop(&mut self) -> Option<Entry> {
        self.queue.pop_front()
    }

    /// URLs waiting.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// URLs queued so far (fetched or waiting).
    pub fn known(&self) -> usize {
        self.known.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        normalize(s).unwrap()
    }

    #[test]
    fn deduplicates_limits_and_avoids() {
        let mut f = Frontier::new(2, 2, &["logout".to_string(), " ".to_string()]);
        let any = |_: &Url| true;
        assert_eq!(
            f.seed(u("http://lab.test/logout")),
            Push::Added,
            "seeds are trusted"
        );
        assert_eq!(
            f.push(u("http://lab.test/a#x"), 1, FoundKind::Link, any),
            Push::Added
        );
        assert_eq!(
            f.push(u("http://lab.test/a"), 1, FoundKind::Link, any),
            Push::Seen
        );
        assert_eq!(
            f.push(u("http://lab.test/b"), 3, FoundKind::Link, any),
            Push::TooDeep
        );
        assert_eq!(
            f.push(u("http://lab.test/account/LOGOUT"), 1, FoundKind::Link, any),
            Push::Avoided
        );
        assert_eq!(
            f.push(u("http://lab.test/logo.PNG"), 1, FoundKind::Link, any),
            Push::Static
        );
        assert_eq!(
            f.push(u("http://other.test/"), 1, FoundKind::Link, |x: &Url| x
                .host_str()
                == Some("lab.test")),
            Push::OutOfScope
        );
        for (i, expected) in [Push::Added, Push::Added, Push::TooManyVariants]
            .into_iter()
            .enumerate()
        {
            let url = u(&format!("http://lab.test/item?id={i}"));
            assert_eq!(f.push(url, 1, FoundKind::Link, any), expected);
        }
        assert_eq!((f.queued(), f.known()), (4, 4));
        assert_eq!(f.pop().unwrap().url.as_str(), "http://lab.test/logout");
        assert_eq!(f.pop().unwrap().url.as_str(), "http://lab.test/a");
    }

    #[test]
    fn normalizes_urls() {
        assert_eq!(u("HTTP://Lab.Test:80/x#y").as_str(), "http://lab.test/x");
        assert!(normalize("ftp://lab.test/").is_none());
        assert!(normalize("/relative").is_none());
    }

    #[test]
    fn skips_urls_of_a_pattern_seen_often_enough() {
        let any = |_: &Url| true;
        let mut f = Frontier::new(5, 10, &[]).with_max_similar(2);
        assert_eq!(f.seed(u("http://lab.test/item/1")), Push::Added);
        assert_eq!(
            f.push(u("http://lab.test/item/2"), 1, FoundKind::Link, any),
            Push::Added
        );
        assert_eq!(
            f.push(u("http://lab.test/item/3"), 1, FoundKind::Link, any),
            Push::Similar
        );
        assert_eq!(
            f.push(u("http://lab.test/item/4?x=1"), 1, FoundKind::Link, any),
            Push::Added
        );
        assert_eq!(
            f.push(u("http://lab.test/other"), 1, FoundKind::Link, any),
            Push::Added
        );
        // Seeds are always fetched.
        assert_eq!(f.seed(u("http://lab.test/item/5")), Push::Added);
        let mut unlimited = Frontier::new(5, 10, &[]);
        for i in 0..50 {
            let url = u(&format!("http://lab.test/item/{i}"));
            assert_eq!(unlimited.push(url, 1, FoundKind::Link, any), Push::Added);
        }
    }
}
