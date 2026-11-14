//! Resource pools (M26, matrix INTRUDER-006): named limits shared by every
//! task and tool assigned to them — requests in flight, requests per
//! second, a random delay, and backing off when a server answers 429 or
//! 503. They sit on top of each task's own limits.
//!
//! Every tool send goes through [`Engine::session_send`], which takes a
//! slot of the tool's pool there (the Repeater's kept connections take it
//! in the Repeater). A tool without a pool behaves as before.

use super::{Engine, read, write};
use crate::AppError;
use chosproxy_core::config::ConfigSection;
use chosproxy_http::head::Headers;
use chosproxy_request::{Limiter, SendError, Sent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::OwnedSemaphorePermit;
use ts_rs::TS;

/// Most pools.
const MAX_POOLS: usize = 50;
/// Longest back-off, whatever `Retry-After` says.
const MAX_BACK_OFF: Duration = Duration::from_secs(60);
/// Back-off when the server gives no usable `Retry-After`.
const DEFAULT_BACK_OFF: Duration = Duration::from_secs(5);
/// Waits at least this long are noted in the history.
const NOTED_WAIT: Duration = Duration::from_secs(1);

/// The tools that can be assigned to a pool: (id, label). The id is the
/// tool name used by session rules.
pub const POOL_TOOLS: [(&str, &str); 5] = [
    ("crawler", "Crawler, scans and content discovery"),
    ("fuzzer", "Fuzzer"),
    ("sequencer", "Sequencer"),
    ("repeater", "Repeater"),
    ("plugin", "Plugins (network.send)"),
];

/// One named pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct PoolConfig {
    /// Unique name.
    pub name: String,
    /// Requests in flight at most, across everything in the pool.
    pub concurrency: u32,
    /// Requests started per second at most (0 = no limit).
    pub requests_per_second: u32,
    /// A random extra delay of up to this many milliseconds before each
    /// request (0 = none).
    pub random_delay_ms: u32,
    /// Pause the pool when a server answers 429 or 503 (for its
    /// `Retry-After`, at most 60 s, or 5 s).
    pub back_off: bool,
}

impl Default for PoolConfig {
    fn default() -> PoolConfig {
        PoolConfig {
            name: String::new(),
            concurrency: 10,
            requests_per_second: 0,
            random_delay_ms: 0,
            back_off: true,
        }
    }
}

impl PoolConfig {
    fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err("a pool needs a name of at most 60 characters".into());
        }
        if !(1..=200).contains(&self.concurrency) {
            return Err(format!(
                "{name}: requests in flight must be between 1 and 200"
            ));
        }
        if self.requests_per_second > 1000 {
            return Err(format!("{name}: at most 1000 requests per second"));
        }
        if self.random_delay_ms > 60_000 {
            return Err(format!("{name}: the random delay is at most 60000 ms"));
        }
        Ok(())
    }
}

/// The pools and which tool uses which (`pools.toml`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct PoolsConfig {
    /// The pools.
    pub pools: Vec<PoolConfig>,
    /// Tool id → pool name. A tool not listed uses only its own limits.
    pub assignments: BTreeMap<String, String>,
}

impl ConfigSection for PoolsConfig {
    const FILE_NAME: &'static str = "pools.toml";

    fn validate(&self) -> Result<(), String> {
        if self.pools.len() > MAX_POOLS {
            return Err(format!("at most {MAX_POOLS} resource pools"));
        }
        for (i, pool) in self.pools.iter().enumerate() {
            pool.validate()?;
            if self.pools[..i]
                .iter()
                .any(|p| p.name.trim().eq_ignore_ascii_case(pool.name.trim()))
            {
                return Err(format!("two pools are named \"{}\"", pool.name.trim()));
            }
        }
        for (tool, pool) in &self.assignments {
            if !POOL_TOOLS.iter().any(|(id, _)| id == tool) {
                return Err(format!("unknown tool \"{tool}\""));
            }
            if !self
                .pools
                .iter()
                .any(|p| p.name.trim().eq_ignore_ascii_case(pool.trim()))
            {
                return Err(format!(
                    "{tool} is assigned to a pool that does not exist: {pool}"
                ));
            }
        }
        Ok(())
    }
}

/// A pool in this session: its limiter and counters.
#[derive(Debug)]
pub(crate) struct PoolState {
    config: PoolConfig,
    limiter: Limiter,
    sent: AtomicU64,
    throttled: AtomicU64,
}

impl PoolState {
    fn new(config: PoolConfig) -> PoolState {
        PoolState {
            limiter: Limiter::with_delay(
                config.concurrency as usize,
                config.requests_per_second,
                Duration::from_millis(u64::from(config.random_delay_ms)),
            ),
            config,
            sent: AtomicU64::new(0),
            throttled: AtomicU64::new(0),
        }
    }
}

