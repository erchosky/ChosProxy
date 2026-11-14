//! Grep match and extract: flag responses containing given text and pull
//! values out of them, one column each in the results.

use regex::{Regex, RegexBuilder};

/// Most match terms and extract patterns.
pub const MAX_GREP: usize = 20;
/// Longest extracted value kept.
const MAX_EXTRACT: usize = 200;

/// Compiled match terms and extract patterns.
#[derive(Debug, Clone)]
pub struct Grep {
    matches: Vec<Regex>,
    extracts: Vec<Regex>,
}

impl Grep {
    /// `matches` are plain texts (or regexes with `regex`); `extracts` are
    /// regexes whose first group (or whole match) is extracted.
    pub fn new(
        matches: &[String],
        regex: bool,
        case_sensitive: bool,
        extracts: &[String],
    ) -> Result<Grep, String> {
        if matches.len() > MAX_GREP || extracts.len() > MAX_GREP {
            return Err(format!(
                "at most {MAX_GREP} match terms and {MAX_GREP} extract patterns"
            ));
        }
        let build = |pattern: &str, case: bool| {
            RegexBuilder::new(pattern)
                .case_insensitive(!case)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| format!("invalid pattern {pattern:?}: {e}"))
        };
        let matches = matches
            .iter()
            .filter(|m| !m.is_empty())
            .map(|m| {
                if regex {
                    build(m, case_sensitive)
                } else {
                    build(&regex::escape(m), case_sensitive)
                }
            })
            .collect::<Result<_, _>>()?;
        let extracts = extracts
            .iter()
            .filter(|e| !e.is_empty())
            .map(|e| build(e, true))
            .collect::<Result<_, _>>()?;
        Ok(Grep { matches, extracts })
    }

    /// Whether `text` contains each match term.
    pub fn matches(&self, text: &str) -> Vec<bool> {
        self.matches.iter().map(|r| r.is_match(text)).collect()
    }

    /// The value of each extract pattern in `text` ("" when not found).
    pub fn extracts(&self, text: &str) -> Vec<String> {
        self.extracts
            .iter()
            .map(|r| {
                r.captures(text)
                    .and_then(|c| c.get(1).or_else(|| c.get(0)))
                    .map(|m| m.as_str().chars().take(MAX_EXTRACT).collect())
                    .unwrap_or_default()
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn match_and_extract() {
        let g = Grep::new(
            &["error".into(), "a.b".into(), String::new()],
            false,
            false,
            &[r"<title>(.*?)</title>".into(), r"id=\d+".into()],
        )
        .unwrap();
        let text = "<title>Login ERROR</title> id=42 axb";
        assert_eq!(
            g.matches(text),
            [true, false],
            "plain text, case-insensitive; dots are literal"
        );
        assert_eq!(g.extracts(text), ["Login ERROR", "id=42"]);
        assert_eq!(g.extracts("nothing"), ["", ""]);

        let r = Grep::new(&["a.b".into()], true, true, &[]).unwrap();
        assert_eq!(r.matches("axb"), [true]);
        assert!(Grep::new(&["(".into()], true, false, &[]).is_err());
        assert!(Grep::new(&[], false, false, &["(".into()]).is_err());
    }
}
