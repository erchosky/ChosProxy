//! Statistics on a sample of tokens (spec §19): length, character set,
//! per-position entropy, bit-level bias, frequency, repetition and simple
//! patterns. Each finding is an observation with an explanation; there is no
//! single "secure / insecure" verdict.

use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use ts_rs::TS;

/// Most tokens analyzed.
pub const MAX_TOKENS: usize = 100_000;
/// Two-sided 99% normal quantile.
const Z99: f64 = 2.576;

/// How notable an observation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ObservationLevel {
    /// Nothing worrying in this measurement.
    Good,
    /// Worth a closer look.
    Attention,
    /// A real weakness is likely.
    Concern,
}

/// One finding with its explanation.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Observation {
    /// How notable.
    pub level: ObservationLevel,
    /// Short statement.
    pub title: String,
    /// What it means and why.
    pub detail: String,
}

/// Variation of the characters at one position.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PositionStat {
    /// Position (0-based).
    pub position: u32,
    /// Shannon entropy of the characters seen there, in bits.
    pub entropy_bits: f64,
    /// Different characters seen there.
    pub distinct: u32,
}

/// Balance of one bit across tokens.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BitStat {
    /// Bit index (0-based, most significant first per character).
    pub bit: u32,
    /// Share of tokens with this bit set (0.5 is ideal).
    pub ones_ratio: f64,
    /// Outside the 99% band around 0.5.
    pub biased: bool,
}

/// How many tokens have a given length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LengthCount {
    /// Length in characters.
    pub length: u32,
    /// Tokens with it.
    #[ts(type = "number")]
    pub count: u64,
}

/// Chi-square test of how evenly characters are used.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChiSquare {
    /// Statistic.
    pub statistic: f64,
    /// Degrees of freedom.
    pub degrees: u32,
    /// Approximate p-value (small means uneven).
    pub p_value: f64,
}

/// The full analysis of a token sample.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TokenAnalysis {
    /// Tokens analyzed.
    #[ts(type = "number")]
    pub samples: u64,
    /// Different tokens.
    #[ts(type = "number")]
    pub unique: u64,
    /// Shortest token.
    pub min_length: u32,
    /// Longest token.
    pub max_length: u32,
    /// Most common length (per-position and bit analyses use these tokens).
    pub modal_length: u32,
    /// Tokens with the most common length.
    #[ts(type = "number")]
    pub modal_count: u64,
    /// Length distribution.
    pub lengths: Vec<LengthCount>,
    /// Characters seen, sorted.
    pub alphabet: String,
    /// What the characters look like: decimal, hex, base64, base64url or
    /// text.
    pub encoding: String,
    /// Information one character can carry in that encoding, in bits.
    pub bits_per_char: f64,
    /// Entropy per position (tokens of the modal length).
    pub positions: Vec<PositionStat>,
    /// Sum of the per-position entropies: an upper estimate of the token's
    /// unpredictability.
    pub estimated_entropy_bits: f64,
    /// What the format could hold at most (length × bits per character).
    pub max_entropy_bits: f64,
    /// The most entropy one position can show with this many samples.
    pub sample_limit_bits: f64,
    /// Bit balance (hex and base64 tokens only).
    pub bits: Vec<BitStat>,
    /// Half-width of the 99% band around 0.5 for the bit ratios.
    pub bit_band: f64,
    /// Bits outside the band.
    pub biased_bits: u32,
    /// Monobit test over all bits: z-score (|z| > 2.58 is unusual).
    pub monobit_z: Option<f64>,
    /// Character frequency test.
    pub chi_square: Option<ChiSquare>,
    /// Characters shared at the start of every token.
    pub common_prefix: u32,
    /// Characters shared at the end of every token.
    pub common_suffix: u32,
    /// Tokens read as numbers grow by small steps.
    pub sequential: bool,
    /// Findings, most notable first.
    pub observations: Vec<Observation>,
}

fn log2(x: f64) -> f64 {
    x.log2()
}

fn entropy(counts: impl Iterator<Item = u64>, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let n = total as f64;
    counts
        .filter(|c| *c > 0)
        .map(|c| {
            let p = c as f64 / n;
            -p * log2(p)
        })
        .sum()
}

/// Standard normal CDF (Abramowitz–Stegun 7.1.26 for erf).
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    if z >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