/// The pools in force (rebuilt when they are saved; a pool whose settings
/// did not change keeps its limiter and counters).
#[derive(Debug, Default)]
pub(crate) struct PoolRuntime {
    pools: HashMap<String, Arc<PoolState>>,
    /// Tool id → pool key (lowercased name).
    assignments: HashMap<String, String>,
}

fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

impl PoolRuntime {
    pub(crate) fn build(config: &PoolsConfig, previous: Option<&PoolRuntime>) -> PoolRuntime {
        let mut pools = HashMap::new();
        for pool in &config.pools {
            let k = key(&pool.name);
            let mut pool = pool.clone();
            pool.name = pool.name.trim().to_string();
            let state = previous
                .and_then(|p| p.pools.get(&k))
                .filter(|s| s.config == pool)
                .cloned()
                .unwrap_or_else(|| Arc::new(PoolState::new(pool)));
            pools.insert(k, state);
        }
        let assignments = config
            .assignments
            .iter()
            .map(|(tool, pool)| (tool.clone(), key(pool)))
            .filter(|(_, k)| pools.contains_key(k))
            .collect();
        PoolRuntime { pools, assignments }
    }

    fn for_tool(&self, tool: &str) -> Option<Arc<PoolState>> {
        let lookup = if tool == "scanner" { "crawler" } else { tool };
        self.assignments
            .get(lookup)
            .and_then(|k| self.pools.get(k))
            .cloned()
    }
}

/// A pool's settings and what it is doing now.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PoolStatus {
    /// Its settings.
    pub config: PoolConfig,
    /// Tools assigned to it (ids).
    pub tools: Vec<String>,
    /// Requests holding a slot.
    #[ts(type = "number")]
    pub in_flight: u64,
    /// Requests waiting for a slot, the rate or a delay.
    #[ts(type = "number")]
    pub waiting: u64,
    /// Requests sent through it in this session.
    #[ts(type = "number")]
    pub sent: u64,
    /// Times it backed off after a 429 or 503.
    #[ts(type = "number")]
    pub throttled: u64,
    /// Milliseconds until it starts requests again, when backing off.
    #[ts(type = "number | null")]
    pub paused_ms: Option<u64>,
}

/// A tool that can use a pool.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PoolTool {
    /// Id (as in assignments).
    pub id: String,
    /// What it covers.
    pub label: String,
}

/// The Resource pools view.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PoolsView {
    /// The pools with their live counters.
    pub pools: Vec<PoolStatus>,
    /// Tool id → pool name.
    pub assignments: BTreeMap<String, String>,
    /// Tools that can be assigned.
    pub tools: Vec<PoolTool>,
}

/// A slot taken in a pool for one send.
pub(crate) struct PoolTicket {
    pool: Arc<PoolState>,
    waited: Duration,
    _permit: Option<OwnedSemaphorePermit>,
}

