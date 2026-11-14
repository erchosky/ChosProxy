//! Which part of the application produced a piece of traffic.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Origin of a stored exchange. Every automated request must carry the module
/// that produced it so the user can always tell manual from automated traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Source {
    /// Traffic relayed by a proxy listener.
    Proxy,
    /// Sent manually from Repeater.
    Repeater,
    /// Sent by the fuzzer.
    Fuzzer,
    /// Sent by the crawler.
    Crawler,
    /// Sent by the scanner.
    Scanner,
    /// Sent by a sequencer capture.
    Sequencer,
    /// Sent by a plugin.
    Plugin,
    /// Sent by content discovery.
    Discovery,
    /// Sent by ChosAgent (each request approved by the tester).
    Agent,
}

impl Source {
    /// Stable lowercase identifier (used in storage).
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Proxy => "proxy",
            Source::Repeater => "repeater",
            Source::Fuzzer => "fuzzer",
            Source::Crawler => "crawler",
            Source::Scanner => "scanner",
            Source::Sequencer => "sequencer",
            Source::Plugin => "plugin",
            Source::Discovery => "discovery",
            Source::Agent => "agent",
        }
    }

    /// Parses the identifier produced by [`Source::as_str`].
    pub fn parse(s: &str) -> Option<Source> {
        Some(match s {
            "proxy" => Source::Proxy,
            "repeater" => Source::Repeater,
            "fuzzer" => Source::Fuzzer,
            "crawler" => Source::Crawler,
            "scanner" => Source::Scanner,
            "sequencer" => Source::Sequencer,
            "plugin" => Source::Plugin,
            "discovery" => Source::Discovery,
            "agent" => Source::Agent,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for s in [
            Source::Proxy,
            Source::Repeater,
            Source::Fuzzer,
            Source::Crawler,
            Source::Scanner,
            Source::Sequencer,
            Source::Plugin,
            Source::Discovery,
            Source::Agent,
        ] {
            assert_eq!(Source::parse(s.as_str()), Some(s));
        }
        assert_eq!(Source::parse("nope"), None);
    }
}
