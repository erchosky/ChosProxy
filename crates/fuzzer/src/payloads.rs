//! Payload sets: where the values come from and how they are processed.
//!
//! No list of real credentials is shipped (spec §25): the built-in lists are
//! generic test strings; anything else is the tester's own list.

use chosproxy_decoder::{Operation, apply};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most payloads one set may produce.
pub const MAX_SET_SIZE: usize = 200_000;
/// Longest single payload.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// Where a set's payloads come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "FuzzPayloadSource")]
pub enum PayloadSource {
    /// The tester's own values (typed, pasted or loaded from a file).
    List {
        /// Values, in order.
        items: Vec<String>,
    },
    /// Numbers from `from` to `to` (inclusive) by `step`.
    Numbers {
        /// First number.
        #[ts(type = "number")]
        from: i64,
        /// Last number (inclusive).
        #[ts(type = "number")]
        to: i64,
        /// Step (not 0; negative counts down).
        #[ts(type = "number")]
        step: i64,
        /// Pad with zeros to at least this many digits.
        min_digits: u32,
    },
    /// Every string of `min_length`–`max_length` characters from `charset`.
    Characters {
        /// Characters to combine.
        charset: String,
        /// Shortest string.
        min_length: u32,
        /// Longest string.
        max_length: u32,
    },
    /// Dates from `from` to `to` (inclusive, `YYYY-MM-DD`) every
    /// `step_days` days, written with `format` (`yyyy`, `yy`, `MM`, `M`,
    /// `dd`, `d`, `MMM`; anything else is copied).
    Dates {
        /// First date, `YYYY-MM-DD`.
        from: String,
        /// Last date, `YYYY-MM-DD`.
        to: String,
        /// Days between dates (1 or more).
        step_days: u32,
        /// How each date is written, e.g. `dd/MM/yyyy`.
        format: String,
    },
    /// One of the lists that come with ChosProxy (by id).
    Builtin {
        /// List id.
        name: String,
    },
}

/// A change applied to every payload of a set, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "FuzzProcessing")]
pub enum Processing {
    /// Text added before.
    Prefix {
        /// Text.
        text: String,
    },
    /// Text added after.
    Suffix {
        /// Text.
        text: String,
    },
    /// Every occurrence of `from` replaced by `to`.
    Replace {
        /// Text to find.
        from: String,
        /// Replacement.
        to: String,
    },
    /// Lower case.
    Lowercase,
    /// Upper case.
    Uppercase,
    /// A Decoder operation (URL/Base64/hex encoding, hashes…).
    Encode {
        /// The operation.
        operation: Operation,
    },
    /// A payload script of the library (`fn payload(p)`), by id.
    Script {
        /// Script id.
        script: String,
    },
}

/// Runs a payload script: `(script id, payload) → processed payload`.
pub type ScriptRunner<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;

/// A runner for places without scripts.
pub fn no_scripts(_: &str, _: &str) -> Result<String, String> {
    Err("payload scripts are only available in the Fuzzer".into())
}

/// One payload set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "FuzzPayloadSet")]
pub struct PayloadSet {
    /// Source of the values.
    pub source: PayloadSource,
    /// Changes applied to each value, in order.
    #[serde(default)]
    pub processing: Vec<Processing>,
}

/// A list that comes with ChosProxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "FuzzBuiltinList")]
pub struct BuiltinList {
    /// Id used in [`PayloadSource::Builtin`].
    pub id: &'static str,
    /// Name shown.
    pub name: &'static str,
    /// What it is for.
    pub description: &'static str,
    /// Values.
    pub items: &'static [&'static str],
}

