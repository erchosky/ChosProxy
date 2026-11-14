//! Attack modes: which payloads go into which positions, request by
//! request. Every request is addressed by its index, so a plan needs no
//! state and the count is known before starting.

use crate::template::Template;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How payloads are placed in the positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "FuzzAttackMode")]
pub enum AttackMode {
    /// One set; each position in turn gets every payload while the others
    /// keep their original value.
    Sniper,
    /// One set; the same payload in every position at once.
    BatteringRam,
    /// One set per position; the n-th payload of every set together (stops
    /// at the shortest set).
    Pitchfork,
    /// One set per position; every combination.
    ClusterBomb,
}

impl AttackMode {
    /// Payload sets the mode needs for `positions` positions.
    pub fn sets_needed(self, positions: usize) -> usize {
        match self {
            AttackMode::Sniper | AttackMode::BatteringRam => 1,
            AttackMode::Pitchfork | AttackMode::ClusterBomb => positions,
        }
    }
}

/// The values of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// Value for each position.
    pub values: Vec<String>,
    /// The payload(s) used, for the results table.
    pub payloads: Vec<String>,
    /// The position that changed (sniper only).
    pub position: Option<usize>,
}

/// A planned attack.
#[derive(Debug, Clone)]
pub struct Plan {
    template: Template,
    mode: AttackMode,
    sets: Vec<Vec<String>>,
    count: u64,
}

impl Plan {
    /// Checks that the sets fit the mode and the positions.
    pub fn new(
        template: Template,
        mode: AttackMode,
        sets: Vec<Vec<String>>,
    ) -> Result<Plan, String> {
        let positions = template.positions();
        if positions == 0 {
            return Err("mark at least one payload position with §…§".into());
        }
        let needed = mode.sets_needed(positions);
        if sets.len() < needed {
            return Err(format!(
                "this attack mode needs {needed} payload set(s), one per position; {} given",
                sets.len()
            ));
        }
        if sets.iter().take(needed).any(Vec::is_empty) {
            return Err("a payload set is empty".into());
        }
        let len = |i: usize| sets.get(i).map_or(0, Vec::len) as u64;
        let count = match mode {
            AttackMode::Sniper => len(0).saturating_mul(positions as u64),
            AttackMode::BatteringRam => len(0),
            AttackMode::Pitchfork => (0..positions).map(len).min().unwrap_or(0),
            AttackMode::ClusterBomb => {
                (0..positions).fold(1u64, |acc, i| acc.saturating_mul(len(i)))
            }
        };
        Ok(Plan {
            template,
            mode,
            sets,
            count,
        })
    }

    /// Requests in the attack (without the baseline).
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The template.
    pub fn template(&self) -> &Template {
        &self.template
    }

    /// The `index`-th request (0-based), or `None` past the end.
    pub fn attempt(&self, index: u64) -> Option<Attempt> {
        if index >= self.count {
            return None;
        }
        let defaults = self.template.defaults();
        let positions = defaults.len();
        let set = |s: usize, i: u64| -> String {
            self.sets
                .get(s)
                .and_then(|v| v.get(usize::try_from(i).unwrap_or(usize::MAX)))
                .cloned()
                .unwrap_or_default()
        };
        Some(match self.mode {
            AttackMode::Sniper => {
                let len = self.sets.first().map_or(1, Vec::len).max(1) as u64;
                let position = usize::try_from(index / len).unwrap_or(0);
                let payload = set(0, index % len);
                let mut values = defaults.to_vec();
                if let Some(v) = values.get_mut(position) {
                    v.clone_from(&payload);
                }
                Attempt {
                    values,
                    payloads: vec![payload],
                    position: Some(position),
                }
            }
            AttackMode::BatteringRam => {
                let payload = set(0, index);
                Attempt {
                    values: vec![payload.clone(); positions],
                    payloads: vec![payload],
                    position: None,
                }
            }
            AttackMode::Pitchfork => {
                let values: Vec<String> = (0..positions).map(|p| set(p, index)).collect();
                Attempt {
                    payloads: values.clone(),
                    values,
                    position: None,
                }
            }
            AttackMode::ClusterBomb => {
                // Mixed-radix digits; the last position changes fastest.
                let mut rest = index;
                let mut values = vec![String::new(); positions];
                for p in (0..positions).rev() {
                    let len = self.sets.get(p).map_or(1, Vec::len).max(1) as u64;
                    values[p] = set(p, rest % len);
                    rest /= len;
                }
                Attempt {
                    payloads: values.clone(),
                    values,
                    position: None,
                }
            }
        })
    }

    /// The request text of an attempt.
    pub fn render(&self, attempt: &Attempt) -> String {
        self.template.render(&attempt.values)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn plan(text: &str, mode: AttackMode, sets: &[&[&str]]) -> Result<Plan, String> {
        Plan::new(
            Template::parse(text).unwrap(),
            mode,
            sets.iter()
                .map(|s| s.iter().map(|v| (*v).to_string()).collect())
                .collect(),
        )
    }

    fn all(p: &Plan) -> Vec<String> {
        (0..p.count())
            .map(|i| p.render(&p.attempt(i).unwrap()))
            .collect()
    }

    #[test]
    fn sniper_and_battering_ram() {
        let p = plan("a=§1§&b=§2§", AttackMode::Sniper, &[&["x", "y"]]).unwrap();
        assert_eq!(all(&p), ["a=x&b=2", "a=y&b=2", "a=1&b=x", "a=1&b=y"]);
        assert_eq!(p.attempt(2).unwrap().position, Some(1));
        assert!(p.attempt(4).is_none());

        let p = plan("a=§1§&b=§2§", AttackMode::BatteringRam, &[&["x", "y"]]).unwrap();
        assert_eq!(all(&p), ["a=x&b=x", "a=y&b=y"]);
    }

    #[test]
    fn pitchfork_and_cluster_bomb() {
        let p = plan(
            "§u§:§p§",
            AttackMode::Pitchfork,
            &[&["a", "b", "c"], &["1", "2"]],
        )
        .unwrap();
        assert_eq!(all(&p), ["a:1", "b:2"]);
        let p = plan(
            "§u§:§p§",
            AttackMode::ClusterBomb,
            &[&["a", "b"], &["1", "2", "3"]],
        )
        .unwrap();
        assert_eq!(all(&p), ["a:1", "a:2", "a:3", "b:1", "b:2", "b:3"]);
        assert_eq!(p.attempt(4).unwrap().payloads, ["b", "2"]);
    }

    #[test]
    fn plans_are_checked() {
        assert!(
            plan("no positions", AttackMode::Sniper, &[&["x"]])
                .unwrap_err()
                .contains("§")
        );
        assert!(
            plan("§a§§b§", AttackMode::ClusterBomb, &[&["x"]])
                .unwrap_err()
                .contains("2 payload")
        );
        assert!(plan("§a§", AttackMode::Sniper, &[&[]]).is_err());
        let big: Vec<String> = (0..100_000).map(|i| i.to_string()).collect();
        let huge = Plan::new(
            Template::parse(&"§x§".repeat(4)).unwrap(),
            AttackMode::ClusterBomb,
            vec![big.clone(), big.clone(), big.clone(), big],
        )
        .unwrap();
        assert_eq!(
            huge.count(),
            u64::MAX,
            "the count saturates instead of overflowing"
        );
    }
}
