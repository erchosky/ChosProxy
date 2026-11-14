//! Performance view (M31, PLATFORM-006): live counters of this ChosProxy
//! process, read on demand. Local only: nothing is sent or stored.

use super::{Engine, read};
use crate::AppError;
use chosproxy_proxy::ProxyStats;
use serde::Serialize;
use std::sync::atomic::Ordering;
use ts_rs::TS;

/// Memory and CPU of this process.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessUsage {
    /// Resident memory, bytes.
    #[ts(type = "number")]
    pub memory_bytes: u64,
    /// CPU use since the previous reading, percent of one core (can pass
    /// 100 on several cores; 0 on the first reading).
    pub cpu_percent: f32,
}

/// One reading of the counters.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceSnapshot {
    /// When, ms since the epoch.
    #[ts(type = "number")]
    pub at: i64,
    /// This process (none if the system does not tell).
    pub process: Option<ProcessUsage>,
    /// Size of the project database file, bytes (0 without a project).
    #[ts(type = "number")]
    pub db_bytes: u64,
    /// Size of its write-ahead log, bytes.
    #[ts(type = "number")]
    pub wal_bytes: u64,
    /// Requests and responses stored since the project opened (all tools);
    /// the view turns two readings into a rate.
    #[ts(type = "number")]
    pub stored: u64,
    /// Captures dropped because storage could not keep up.
    #[ts(type = "number")]
    pub dropped: u64,
    /// Proxy counters (open connections and totals).
    pub proxy: ProxyStats,
    /// Requests being sent through resource pools.
    #[ts(type = "number")]
    pub pool_in_flight: u64,
    /// Requests waiting for a pool slot.
    #[ts(type = "number")]
    pub pool_waiting: u64,
    /// Tasks running in this session.
    #[ts(type = "number")]
    pub tasks_running: u64,
}

impl Engine {
    /// A reading of the performance counters.
    pub async fn performance(&self) -> Result<PerformanceSnapshot, AppError> {
        let process = {
            let mut sys = super::lock(&self.inner.perf);
            match sysinfo::get_current_pid() {
                Ok(pid) => {
                    sys.refresh_processes_specifics(
                        sysinfo::ProcessesToUpdate::Some(&[pid]),
                        false,
                        sysinfo::ProcessRefreshKind::nothing()
                            .with_memory()
                            .with_cpu(),
                    );
                    sys.process(pid).map(|p| ProcessUsage {
                        memory_bytes: p.memory(),
                        cpu_percent: p.cpu_usage(),
                    })
                }
                Err(_) => None,
            }
        };
        let (db_bytes, wal_bytes) = match read(&self.inner.project) {
            Some(p) => {
                let size = |path: String| std::fs::metadata(path).map_or(0, |m| m.len());
                (size(p.path.clone()), size(format!("{}-wal", p.path)))
            }
            None => (0, 0),
        };
        let pools = self.resource_pools().await;
        Ok(PerformanceSnapshot {
            at: chosproxy_core::time::now_ms(),
            process,
            db_bytes,
            wal_bytes,
            stored: self.inner.stored.load(Ordering::Relaxed),
            dropped: self.inner.dropped.load(Ordering::Relaxed),
            proxy: self.inner.proxy.stats(),
            pool_in_flight: pools.pools.iter().map(|p| p.in_flight).sum(),
            pool_waiting: pools.pools.iter().map(|p| p.waiting).sum(),
            tasks_running: super::lock(&self.inner.tasks_running).len() as u64,
        })
    }
}