/// The built-in lists: generic test inputs, no credentials.
pub static BUILTIN: &[BuiltinList] = &[
    BuiltinList {
        id: "special-characters",
        name: "Special characters",
        description: "Characters that often break parsers, templates and queries.",
        items: &[
            "'", "\"", "`", "<", ">", "&", ";", "|", "\\", "/", "%", "#", "{", "}", "(", ")", "[",
            "]", "$", "*", "~", "!", "?", "=", "+", ",", ":", "@", "^", "%00", "%0a", "%0d%0a",
        ],
    },
    BuiltinList {
        id: "boundary-values",
        name: "Boundary values",
        description: "Empty, very long, negative, huge and unusual values for input validation.",
        items: &[
            "",
            " ",
            "0",
            "-1",
            "1",
            "2147483647",
            "2147483648",
            "-2147483649",
            "9223372036854775808",
            "1e309",
            "NaN",
            "null",
            "undefined",
            "true",
            "false",
            "[]",
            "{}",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "ü€😀",
            "%ff%fe",
        ],
    },
    BuiltinList {
        id: "quick-probes",
        name: "Quick probes",
        description: "Harmless strings that reveal injection points through errors or reflection \
                      (quotes, template expressions, markup, path traversal).",
        items: &[
            "'",
            "''",
            "\"",
            "' OR '1'='1",
            "1 AND 1=2",
            "1'\"",
            "${7*7}",
            "{{7*7}}",
            "<%= 7*7 %>",
            "#{7*7}",
            "<chosproxy>",
            "\"><chosproxy>",
            "javascript:chosproxy",
            "../../../../etc/hosts",
            "..%2f..%2f..%2fetc%2fhosts",
            "%00",
            "|echo chosproxy",
            ";echo chosproxy",
        ],
    },
];

/// The built-in list with this id.
pub fn builtin(id: &str) -> Option<&'static BuiltinList> {
    BUILTIN.iter().find(|l| l.id == id)
}

fn too_many() -> String {
    format!("a payload set may hold at most {MAX_SET_SIZE} values")
}

