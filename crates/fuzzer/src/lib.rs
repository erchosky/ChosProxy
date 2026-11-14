//! The fuzzer's pure parts (spec §25): payload positions in a request
//! ([`Template`]), payload sets and their processing, attack modes
//! ([`Plan`]) and grep match/extract ([`Grep`]). Sending, storage and the
//! task live in the engine.
//!
//! The fuzzer is for authorized, repetitive testing of inputs. It ships no
//! lists of real credentials and has no features aimed at password spraying
//! or credential stuffing.

mod attack;
mod grep;
mod payloads;
mod template;

pub use attack::{AttackMode, Attempt, Plan};
pub use grep::{Grep, MAX_GREP};
pub use payloads::{
    BUILTIN, BuiltinList, MAX_PAYLOAD_BYTES, MAX_SET_SIZE, PayloadSet, PayloadSource, Processing,
    builtin,
};
pub use template::{MARKER, MAX_POSITIONS, Template, clear_marks};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most requests one attack may send.
pub const MAX_REQUESTS: u32 = 200_000;

/// Where the attack sends its requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FuzzTarget {
    /// `http` or `https`.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
}

/// Everything an attack needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct FuzzConfig {
    /// Target.
    pub target: FuzzTarget,
    /// Request text (`\n` line endings) with `§…§` positions.
    pub template: String,
    /// Send the head with CRLF line endings.
    pub head_crlf: bool,
    /// Send the body with CRLF line endings.
    pub body_crlf: bool,
    /// Attack mode.
    pub mode: AttackMode,
    /// Payload sets (one, or one per position).
    pub payload_sets: Vec<PayloadSet>,
    /// Recompute Content-Length after inserting payloads.
    pub update_content_length: bool,
    /// Add cookies from the project cookie jar.
    pub use_cookie_jar: bool,
    /// Requests in flight at once.
    pub concurrency: u32,
    /// Requests started per second (0 = no limit).
    pub requests_per_second: u32,
    /// Seconds to wait for each response.
    pub timeout_secs: u32,
    /// Times a request is retried when no response arrives (0–3).
    pub retries: u32,
    /// Refuse attacks larger than this.
    pub max_requests: u32,
    /// Texts (or regexes) flagged in each response.
    pub grep_match: Vec<String>,
    /// `grep_match` entries are regular expressions.
    pub grep_regex: bool,
    /// Match case.
    pub grep_case_sensitive: bool,
    /// Regexes whose first group is extracted from each response.
    pub grep_extract: Vec<String>,
    /// When the attack stops before sending everything.
    pub stop: StopConditions,
}

/// When an attack stops early. Every limit at 0 (or an empty list) is off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export, rename = "FuzzStopConditions")]
pub struct StopConditions {
    /// Stop once this many responses matched a grep term.
    pub after_matches: u32,
    /// Stop once this many requests got no response.
    pub after_errors: u32,
    /// Stop when a response has one of these status codes.
    pub on_status: Vec<u16>,
}

impl StopConditions {
    /// Most status codes in `on_status`.
    pub const MAX_STATUSES: usize = 20;

    fn validate(&self) -> Result<(), String> {
        if self.on_status.len() > Self::MAX_STATUSES {
            return Err(format!("at most {} stop status codes", Self::MAX_STATUSES));
        }
        if let Some(bad) = self.on_status.iter().find(|s| !(100..=599).contains(*s)) {
            return Err(format!("{bad} is not an HTTP status code (100–599)"));
        }
        Ok(())
    }

    /// Why the attack must stop now, given the counts so far and the status
    /// of the response just received; `None` to go on.
    pub fn reason(&self, matched: u64, errors: u64, status: Option<u16>) -> Option<String> {
        if let Some(s) = status.filter(|s| self.on_status.contains(s)) {
            return Some(format!("a response had status {s}"));
        }
        if self.after_matches > 0 && matched >= u64::from(self.after_matches) {
            return Some(format!("{matched} response(s) matched"));
        }
        if self.after_errors > 0 && errors >= u64::from(self.after_errors) {
            return Some(format!("{errors} request(s) got no response"));
        }
        None
    }
}

impl Default for FuzzConfig {
    fn default() -> FuzzConfig {
        FuzzConfig {
            target: FuzzTarget {
                scheme: "http".into(),
                host: "localhost".into(),
                port: 80,
            },
            template: String::new(),
            head_crlf: true,
            body_crlf: true,
            mode: AttackMode::Sniper,
            payload_sets: Vec::new(),
            update_content_length: true,
            use_cookie_jar: false,
            concurrency: 4,
            requests_per_second: 10,
            timeout_secs: 15,
            retries: 1,
            max_requests: 10_000,
            grep_match: Vec::new(),
            grep_regex: false,
            grep_case_sensitive: false,
            grep_extract: Vec::new(),
            stop: StopConditions::default(),
        }
    }
}