/// Upper-tail p-value of a chi-square statistic (Wilson–Hilferty).
fn chi_square_p(statistic: f64, degrees: u32) -> f64 {
    let k = f64::from(degrees);
    let z = ((statistic / k).cbrt() - (1.0 - 2.0 / (9.0 * k))) / (2.0 / (9.0 * k)).sqrt();
    1.0 - normal_cdf(z)
}

const B64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64URL: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The encoding the characters fit, its symbol table (if a power of two)
/// and bits per character.
fn encoding_of(alphabet: &BTreeSet<char>) -> (&'static str, Option<String>, f64) {
    let all = |f: fn(char) -> bool| alphabet.iter().all(|c| f(*c));
    if all(|c| c.is_ascii_digit()) {
        return ("decimal", None, log2(10.0));
    }
    let lower_hex = all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    let upper_hex = all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c));
    if lower_hex || upper_hex {
        let table = if lower_hex {
            "0123456789abcdef"
        } else {
            "0123456789ABCDEF"
        };
        return ("hex", Some(table.to_string()), 4.0);
    }
    // Letters and digits alone fit Base64 too; call it Base64 only when its
    // own symbols appear or both cases and digits are used.
    let any = |set: &str| alphabet.iter().any(|c| set.contains(*c));
    let mixed =
        any("abcdefghijklmnopqrstuvwxyz") && any("ABCDEFGHIJKLMNOPQRSTUVWXYZ") && any("0123456789");
    if all(|c| B64.contains(c) || c == '=') && (any("+/=") || mixed) {
        return ("base64", Some(B64.to_string()), 6.0);
    }
    if all(|c| B64URL.contains(c) || c == '=') && (any("-_") || mixed) {
        return ("base64url", Some(B64URL.to_string()), 6.0);
    }
    ("text", None, log2(alphabet.len().max(1) as f64))
}