fn numbers(from: i64, to: i64, step: i64, min_digits: u32) -> Result<Vec<String>, String> {
    if step == 0 || (to > from && step < 0) || (to < from && step > 0) {
        return Err(
            "the step must be non-zero and go from the first number towards the last".into(),
        );
    }
    let count = (i128::from(to) - i128::from(from)) / i128::from(step) + 1;
    if count > MAX_SET_SIZE as i128 {
        return Err(too_many());
    }
    let width = usize::try_from(min_digits.min(30)).unwrap_or(0);
    let mut out = Vec::new();
    let mut n = i128::from(from);
    for _ in 0..count {
        let digits = format!("{:0width$}", n.unsigned_abs());
        out.push(if n < 0 { format!("-{digits}") } else { digits });
        n += i128::from(step);
    }
    Ok(out)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day number (inverse of `days_from_civil`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn parse_date(text: &str) -> Result<i64, String> {
    let bad = || format!("{text:?} is not a date like 2024-02-29");
    let mut parts = text.trim().splitn(3, '-');
    let mut next =
        || -> Result<i64, String> { parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad) };
    let (y, m, d) = (next()?, next()?, next()?);
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    let days = days_from_civil(y, m, d);
    // Rejects 2023-02-30 and the like.
    if civil_from_days(days) != (y, m, d) {
        return Err(bad());
    }
    Ok(days)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn format_date(format: &str, (y, m, d): (i64, i64, i64)) -> String {
    let mut out = String::new();
    let mut rest = format;
    while !rest.is_empty() {
        let tokens: [(&str, String); 7] = [
            ("yyyy", format!("{y:04}")),
            (
                "MMM",
                MONTHS[usize::try_from(m - 1).unwrap_or(0) % 12].to_string(),
            ),
            ("yy", format!("{:02}", y % 100)),
            ("MM", format!("{m:02}")),
            ("dd", format!("{d:02}")),
            ("M", m.to_string()),
            ("d", d.to_string()),
        ];
        match tokens.iter().find(|(t, _)| rest.starts_with(t)) {
            Some((token, value)) => {
                out.push_str(value);
                rest = &rest[token.len()..];
            }
            None => {
                let c = rest.chars().next().unwrap_or_default();
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

fn dates(from: &str, to: &str, step_days: u32, format: &str) -> Result<Vec<String>, String> {
    let (start, end) = (parse_date(from)?, parse_date(to)?);
    if step_days == 0 {
        return Err("the step must be at least one day".into());
    }
    if end < start {
        return Err("the last date comes before the first".into());
    }
    if format.trim().is_empty() || format.len() > 100 {
        return Err("give a date format such as yyyy-MM-dd".into());
    }
    let count = (end - start) / i64::from(step_days) + 1;
    if count > MAX_SET_SIZE as i64 {
        return Err(too_many());
    }
    Ok((0..count)
        .map(|i| format_date(format, civil_from_days(start + i * i64::from(step_days))))
        .collect())
}

fn characters(charset: &str, min: u32, max: u32) -> Result<Vec<String>, String> {
    let chars: Vec<char> = {
        let mut seen = Vec::new();
        for c in charset.chars() {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        seen
    };
    if chars.is_empty() || min == 0 || min > max || max > 8 {
        return Err("use a non-empty character set and lengths from 1 to 8".into());
    }
    let mut total: u128 = 0;
    for len in min..=max {
        total += (chars.len() as u128).pow(len);
    }
    if total > MAX_SET_SIZE as u128 {
        return Err(too_many());
    }
    let mut out = Vec::new();
    for len in min..=max {
        let len = len as usize;
        let mut idx = vec![0usize; len];
        // Odometer: the last character changes fastest.
        'strings: loop {
            out.push(idx.iter().map(|i| chars[*i]).collect());
            let mut k = len;
            loop {
                if k == 0 {
                    break 'strings;
                }
                k -= 1;
                idx[k] += 1;
                if idx[k] < chars.len() {
                    break;
                }
                idx[k] = 0;
            }
        }
    }
    Ok(out)
}

fn process(
    value: String,
    steps: &[Processing],
    scripts: ScriptRunner<'_>,
) -> Result<String, String> {
    let mut v = value;
    for step in steps {
        v = match step {
            Processing::Prefix { text } => format!("{text}{v}"),
            Processing::Suffix { text } => format!("{v}{text}"),
            Processing::Replace { from, to } if !from.is_empty() => v.replace(from.as_str(), to),
            Processing::Replace { .. } => v,
            Processing::Lowercase => v.to_lowercase(),
            Processing::Uppercase => v.to_uppercase(),
            Processing::Encode { operation } => {
                let bytes = apply(*operation, v.as_bytes())
                    .map_err(|e| format!("{operation:?} failed on {v:?}: {e}"))?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
            Processing::Script { script } => scripts(script, &v)
                .map_err(|e| format!("payload script {script:?} failed on {v:?}: {e}"))?,
        };
    }
    if v.len() > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "a payload is longer than {} KiB",
            MAX_PAYLOAD_BYTES / 1024
        ));
    }
    Ok(v)
}

impl PayloadSet {
    /// The values of the set, processed (without payload scripts).
    pub fn values(&self) -> Result<Vec<String>, String> {
        self.values_with(&no_scripts)
    }

    /// The values of the set, processed; `scripts` runs payload scripts.
    pub fn values_with(&self, scripts: ScriptRunner<'_>) -> Result<Vec<String>, String> {
        let raw = match &self.source {
            PayloadSource::List { items } => {
                if items.len() > MAX_SET_SIZE {
                    return Err(too_many());
                }
                items.clone()
            }
            PayloadSource::Numbers {
                from,
                to,
                step,
                min_digits,
            } => numbers(*from, *to, *step, *min_digits)?,
            PayloadSource::Characters {
                charset,
                min_length,
                max_length,
            } => characters(charset, *min_length, *max_length)?,
            PayloadSource::Dates {
                from,
                to,
                step_days,
                format,
            } => dates(from, to, *step_days, format)?,
            PayloadSource::Builtin { name } => builtin(name)
                .ok_or_else(|| format!("unknown built-in list {name:?}"))?
                .items
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        };
        if raw.is_empty() {
            return Err("a payload set is empty".into());
        }
        raw.into_iter()
            .map(|v| process(v, &self.processing, scripts))
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn set(source: PayloadSource, processing: Vec<Processing>) -> PayloadSet {
        PayloadSet { source, processing }
    }

    #[test]
    fn sources() {
        let n = |from, to, step, min_digits| {
            set(
                PayloadSource::Numbers {
                    from,
                    to,
                    step,
                    min_digits,
                },
                vec![],
            )
            .values()
        };
        assert_eq!(n(1, 3, 1, 0).unwrap(), ["1", "2", "3"]);
        assert_eq!(n(10, 0, -5, 3).unwrap(), ["010", "005", "000"]);
        assert_eq!(n(-1, 1, 1, 2).unwrap(), ["-01", "00", "01"]);
        assert!(n(1, 3, 0, 0).is_err());
        assert!(n(1, 3, -1, 0).is_err());
        assert!(n(0, 10_000_000, 1, 0).unwrap_err().contains("at most"));

        let chars = set(
            PayloadSource::Characters {
                charset: "ab".into(),
                min_length: 1,
                max_length: 2,
            },
            vec![],
        );
        assert_eq!(chars.values().unwrap(), ["a", "b", "aa", "ab", "ba", "bb"]);

        let builtin_set = set(
            PayloadSource::Builtin {
                name: "special-characters".into(),
            },
            vec![],
        );
        assert!(builtin_set.values().unwrap().contains(&"'".to_string()));
        assert!(
            set(PayloadSource::Builtin { name: "x".into() }, vec![])
                .values()
                .is_err()
        );
        assert!(
            set(PayloadSource::List { items: vec![] }, vec![])
                .values()
                .is_err()
        );
    }

    #[test]
    fn processing_in_order() {
        let s = set(
            PayloadSource::List {
                items: vec!["a b".into()],
            },
            vec![
                Processing::Uppercase,
                Processing::Prefix { text: "<".into() },
                Processing::Suffix { text: ">".into() },
                Processing::Replace {
                    from: " ".into(),
                    to: "_".into(),
                },
                Processing::Encode {
                    operation: Operation::UrlEncode,
                },
            ],
        );
        assert_eq!(s.values().unwrap(), ["%3CA_B%3E"]);
        let scripted = set(
            PayloadSource::List {
                items: vec!["a".into()],
            },
            vec![Processing::Script {
                script: "twice".into(),
            }],
        );
        assert!(scripted.values().is_err());
        let runner = |id: &str, v: &str| -> Result<String, String> {
            assert_eq!(id, "twice");
            Ok(v.repeat(2))
        };
        assert_eq!(scripted.values_with(&runner).unwrap(), ["aa"]);
        let b64 = set(
            PayloadSource::List {
                items: vec!["admin".into()],
            },
            vec![Processing::Encode {
                operation: Operation::Base64Encode,
            }],
        );
        assert_eq!(b64.values().unwrap(), ["YWRtaW4="]);
    }

    #[test]
    fn serde_shape() {
        let json = r#"{"source":{"kind":"numbers","from":1,"to":2,"step":1,"minDigits":0},
            "processing":[{"kind":"prefix","text":"x"}]}"#;
        let s: PayloadSet = serde_json::from_str(json).unwrap();
        assert_eq!(s.values().unwrap(), ["x1", "x2"]);
    }

    #[test]
    fn generates_dates_in_formats() {
        let days = |from: &str, to: &str, step, format: &str| {
            set(
                PayloadSource::Dates {
                    from: from.into(),
                    to: to.into(),
                    step_days: step,
                    format: format.into(),
                },
                vec![],
            )
            .values()
        };
        assert_eq!(
            days("2024-02-27", "2024-03-01", 1, "yyyy-MM-dd").unwrap(),
            ["2024-02-27", "2024-02-28", "2024-02-29", "2024-03-01"]
        );
        assert_eq!(
            days("1999-12-31", "2000-01-02", 2, "dd/MM/yy · d MMM yyyy").unwrap(),
            ["31/12/99 · 31 Dec 1999", "02/01/00 · 2 Jan 2000"]
        );
        assert_eq!(days("2023-01-05", "2023-01-05", 7, "M/d").unwrap(), ["1/5"]);
        assert!(
            days("2023-02-29", "2023-03-01", 1, "yyyy").is_err(),
            "not a leap year"
        );
        assert!(days("2023-03-01", "2023-02-01", 1, "yyyy").is_err());
        assert!(days("2023-03-01", "2023-03-02", 0, "yyyy").is_err());
        assert!(days("01/02/2023", "2023-03-02", 1, "yyyy").is_err());
        assert!(
            days("0001-01-01", "9999-12-31", 1, "yyyy").is_err(),
            "too many"
        );
    }
}