/// How long a 429/503 response asks the client to wait (seconds form of
/// `Retry-After` only; dates and junk give the default).
fn back_off_for(sent: &Sent) -> Option<Duration> {
    if !matches!(sent.head.status, 429 | 503) {
        return None;
    }
    let asked = sent
        .head
        .header_str("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|d| !d.is_zero());
    Some(asked.unwrap_or(DEFAULT_BACK_OFF).min(MAX_BACK_OFF))
}

impl PoolTicket {
    /// Counts the send, backs the pool off if the server asked for it, and
    /// notes in the history how long the request waited and why.
    pub(crate) async fn finish(self, outcome: &mut Result<Sent, SendError>) {
        self.pool.sent.fetch_add(1, Ordering::Relaxed);
        let Ok(sent) = outcome else {
            return;
        };
        if self.waited >= NOTED_WAIT {
            sent.notes.push(format!(
                "Waited {:.1} s for the resource pool “{}”.",
                self.waited.as_secs_f64(),
                self.pool.config.name
            ));
        }
        if self.pool.config.back_off
            && let Some(pause) = back_off_for(sent)
        {
            self.pool.throttled.fetch_add(1, Ordering::Relaxed);
            self.pool.limiter.back_off(pause).await;
            sent.notes.push(format!(
                "The server answered {}: the resource pool “{}” pauses for {} s.",
                sent.head.status,
                self.pool.config.name,
                pause.as_secs()
            ));
        }
    }
}

impl Engine {
    /// Takes a slot in the pool of `tool`, waiting as its limits say.
    /// `None` when the tool has no pool.
    pub(crate) async fn pool_enter(&self, tool: &str) -> Option<PoolTicket> {
        // A pool chosen for the task this send belongs to comes first.
        let chosen = super::task_ops::current_task().and_then(|id| self.task_pool(id));
        let pool = {
            let runtime = read(&self.inner.pools);
            chosen
                .and_then(|name| runtime.pools.get(&key(&name)).cloned())
                .or_else(|| runtime.for_tool(tool))
        }?;
        let start = Instant::now();
        let permit = pool.limiter.acquire().await;
        Some(PoolTicket {
            waited: start.elapsed(),
            pool,
            _permit: permit,
        })
    }

    /// The saved pools and assignments.
    pub fn pools_config(&self) -> PoolsConfig {
        self.inner.store.load::<PoolsConfig>().value
    }

    /// The pools with their live counters.
    pub async fn resource_pools(&self) -> PoolsView {
        let config = self.pools_config();
        let runtime = read(&self.inner.pools);
        let mut pools = Vec::new();
        for pool in &config.pools {
            let state = runtime.pools.get(&key(&pool.name));
            let tools = config
                .assignments
                .iter()
                .filter(|(_, p)| key(p) == key(&pool.name))
                .map(|(t, _)| t.clone())
                .collect();
            let mut status = PoolStatus {
                config: pool.clone(),
                tools,
                in_flight: 0,
                waiting: 0,
                sent: 0,
                throttled: 0,
                paused_ms: None,
            };
            if let Some(state) = state {
                status.in_flight = state.limiter.in_flight() as u64;
                status.waiting = state.limiter.waiting();
                status.sent = state.sent.load(Ordering::Relaxed);
                status.throttled = state.throttled.load(Ordering::Relaxed);
                status.paused_ms = state
                    .limiter
                    .paused_for()
                    .await
                    .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            }
            pools.push(status);
        }
        PoolsView {
            pools,
            assignments: config.assignments,
            tools: POOL_TOOLS
                .iter()
                .map(|(id, label)| PoolTool {
                    id: (*id).into(),
                    label: (*label).into(),
                })
                .collect(),
        }
    }

    /// Validates, saves and applies pools and assignments. Requests already
    /// waiting in a changed pool finish under its old limits.
    pub async fn save_resource_pools(
        &self,
        mut config: PoolsConfig,
    ) -> Result<PoolsView, AppError> {
        for pool in &mut config.pools {
            pool.name = pool.name.trim().to_string();
        }
        config.assignments.retain(|_, pool| !pool.trim().is_empty());
        config.validate().map_err(AppError::Invalid)?;
        self.inner.store.save(&config)?;
        let runtime = {
            let current = read(&self.inner.pools);
            PoolRuntime::build(&config, Some(&current))
        };
        write(&self.inner.pools, Arc::new(runtime));
        self.audit(
            "pools.changed",
            &format!(
                "{} resource pool(s), {} tool assignment(s)",
                config.pools.len(),
                config.assignments.len()
            ),
        );
        Ok(self.resource_pools().await)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    fn pool(name: &str) -> PoolConfig {
        PoolConfig {
            name: name.into(),
            ..PoolConfig::default()
        }
    }

    #[test]
    fn validation() {
        let mut c = PoolsConfig {
            pools: vec![pool("Slow"), pool("slow ")],
            ..PoolsConfig::default()
        };
        assert!(c.validate().unwrap_err().contains("two pools"));
        c.pools.pop();
        c.assignments.insert("fuzzer".into(), "SLOW".into());
        assert!(c.validate().is_ok());
        c.assignments.insert("browser".into(), "Slow".into());
        assert!(c.validate().unwrap_err().contains("unknown tool"));
        c.assignments.remove("browser");
        c.assignments.insert("crawler".into(), "Fast".into());
        assert!(c.validate().unwrap_err().contains("does not exist"));
        c.assignments.clear();
        c.pools[0].concurrency = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn rebuilding_keeps_unchanged_pools() {
        let mut c = PoolsConfig {
            pools: vec![pool("A"), pool("B")],
            ..PoolsConfig::default()
        };
        c.assignments.insert("fuzzer".into(), "a".into());
        let first = PoolRuntime::build(&c, None);
        first.pools["a"].sent.store(7, Ordering::Relaxed);
        c.pools[1].concurrency = 3;
        let second = PoolRuntime::build(&c, Some(&first));
        assert_eq!(second.pools["a"].sent.load(Ordering::Relaxed), 7);
        assert!(!Arc::ptr_eq(&first.pools["b"], &second.pools["b"]));
        assert!(second.for_tool("fuzzer").is_some());
        assert!(second.for_tool("crawler").is_none());
    }
}
