//! Payload positions: a request text with `§default§` markers.

/// The marker around a payload position.
pub const MARKER: char = '§';

/// Most positions in one request.
pub const MAX_POSITIONS: usize = 20;

/// A request text split at its positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Text around the positions (one more than positions).
    literals: Vec<String>,
    /// The text between each pair of markers (the original value).
    defaults: Vec<String>,
}

impl Template {
    /// Reads `§`-marked text. Markers must come in pairs.
    pub fn parse(text: &str) -> Result<Template, String> {
        let pieces: Vec<&str> = text.split(MARKER).collect();
        if pieces.len().is_multiple_of(2) {
            return Err(
                "the § markers are not balanced: each position needs an opening and a closing §"
                    .into(),
            );
        }
        let positions = pieces.len() / 2;
        if positions > MAX_POSITIONS {
            return Err(format!("at most {MAX_POSITIONS} positions"));
        }
        let mut literals = Vec::with_capacity(positions + 1);
        let mut defaults = Vec::with_capacity(positions);
        for (i, piece) in pieces.into_iter().enumerate() {
            if i % 2 == 0 {
                literals.push(piece.to_string());
            } else {
                defaults.push(piece.to_string());
            }
        }
        Ok(Template { literals, defaults })
    }

    /// Number of positions.
    pub fn positions(&self) -> usize {
        self.defaults.len()
    }

    /// The original value of each position.
    pub fn defaults(&self) -> &[String] {
        &self.defaults
    }

    /// The text with `values[i]` in position `i` (missing values keep the
    /// default).
    pub fn render(&self, values: &[String]) -> String {
        let mut out = String::new();
        for (i, literal) in self.literals.iter().enumerate() {
            out.push_str(literal);
            if let Some(default) = self.defaults.get(i) {
                out.push_str(values.get(i).unwrap_or(default));
            }
        }
        out
    }
}

/// The text without markers (the request as it was).
pub fn clear_marks(text: &str) -> String {
    text.replace(MARKER, "")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_positions() {
        let t = Template::parse("GET /?id=§1§&q=§a b§ HTTP/1.1\n\n").unwrap();
        assert_eq!(t.positions(), 2);
        assert_eq!(t.defaults(), ["1", "a b"]);
        assert_eq!(
            t.render(&["7".into()]),
            "GET /?id=7&q=a b HTTP/1.1\n\n",
            "missing values keep the default"
        );
        assert_eq!(Template::parse("no markers").unwrap().positions(), 0);
        assert!(Template::parse("a § b").unwrap_err().contains("balanced"));
        assert!(Template::parse(&"§x§".repeat(21)).is_err());
        assert_eq!(clear_marks("a=§1§"), "a=1");
    }
}