fn common_prefix(tokens: &[&str]) -> usize {
    let Some(first) = tokens.first() else {
        return 0;
    };
    let mut len = first.chars().count();
    for t in tokens.iter().skip(1) {
        len = len.min(
            first
                .chars()
                .zip(t.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    len
}

fn sequential(tokens: &[&str], encoding: &str) -> bool {
    if tokens.len() < 5 {
        return false;
    }
    let radix = match encoding {
        "decimal" => 10,
        "hex" => 16,
        _ => return false,
    };
    let values: Option<Vec<i128>> = tokens
        .iter()
        .map(|t| i128::from_str_radix(t, radix).ok())
        .collect();
    let Some(values) = values else {
        return false;
    };
    let diffs: Vec<i128> = values.windows(2).map(|w| w[1] - w[0]).collect();
    let small = diffs.iter().filter(|d| **d > 0 && **d <= 1_000_000).count();
    small * 10 >= diffs.len() * 9
}

fn obs(
    level: ObservationLevel,
    title: impl Into<String>,
    detail: impl Into<String>,
) -> Observation {
    Observation {
        level,
        title: title.into(),
        detail: detail.into(),
    }
}

/// Analyzes a sample of tokens (in capture order). Needs at least two.
pub fn analyze(tokens: &[String]) -> Result<TokenAnalysis, String> {
    let tokens: Vec<&str> = tokens
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .take(MAX_TOKENS)
        .collect();
    let n = tokens.len();
    if n < 2 {
        return Err("at least two tokens are needed".into());
    }
    let samples = n as u64;
    let unique = tokens.iter().collect::<HashSet<_>>().len() as u64;

    let mut by_length: HashMap<usize, u64> = HashMap::new();
    for t in &tokens {
        *by_length.entry(t.chars().count()).or_default() += 1;
    }
    let mut lengths: Vec<LengthCount> = by_length
        .iter()
        .map(|(l, c)| LengthCount {
            length: u32::try_from(*l).unwrap_or(u32::MAX),
            count: *c,
        })
        .collect();
    lengths.sort_by_key(|l| l.length);
    let modal = lengths
        .iter()
        .max_by(|a, b| a.count.cmp(&b.count).then(b.length.cmp(&a.length)))
        .copied()
        .unwrap_or(LengthCount {
            length: 0,
            count: 0,
        });
    let min_length = lengths.first().map_or(0, |l| l.length);
    let max_length = lengths.last().map_or(0, |l| l.length);

    let alphabet: BTreeSet<char> = tokens.iter().flat_map(|t| t.chars()).collect();
    let (encoding, table, bits_per_char) = encoding_of(&alphabet);

    // Per-position entropy over tokens of the modal length.
    let modal_tokens: Vec<Vec<char>> = tokens
        .iter()
        .map(|t| t.chars().collect::<Vec<_>>())
        .filter(|c| c.len() == modal.length as usize)
        .collect();
    let m = modal_tokens.len() as u64;
    let mut positions = Vec::new();
    for p in 0..modal.length as usize {
        let mut counts: HashMap<char, u64> = HashMap::new();
        for t in &modal_tokens {
            if let Some(c) = t.get(p) {
                *counts.entry(*c).or_default() += 1;
            }
        }
        positions.push(PositionStat {
            position: u32::try_from(p).unwrap_or(u32::MAX),
            entropy_bits: entropy(counts.values().copied(), m),
            distinct: u32::try_from(counts.len()).unwrap_or(u32::MAX),
        });
    }
    let estimated_entropy_bits: f64 = positions.iter().map(|p| p.entropy_bits).sum();
    let max_entropy_bits = f64::from(modal.length) * bits_per_char;
    let sample_limit_bits = log2(m.max(1) as f64);

    // Bit balance for power-of-two encodings.
    let mut bits = Vec::new();
    let mut monobit_z = None;
    let bit_band = Z99 * 0.5 / (m.max(1) as f64).sqrt();
    if let Some(table) = &table {
        let width = if encoding == "hex" { 4 } else { 6 };
        let index: HashMap<char, u32> = table
            .chars()
            .enumerate()
            .map(|(i, c)| (c, u32::try_from(i).unwrap_or(0)))
            .collect();
        let symbols = modal.length as usize
            - modal_tokens
                .first()
                .map_or(0, |t| t.iter().rev().take_while(|c| **c == '=').count());
        let mut ones = vec![0u64; symbols * width];
        for t in &modal_tokens {
            for (p, c) in t.iter().take(symbols).enumerate() {
                let v = index.get(c).copied().unwrap_or(0);
                for b in 0..width {
                    if v >> (width - 1 - b) & 1 == 1 {
                        ones[p * width + b] += 1;
                    }
                }
            }
        }
        let total_ones: u64 = ones.iter().sum();
        let total_bits = (ones.len() as u64) * m;
        if total_bits > 0 {
            let nb = total_bits as f64;
            monobit_z = Some((total_ones as f64 - nb / 2.0) / (nb / 4.0).sqrt());
        }
        bits = ones
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let ratio = *o as f64 / m.max(1) as f64;
                BitStat {
                    bit: u32::try_from(i).unwrap_or(u32::MAX),
                    ones_ratio: ratio,
                    biased: (ratio - 0.5).abs() > bit_band,
                }
            })
            .collect();
    }
    let biased_bits = u32::try_from(bits.iter().filter(|b| b.biased).count()).unwrap_or(0);

    // Character frequency over the whole sample.
    let symbols: Vec<char> = match (&table, encoding) {
        (Some(t), _) => t.chars().collect(),
        (None, "decimal") => "0123456789".chars().collect(),
        _ => alphabet.iter().copied().collect(),
    };
    let mut freq: HashMap<char, u64> = HashMap::new();
    let mut total_chars = 0u64;
    for t in &tokens {
        for c in t.chars().filter(|c| *c != '=') {
            *freq.entry(c).or_default() += 1;
            total_chars += 1;
        }
    }
    let k = symbols.len();
    let chi_square = if k >= 2 && total_chars as f64 / k as f64 >= 5.0 {
        let expected = total_chars as f64 / k as f64;
        let statistic: f64 = symbols
            .iter()
            .map(|c| {
                let o = *freq.get(c).unwrap_or(&0) as f64;
                (o - expected).powi(2) / expected
            })
            .sum();
        let degrees = u32::try_from(k - 1).unwrap_or(u32::MAX);
        Some(ChiSquare {
            statistic,
            degrees,
            p_value: chi_square_p(statistic, degrees),
        })
    } else {
        None
    };

    let prefix = common_prefix(&tokens);
    let reversed: Vec<String> = tokens.iter().map(|t| t.chars().rev().collect()).collect();
    let suffix = common_prefix(&reversed.iter().map(String::as_str).collect::<Vec<_>>());
    let is_sequential = sequential(&tokens, encoding);

    // Observations.
    use ObservationLevel::{Attention, Concern, Good};
    let mut o = Vec::new();
    if n < 100 {
        o.push(obs(
            Attention,
            format!("Only {n} tokens analyzed"),
            "With few samples every estimate is rough: entropy is underestimated and \
             patterns can hide. Capture at least a few hundred tokens, ideally 1,000 or more \
             for the bit-level analysis.",
        ));
    }
    let duplicates = samples - unique;
    if duplicates > 0 {
        o.push(obs(
            Concern,
            format!("{duplicates} token(s) repeated"),
            "A session or anti-CSRF token should never repeat. Repeats mean the token space is \
             small, the generator is weak, or the server reuses tokens (check that the requests \
             did not send a cookie that keeps the same session).",
        ));
    } else {
        o.push(obs(
            Good,
            "No token repeated",
            "Every token in the sample is different.",
        ));
    }
    if is_sequential {
        o.push(obs(
            Concern,
            "Tokens increase predictably",
            "Read as numbers, consecutive tokens grow by small steps: the next token can be \
             guessed from the previous one (a counter or a time stamp rather than random data).",
        ));
    }
    if prefix >= 4 || suffix >= 4 {
        o.push(obs(
            Attention,
            format!(
                "Fixed parts: {prefix} character(s) at the start and {suffix} at the end are the same in every token"
            ),
            "Characters that never change add no randomness. They may be a version, a server \
             id or an encoded time stamp; only the varying part protects the token.",
        ));
    }
    let entropy_detail = format!(
        "Estimated from how much each position varies ({:.0} of at most {:.0} bits for this \
         format). It is an upper estimate: dependencies between characters are not measured, \
         and with {m} samples one position can show at most {:.1} bits. Session identifiers \
         should carry at least 64 bits of entropy (OWASP).",
        estimated_entropy_bits, max_entropy_bits, sample_limit_bits
    );
    if estimated_entropy_bits < 32.0 {
        o.push(obs(
            Concern,
            format!("Low estimated entropy: about {estimated_entropy_bits:.0} bits"),
            entropy_detail,
        ));
    } else if estimated_entropy_bits < 64.0 {
        o.push(obs(
            Attention,
            format!("Estimated entropy about {estimated_entropy_bits:.0} bits"),
            entropy_detail,
        ));
    } else {
        o.push(obs(
            Good,
            format!("Estimated entropy about {estimated_entropy_bits:.0} bits"),
            entropy_detail,
        ));
    }
    let limit = bits_per_char.min(sample_limit_bits);
    let weak: Vec<String> = positions
        .iter()
        .filter(|p| p.entropy_bits < 0.5 * limit)
        .map(|p| (p.position + 1).to_string())
        .collect();
    if !weak.is_empty() && weak.len() < positions.len() {
        o.push(obs(
            Attention,
            format!("{} position(s) vary little", weak.len()),
            format!(
                "Positions {} use few different characters compared with the others. Fixed or \
                 nearly fixed positions reduce the effective length of the token.",
                weak.iter().take(20).cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    if !bits.is_empty() {
        let share = f64::from(biased_bits) / bits.len() as f64;
        let detail = format!(
            "Each bit should be set in about half of the tokens. With {m} samples a random bit \
             stays within 0.5 ± {bit_band:.3} 99% of the time, so about 1% of bits fall outside \
             by chance; {biased_bits} of {} do.",
            bits.len()
        );
        if share > 0.15 {
            o.push(obs(
                Concern,
                format!("{biased_bits} bits are clearly unbalanced"),
                detail,
            ));
        } else if share > 0.05 {
            o.push(obs(
                Attention,
                format!("{biased_bits} bits are unbalanced"),
                detail,
            ));
        } else {
            o.push(obs(Good, "Bits are balanced", detail));
        }
    }
    if let Some(z) = monobit_z
        && z.abs() > Z99
    {
        o.push(obs(
            Attention,
            "More ones than zeros overall (or the reverse)",
            format!(
                "Monobit test z = {z:.2}: across all bits the share of ones is further from 0.5 \
                 than chance explains."
            ),
        ));
    }
    if let Some(c) = &chi_square
        && c.p_value < 0.01
    {
        o.push(obs(
            Attention,
            "Characters are not used evenly",
            format!(
                "Chi-square = {:.1} with {} degrees of freedom (p ≈ {:.4}): some characters \
                 appear much more often than others.{}",
                c.statistic,
                c.degrees,
                c.p_value,
                if encoding.starts_with("base64") {
                    " In fixed-length Base64 the last character often has fewer possible \
                     values; look at the per-position chart."
                } else {
                    ""
                }
            ),
        ));
    }
    o.sort_by_key(|x| match x.level {
        Concern => 0,
        Attention => 1,
        Good => 2,
    });

    Ok(TokenAnalysis {
        samples,
        unique,
        min_length,
        max_length,
        modal_length: modal.length,
        modal_count: m,
        lengths,
        alphabet: alphabet.into_iter().collect(),
        encoding: encoding.to_string(),
        bits_per_char,
        positions,
        estimated_entropy_bits,
        max_entropy_bits,
        sample_limit_bits,
        bits,
        bit_band,
        biased_bits,
        monobit_z,
        chi_square,
        common_prefix: u32::try_from(prefix).unwrap_or(u32::MAX),
        common_suffix: u32::try_from(suffix).unwrap_or(u32::MAX),
        sequential: is_sequential,
        observations: o,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    /// A small deterministic generator (xorshift) for test samples.
    fn random_hex(count: usize, len: usize) -> Vec<String> {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        (0..count)
            .map(|_| {
                (0..len)
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 7;
                        x ^= x << 17;
                        char::from_digit((x % 16) as u32, 16).unwrap()
                    })
                    .collect()
            })
            .collect()
    }

    fn levels(a: &TokenAnalysis) -> Vec<(ObservationLevel, String)> {
        a.observations
            .iter()
            .map(|o| (o.level, o.title.clone()))
            .collect()
    }

    #[test]
    fn random_tokens_look_random() {
        let a = analyze(&random_hex(2000, 32)).unwrap();
        assert_eq!((a.samples, a.unique, a.modal_length), (2000, 2000, 32));
        assert_eq!(a.encoding, "hex");
        assert!((a.max_entropy_bits - 128.0).abs() < 1e-9);
        assert!(
            a.estimated_entropy_bits > 120.0,
            "{}",
            a.estimated_entropy_bits
        );
        assert_eq!(a.bits.len(), 128);
        assert!(a.biased_bits < 8, "{} biased", a.biased_bits);
        assert!(a.monobit_z.unwrap().abs() < 4.0);
        assert!(!a.sequential);
        assert!(
            a.observations
                .iter()
                .all(|o| o.level == ObservationLevel::Good),
            "{:?}",
            levels(&a)
        );
    }

    #[test]
    fn weak_tokens_are_flagged() {
        // A counter with a fixed prefix.
        let counter: Vec<String> = (1000..1300).map(|i| format!("SESS{i}")).collect();
        let a = analyze(&counter).unwrap();
        assert_eq!(a.encoding, "text");
        assert_eq!(a.common_prefix, 5, "SESS1 is shared");
        let titles = levels(&a);
        assert!(
            titles
                .iter()
                .any(|(l, t)| *l == ObservationLevel::Concern && t.contains("entropy")),
            "{titles:?}"
        );
        assert!(titles.iter().any(|(_, t)| t.contains("Fixed parts")));

        let numbers: Vec<String> = (5000..5200).map(|i| (i * 3).to_string()).collect();
        let a = analyze(&numbers).unwrap();
        assert!(a.sequential);
        assert_eq!(a.observations[0].level, ObservationLevel::Concern);

        let mut repeated = random_hex(50, 16);
        repeated.push(repeated[0].clone());
        let a = analyze(&repeated).unwrap();
        assert_eq!(a.unique, 50);
        let titles = levels(&a);
        assert!(titles.iter().any(|(_, t)| t.contains("repeated")));
        assert!(titles.iter().any(|(_, t)| t.starts_with("Only 51 tokens")));
    }

    #[test]
    fn lengths_encodings_and_limits() {
        let tokens: Vec<String> = ["abc", "abcd", "abce", "x"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let a = analyze(&tokens).unwrap();
        assert_eq!(
            (a.min_length, a.max_length, a.modal_length, a.modal_count),
            (1, 4, 4, 2)
        );
        assert_eq!(a.lengths.len(), 3);
        assert_eq!(a.positions.len(), 4);
        assert!(a.positions[3].entropy_bits > 0.99 && a.positions[0].entropy_bits == 0.0);
        assert!(analyze(&["one".to_string()]).is_err());
        assert!(analyze(&[" ".to_string(), String::new()]).is_err());

        let b64 = analyze(&["QUJD".to_string(), "REVG+/==".to_string()]).unwrap();
        assert_eq!(b64.encoding, "base64");
        let url = analyze(&["ab-_".to_string(), "cd_-".to_string()]).unwrap();
        assert_eq!(url.encoding, "base64url");
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((normal_cdf(Z99) - 0.995).abs() < 1e-3);
        assert!(chi_square_p(15.0, 15) > 0.3 && chi_square_p(60.0, 15) < 0.001);
    }
}