/// A checked attack: its plan and its grep.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Requests and payloads.
    pub plan: Plan,
    /// Match/extract.
    pub grep: Grep,
}

impl FuzzConfig {
    /// Checks the settings and builds the plan (payload sets are generated
    /// here, so errors such as an invalid number range appear before
    /// anything is sent).
    pub fn prepare(&self) -> Result<Prepared, String> {
        self.prepare_with(&payloads::no_scripts)
    }

    /// The same, running payload scripts through `scripts`.
    pub fn prepare_with(&self, scripts: payloads::ScriptRunner<'_>) -> Result<Prepared, String> {
        let ranges: [(&str, u32, u32, u32); 5] = [
            ("concurrency", self.concurrency, 1, 32),
            ("requests per second", self.requests_per_second, 0, 500),
            ("timeout", self.timeout_secs, 1, 300),
            ("retries", self.retries, 0, 3),
            ("maximum requests", self.max_requests, 1, MAX_REQUESTS),
        ];
        for (name, value, min, max) in ranges {
            if !(min..=max).contains(&value) {
                return Err(format!("the {name} must be {min}–{max}"));
            }
        }
        self.stop.validate()?;
        if self.template.trim().is_empty() {
            return Err("the request is empty".into());
        }
        let template = Template::parse(&self.template)?;
        if template.positions() == 0 {
            return Err("mark at least one payload position with §…§".into());
        }
        let needed = self.mode.sets_needed(template.positions());
        if self.payload_sets.len() < needed {
            return Err(format!(
                "this attack mode needs {needed} payload set(s); {} configured",
                self.payload_sets.len()
            ));
        }
        let sets = self
            .payload_sets
            .iter()
            .take(needed)
            .enumerate()
            .map(|(i, s)| {
                s.values_with(scripts)
                    .map_err(|e| format!("payload set {}: {e}", i + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let plan = Plan::new(template, self.mode, sets)?;
        if plan.count() > u64::from(self.max_requests) {
            return Err(format!(
                "the attack would send {} requests, more than the limit of {}; reduce the payloads or raise the limit",
                plan.count(),
                self.max_requests
            ));
        }
        let grep = Grep::new(
            &self.grep_match,
            self.grep_regex,
            self.grep_case_sensitive,
            &self.grep_extract,
        )?;
        Ok(Prepared { plan, grep })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn stop_conditions() {
        let off = StopConditions::default();
        assert_eq!(off.reason(1_000, 1_000, Some(500)), None);
        let stop = StopConditions {
            after_matches: 2,
            after_errors: 3,
            on_status: vec![429, 503],
        };
        assert_eq!(stop.reason(1, 2, Some(200)), None);
        assert!(stop.reason(0, 0, Some(429)).unwrap().contains("429"));
        assert!(stop.reason(2, 0, Some(200)).unwrap().contains("matched"));
        assert!(stop.reason(0, 3, None).unwrap().contains("no response"));
        assert!(
            StopConditions {
                on_status: vec![42],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            StopConditions {
                on_status: vec![200; 21],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn prepare_checks_everything_before_sending() {
        let mut c = FuzzConfig {
            template: "GET /item?id=§1§ HTTP/1.1\nHost: lab\n\n".into(),
            payload_sets: vec![PayloadSet {
                source: PayloadSource::Numbers {
                    from: 1,
                    to: 50,
                    step: 1,
                    min_digits: 0,
                },
                processing: vec![],
            }],
            grep_match: vec!["error".into()],
            ..FuzzConfig::default()
        };
        let p = c.prepare().unwrap();
        assert_eq!(p.plan.count(), 50);
        assert!(
            p.plan
                .render(&p.plan.attempt(49).unwrap())
                .starts_with("GET /item?id=50 ")
        );

        c.max_requests = 10;
        assert!(c.prepare().unwrap_err().contains("50 requests"));
        c.max_requests = 100;
        c.mode = AttackMode::ClusterBomb;
        c.template = "§a§§b§".into();
        assert!(c.prepare().unwrap_err().contains("2 payload set"));
        c.mode = AttackMode::Sniper;
        c.template = "GET / HTTP/1.1\n\n".into();
        assert!(c.prepare().unwrap_err().contains("payload position"));
        c.template = "GET /§a§ HTTP/1.1\n\n".into();
        c.grep_extract = vec!["(".into()];
        assert!(c.prepare().unwrap_err().contains("invalid pattern"));
        c.grep_extract.clear();
        c.concurrency = 0;
        assert!(c.prepare().unwrap_err().contains("concurrency"));
    }
}
