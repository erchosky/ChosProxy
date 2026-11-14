//! ChosScripts in the engine (ADR-0032): the library (compiled once per
//! change), checks during passive analysis, history filters and columns,
//! actions, rewrite scripts for Match & Replace, and test runs from the
//! editor.

use super::Engine;
use crate::AppError;
use chosproxy_core::config::{ConfigSection, ScriptEntry, ScriptKind, ScriptsConfig};
use chosproxy_script::{Finding, Message, MessageParts, Script, ScriptError};
use chosproxy_storage::{
    FindingSeverity, HistoryPage, HistoryQuery, HistoryRow, PassiveIssue, StoredExchange,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use ts_rs::TS;

/// `format` of a shared scripts file.
const SCRIPT_FILE_FORMAT: &str = "chosproxy-scripts";

/// A file of shared scripts.
#[derive(Debug, serde::Deserialize, Serialize)]
struct ScriptFile {
    format: String,
    version: u32,
    scripts: Vec<ScriptEntry>,
}

/// History rows a scripted filter looks at, newest matching the other
/// filters first.
pub const SCRIPT_FILTER_SCAN: u32 = 20_000;
/// Rows read per round of a scripted filter.
const SCAN_PAGE: u32 = 500;

/// The compiled library.
#[derive(Default)]
pub(crate) struct Library {
    pub(crate) config: ScriptsConfig,
    compiled: HashMap<String, Result<Arc<Script>, ScriptError>>,
}

impl Library {
    pub(crate) fn build(config: ScriptsConfig) -> Library {
        let compiled = config
            .scripts
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    Script::compile(s.kind, &s.source).map(Arc::new),
                )
            })
            .collect();
        Library { config, compiled }
    }

    /// An enabled, compiled script of `kind`.
    pub(crate) fn usable(&self, id: &str, kind: ScriptKind) -> Option<Arc<Script>> {
        let entry = self.config.get(id)?;
        if !entry.enabled || entry.kind != kind {
            return None;
        }
        self.compiled.get(id)?.as_ref().ok().cloned()
    }

    fn script(&self, id: &str, kind: ScriptKind) -> Result<Arc<Script>, AppError> {
        let entry = self
            .config
            .get(id)
            .ok_or_else(|| AppError::Invalid(format!("no script {id:?}")))?;
        if entry.kind != kind {
            return Err(AppError::Invalid(format!(
                "“{}” is not a {kind:?} script",
                entry.name
            )));
        }
        match self.compiled.get(id) {
            Some(Ok(s)) => Ok(s.clone()),
            Some(Err(e)) => Err(AppError::Invalid(format!("“{}”: {e}", entry.name))),
            None => Err(AppError::Invalid(format!("no script {id:?}"))),
        }
    }

    /// Runs payload script `id` on `value` (for the Fuzzer).
    pub(crate) fn run_payload(&self, id: &str, value: &str) -> Result<String, String> {
        let script = self
            .script(id, ScriptKind::Payload)
            .map_err(|e| e.to_string())?;
        script.payload(value).map_err(|e| e.to_string())
    }

    /// Enabled check scripts that compile.
    pub(crate) fn checks(&self) -> Vec<(ScriptEntry, Arc<Script>)> {
        self.config
            .scripts
            .iter()
            .filter(|s| s.enabled && s.kind == ScriptKind::Check)
            .filter_map(|s| {
                let compiled = self.compiled.get(&s.id)?.as_ref().ok()?;
                Some((s.clone(), compiled.clone()))
            })
            .collect()
    }
}

/// The library as shown in the Scripts view.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptLibrary {
    /// Scripts.
    pub config: ScriptsConfig,
    /// Scripts that do not compile, by id.
    pub problems: HashMap<String, ScriptError>,
}

/// The outcome of running a script once.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptRun {
    /// Compile or run error.
    pub problem: Option<ScriptError>,
    /// Filter result, column value, action output or rewritten messages.
    pub output: Option<String>,
    /// Findings of a check.
    pub findings: Vec<Finding>,
    /// What the script printed.
    pub printed: Vec<String>,
    /// How long it ran, ms.
    #[ts(type = "number")]
    pub elapsed_ms: u64,
}

/// A column value for one history row.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptCell {
    /// Exchange id.
    #[ts(type = "number")]
    pub id: i64,
    /// Value (empty on error).
    pub value: String,
    /// Why there is no value.
    pub error: Option<String>,
}

/// A history page selected by a filter script.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptedHistoryPage {
    /// Matching rows and their count.
    pub page: HistoryPage,
    /// Rows the script looked at.
    #[ts(type = "number")]
    pub scanned: u64,
    /// Only the newest rows were looked at (more exist).
    pub capped: bool,
    /// Rows where the script failed (left out).
    #[ts(type = "number")]
    pub errors: u64,
    /// The first failure.
    pub first_error: Option<String>,
}

/// The site map of the requests that meet conditions (M31b, TARGET-004).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptedSiteMap {
    /// Endpoints of the matching requests, and the marks.
    pub map: chosproxy_storage::SiteMap,
    /// Requests the conditions looked at.
    #[ts(type = "number")]
    pub scanned: u64,
    /// Only the newest requests were looked at (more exist).
    pub capped: bool,
    /// Requests where the conditions failed (left out).
    #[ts(type = "number")]
    pub errors: u64,
    /// The first failure.
    pub first_error: Option<String>,
}

/// Groups history rows into site map endpoints (scheme, host, port and
/// path without the query), ordered like the stored site map.
pub(super) fn site_entries(rows: &[HistoryRow]) -> Vec<chosproxy_storage::SiteMapEntry> {
    use chosproxy_storage::SiteMapEntry;
    use std::collections::BTreeMap;
    let mut by_key: BTreeMap<(String, u16, String, String), SiteMapEntry> = BTreeMap::new();
    for r in rows {
        let (path, has_query) = match r.path.split_once('?') {
            Some((p, _)) => (p.to_string(), true),
            None => (r.path.clone(), false),
        };
        let e = by_key
            .entry((r.host.clone(), r.port, r.scheme.clone(), path.clone()))
            .or_insert_with(|| SiteMapEntry {
                scheme: r.scheme.clone(),
                host: r.host.clone(),
                port: r.port,
                path,
                count: 0,
                methods: Vec::new(),
                statuses: Vec::new(),
                mimes: Vec::new(),
                sources: Vec::new(),
                has_params: false,
                first_seen: r.started_at,
                last_seen: r.started_at,
                last_id: r.id,
                in_scope: r.in_scope,
            });
        e.count += 1;
        if !e.methods.contains(&r.method) {
            e.methods.push(r.method.clone());
        }
        if let Some(status) = r.status
            && !e.statuses.contains(&status)
        {
            e.statuses.push(status);
        }
        if let Some(mime) = &r.mime
            && !e.mimes.contains(mime)
        {
            e.mimes.push(mime.clone());
        }
        if !e.sources.contains(&r.source) {
            e.sources.push(r.source);
        }
        e.has_params |= has_query;
        e.first_seen = e.first_seen.min(r.started_at);
        e.last_seen = e.last_seen.max(r.started_at);
        e.last_id = e.last_id.max(r.id);
    }
    let mut entries: Vec<SiteMapEntry> = by_key.into_values().collect();
    for e in &mut entries {
        e.methods.sort();
        e.statuses.sort_unstable();
        e.mimes.sort();
        e.sources.sort_by_key(|s| s.as_str());
    }
    entries
}

pub(crate) struct FilterCache {
    key: String,
    rows: Vec<HistoryRow>,
    scanned: u64,
    capped: bool,
    errors: u64,
    first_error: Option<String>,
}

/// The script view of a stored exchange.
pub(crate) fn message_of(ex: &StoredExchange) -> Message {
    let r = &ex.row;
    Message::new(MessageParts {
        scheme: &r.scheme,
        host: &r.host,
        port: r.port,
        path: &r.path,
        url: &r.url,
        mime: r.mime.as_deref(),
        request: &ex.request,
        response: ex.response.as_deref(),
    })
}

fn severity(s: &str) -> FindingSeverity {
    match s {
        "high" => FindingSeverity::High,
        "medium" => FindingSeverity::Medium,
        "low" => FindingSeverity::Low,
        _ => FindingSeverity::Info,
    }
}

fn confidence(c: &str) -> chosproxy_storage::Confidence {
    match c {
        "certain" => chosproxy_storage::Confidence::Certain,
        "firm" => chosproxy_storage::Confidence::Firm,
        _ => chosproxy_storage::Confidence::Tentative,
    }
}

/// Findings of the check scripts in one exchange. A failing script is
/// logged and skipped.
pub(crate) fn script_issues(
    ex: &StoredExchange,
    checks: &[(ScriptEntry, Arc<Script>)],
) -> Vec<PassiveIssue> {
    if checks.is_empty() {
        return Vec::new();
    }
    let msg = message_of(ex);
    let mut out = Vec::new();
    for (entry, script) in checks {
        match script.check(&msg) {
            Ok(run) => out.extend(run.value.into_iter().map(|f| {
                let mut dedup_key = format!("script:{}:{}", entry.id, f.key);
                dedup_key.truncate(
                    (0..=dedup_key.len().min(1000))
                        .rev()
                        .find(|i| dedup_key.is_char_boundary(*i))
                        .unwrap_or(0),
                );
                PassiveIssue {
                    source: "passive",
                    exchange_id: ex.row.id,
                    dedup_key,
                    kind: entry.name.clone(),
                    title: f.title,
                    severity: severity(&f.severity),
                    confidence: confidence(&f.confidence),
                    description: if f.detail.is_empty() {
                        format!(
                            "Reported by the check script “{}”. {}",
                            entry.name, entry.description
                        )
                    } else {
                        f.detail
                    },
                    remediation: f.remediation,
                    evidence_note: f.evidence,
                }
            })),
            Err(e) => {
                tracing::debug!(script = %entry.id, error = %e, "check script failed");
            }
        }
    }
    out
}

/// Converts a finding from `active_check` into a stored [`PassiveIssue`] with
/// `source: "scanner"`.
pub(crate) fn script_active_issue(
    entry: &ScriptEntry,
    exchange_id: i64,
    f: Finding,
) -> PassiveIssue {
    let mut dedup_key = format!("script:{}:{}", entry.id, f.key);
    dedup_key.truncate(
        (0..=dedup_key.len().min(1000))
            .rev()
            .find(|i| dedup_key.is_char_boundary(*i))
            .unwrap_or(0),
    );
    PassiveIssue {
        source: "scanner",
        exchange_id,
        dedup_key,
        kind: entry.name.clone(),
        title: f.title,
        severity: severity(&f.severity),
        confidence: confidence(&f.confidence),
        description: if f.detail.is_empty() {
            format!(
                "Reported by the active check script “{}”. {}",
                entry.name, entry.description
            )
        } else {
            f.detail
        },
        remediation: f.remediation,
        evidence_note: f.evidence,
    }
}

/// Output, findings and printed lines of one run.
type Outcome = (Option<String>, Vec<Finding>, Vec<String>);

fn run_once(script: &Script, msg: Option<&Message>) -> ScriptRun {
    let started = Instant::now();
    let mut run = ScriptRun::default();
    if script.kind() == ScriptKind::Payload {
        // Payload scripts need no exchange: try them on a sample value.
        match script.payload("admin") {
            Ok(v) => run.output = Some(format!("payload(\"admin\") → {v:?}")),
            Err(e) => run.problem = Some(e),
        }
        run.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        return run;
    }
    let Some(msg) = msg else {
        run.output = Some("Compiled. Choose an exchange to run it on.".into());
        return run;
    };
    let result: Result<Outcome, ScriptError> = match script.kind() {
        ScriptKind::Payload => Ok((None, vec![], vec![])),
        ScriptKind::Filter => script.filter(msg).map(|r| {
            (
                Some(if r.value { "keep" } else { "hide" }.into()),
                vec![],
                r.printed,
            )
        }),
        ScriptKind::Column => script
            .column(msg)
            .map(|r| (Some(r.value), vec![], r.printed)),
        ScriptKind::Action => script
            .action(msg)
            .map(|r| (Some(r.value), vec![], r.printed)),
        ScriptKind::Check => (|| {
            let mut findings = Vec::new();
            let mut printed = Vec::new();
            let mut output = None;
            if script.defines("check") {
                let r = script.check(msg)?;
                findings.extend(r.value);
                printed.extend(r.printed);
            }
            if script.defines("active_check") {
                let probes = script.active_probes("sample")?;
                let payload = probes.first().map_or("sample", String::as_str);
                let r = script.active_check(msg, msg, payload)?;
                output = Some(format!("active_probes(\"sample\") → {probes:?}"));
                findings.extend(r.value);
                printed.extend(r.printed);
            }
            Ok((output, findings, printed))
        })(),
        ScriptKind::Rewrite => (|| {
            let mut out = String::new();
            let mut printed = Vec::new();
            let request = msg_request(msg);
            let r = script.rewrite(false, &request)?;
            printed.extend(r.printed);
            out.push_str(&match r.value {
                Some(t) => format!("— Request becomes —\n{t}\n"),
                None => "— Request unchanged —\n".into(),
            });
            if let Some(response) = msg_response(msg) {
                let r = script.rewrite(true, &response)?;
                printed.extend(r.printed);
                out.push_str(&match r.value {
                    Some(t) => format!("— Response becomes —\n{t}"),
                    None => "— Response unchanged —".into(),
                });
            }
            Ok((Some(out), vec![], printed))
        })(),
    };
    match result {
        Ok((output, findings, printed)) => {
            run.output = output;
            run.findings = findings;
            run.printed = printed;
        }
        Err(e) => run.problem = Some(e),
    }
    run.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    run
}

fn msg_request(msg: &Message) -> String {
    msg.request_text()
}

fn msg_response(msg: &Message) -> Option<String> {
    msg.response_text()
}

impl Engine {
    pub(crate) fn library(&self) -> Arc<Library> {
        match self.inner.scripts.read() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The script library with compile problems.
    pub fn scripts(&self) -> ScriptLibrary {
        let lib = self.library();
        ScriptLibrary {
            config: lib.config.clone(),
            problems: lib
                .compiled
                .iter()
                .filter_map(|(id, c)| c.as_ref().err().map(|e| (id.clone(), e.clone())))
                .collect(),
        }
    }

    /// Saves the library (scripts that do not compile are kept, and
    /// reported) and applies it: checks, filters, columns, actions and
    /// rewrite rules use the new versions.
    pub async fn save_scripts(&self, config: ScriptsConfig) -> Result<ScriptLibrary, AppError> {
        self.inner.store.save(&config)?;
        let count = config.scripts.len();
        let lib = Arc::new(Library::build(config));
        match self.inner.scripts.write() {
            Ok(mut g) => *g = lib.clone(),
            Err(poisoned) => *poisoned.into_inner() = lib.clone(),
        }
        if let Ok(mut c) = self.inner.script_filter_cache.lock() {
            *c = None;
        }
        let rules = self.inner.proxy_config.lock().await.match_replace.clone();
        let resolver = |id: &str| lib.usable(id, ScriptKind::Rewrite);
        if let Err(e) = self.inner.proxy.set_match_replace_with(&rules, &resolver) {
            tracing::warn!(error = %e, "Match & Replace rules not applied");
        }
        self.audit(
            "scripts.changed",
            &format!("{count} script(s) in the library"),
        );
        Ok(self.scripts())
    }

    /// Writes scripts of the library (all when `ids` is empty) to a JSON
    /// file to share. Returns how many were written.
    pub fn export_scripts(
        &self,
        path: &std::path::Path,
        ids: &[String],
    ) -> Result<usize, AppError> {
        let lib = self.library();
        let scripts: Vec<ScriptEntry> = lib
            .config
            .scripts
            .iter()
            .filter(|s| ids.is_empty() || ids.contains(&s.id))
            .cloned()
            .collect();
        let file = ScriptFile {
            format: SCRIPT_FILE_FORMAT.into(),
            version: 1,
            scripts,
        };
        super::write_shared_file(path, &file, "scripts")?;
        Ok(file.scripts.len())
    }

    /// Adds the scripts of a shared file to the library. They arrive
    /// switched off, so nothing runs before the tester has read them; ids
    /// already in use get a suffix.
    pub async fn import_scripts(&self, path: &std::path::Path) -> Result<ScriptLibrary, AppError> {
        let file: ScriptFile = super::read_shared_file(path, 4, "scripts")?;
        if file.format != SCRIPT_FILE_FORMAT {
            return Err(AppError::Invalid("not a ChosProxy scripts file".into()));
        }
        let mut config = self.library().config.clone();
        for mut s in file.scripts {
            let base = s.id.clone();
            let mut n = 2;
            while config.get(&s.id).is_some() {
                s.id = format!("{}-{n}", base.chars().take(44).collect::<String>());
                n += 1;
            }
            s.enabled = false;
            config.scripts.push(s);
        }
        config.validate().map_err(AppError::Invalid)?;
        self.save_scripts(config).await
    }

    async fn stored(&self, id: i64) -> Result<StoredExchange, AppError> {
        let db = self.db()?;
        let mut found = tokio::task::spawn_blocking(move || db.exchanges_bulk(&[id])).await??;
        found
            .pop()
            .ok_or_else(|| AppError::Invalid(format!("no exchange {id}")))
    }

    /// Compiles `source` and runs it once on an exchange (or only compiles
    /// it without one). Nothing is saved or sent.
    pub async fn script_test(
        &self,
        kind: ScriptKind,
        source: String,
        exchange_id: Option<i64>,
    ) -> Result<ScriptRun, AppError> {
        let script = match Script::compile(kind, &source) {
            Ok(s) => s,
            Err(e) => {
                return Ok(ScriptRun {
                    problem: Some(e),
                    ..ScriptRun::default()
                });
            }
        };
        let msg = match exchange_id {
            Some(id) => Some(message_of(&self.stored(id).await?)),
            None => None,
        };
        Ok(tokio::task::spawn_blocking(move || run_once(&script, msg.as_ref())).await?)
    }

    /// Runs an action script on an exchange.
    pub async fn script_action(&self, id: &str, exchange_id: i64) -> Result<ScriptRun, AppError> {
        let script = self.library().script(id, ScriptKind::Action)?;
        let msg = message_of(&self.stored(exchange_id).await?);
        Ok(tokio::task::spawn_blocking(move || run_once(&script, Some(&msg))).await?)
    }

    /// Values of a column script for history rows (at most 1000).
    pub async fn script_column(
        &self,
        id: &str,
        exchange_ids: Vec<i64>,
    ) -> Result<Vec<ScriptCell>, AppError> {
        if exchange_ids.len() > 1000 {
            return Err(AppError::Invalid("at most 1000 rows at once".into()));
        }
        let script = self.library().script(id, ScriptKind::Column)?;
        let db = self.db()?;
        Ok(tokio::task::spawn_blocking(move || {
            let exchanges = db.exchanges_bulk(&exchange_ids)?;
            Ok::<_, chosproxy_storage::StorageError>(
                exchanges
                    .iter()
                    .map(|ex| match script.column(&message_of(ex)) {
                        Ok(r) => ScriptCell {
                            id: ex.row.id,
                            value: r.value,
                            error: None,
                        },
                        Err(e) => ScriptCell {
                            id: ex.row.id,
                            value: String::new(),
                            error: Some(e.to_string()),
                        },
                    })
                    .collect(),
            )
        })
        .await??)
    }

    /// A history page whose rows also pass a filter script. The newest
    /// [`SCRIPT_FILTER_SCAN`] rows matching the other filters are looked at
    /// once; later pages come from that result.
    pub async fn history_page_scripted(
        &self,
        query: HistoryQuery,
        script_id: &str,
    ) -> Result<ScriptedHistoryPage, AppError> {
        let script = self.library().script(script_id, ScriptKind::Filter)?;
        let key = format!("lib:{script_id}:{:p}", Arc::as_ptr(&self.library()));
        self.filtered_page(query, script, key).await
    }

    /// The same with a filter script given as source (the condition
    /// builder), compiled here and not saved.
    pub async fn history_page_conditions(
        &self,
        query: HistoryQuery,
        source: String,
    ) -> Result<ScriptedHistoryPage, AppError> {
        let script = Arc::new(
            Script::compile(ScriptKind::Filter, &source)
                .map_err(|e| AppError::Invalid(format!("conditions: {e}")))?,
        );
        self.filtered_page(query, script, format!("src:{source}"))
            .await
    }

    /// The site map of the requests that match `query` and meet the
    /// conditions in `source` (a filter script from the condition builder).
    pub async fn site_map_conditions(
        &self,
        query: HistoryQuery,
        source: String,
    ) -> Result<ScriptedSiteMap, AppError> {
        // Every matching row the filter keeps (it looks at a bounded
        // number of requests, newest first).
        let all = HistoryQuery {
            offset: 0,
            limit: u32::MAX,
            ..query
        };
        let scripted = self.history_page_conditions(all, source).await?;
        let db = self.db()?;
        let marks = tokio::task::spawn_blocking(move || db.site_marks()).await??;
        let mut entries = site_entries(&scripted.page.rows);
        let truncated = entries.len() > chosproxy_storage::SiteMap::MAX_ENTRIES as usize;
        entries.truncate(chosproxy_storage::SiteMap::MAX_ENTRIES as usize);
        Ok(ScriptedSiteMap {
            map: chosproxy_storage::SiteMap {
                entries,
                truncated,
                marks,
            },
            scanned: scripted.scanned,
            capped: scripted.capped,
            errors: scripted.errors,
            first_error: scripted.first_error,
        })
    }

    async fn filtered_page(
        &self,
        query: HistoryQuery,
        script: Arc<Script>,
        script_key: String,
    ) -> Result<ScriptedHistoryPage, AppError> {
        let db = self.db()?;
        let last = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || db.last_exchange_id()).await??
        };
        let (offset, limit) = (query.offset as usize, query.limit.max(1) as usize);
        let base = HistoryQuery {
            offset: 0,
            limit: SCAN_PAGE,
            ..query
        };
        let key = format!(
            "{script_key}|{}|{last}",
            serde_json::to_string(&base).unwrap_or_default(),
        );
        let cached = match self.inner.script_filter_cache.lock() {
            Ok(g) => g.as_ref().filter(|c| c.key == key).map(|c| {
                (
                    c.rows[offset.min(c.rows.len())..(offset + limit).min(c.rows.len())].to_vec(),
                    c.rows.len(),
                    c.scanned,
                    c.capped,
                    c.errors,
                    c.first_error.clone(),
                )
            }),
            Err(_) => None,
        };
        let (rows, total, scanned, capped, errors, first_error) = match cached {
            Some(c) => c,
            None => {
                let computed = tokio::task::spawn_blocking(move || {
                    let mut matched = Vec::new();
                    let (mut scanned, mut errors, mut first_error) = (0u64, 0u64, None);
                    let mut capped = false;
                    let mut q = base;
                    loop {
                        let page = db.history_page(&q)?;
                        let ids: Vec<i64> = page.rows.iter().map(|r| r.id).collect();
                        let exchanges = db.exchanges_bulk(&ids)?;
                        let by_id: HashMap<i64, &StoredExchange> =
                            exchanges.iter().map(|e| (e.row.id, e)).collect();
                        for row in &page.rows {
                            scanned += 1;
                            let Some(ex) = by_id.get(&row.id) else {
                                continue;
                            };
                            match script.filter(&message_of(ex)) {
                                Ok(r) if r.value => matched.push(row.clone()),
                                Ok(_) => {}
                                Err(e) => {
                                    errors += 1;
                                    first_error.get_or_insert_with(|| e.to_string());
                                }
                            }
                        }
                        let read = u64::from(q.offset) + page.rows.len() as u64;
                        if page.rows.len() < q.limit as usize || read >= page.total {
                            break;
                        }
                        if read >= u64::from(SCRIPT_FILTER_SCAN) {
                            capped = true;
                            break;
                        }
                        q.offset += q.limit;
                    }
                    Ok::<_, chosproxy_storage::StorageError>(FilterCache {
                        key,
                        rows: matched,
                        scanned,
                        capped,
                        errors,
                        first_error,
                    })
                })
                .await??;
                let out = (
                    computed.rows[offset.min(computed.rows.len())
                        ..(offset + limit).min(computed.rows.len())]
                        .to_vec(),
                    computed.rows.len(),
                    computed.scanned,
                    computed.capped,
                    computed.errors,
                    computed.first_error.clone(),
                );
                if let Ok(mut g) = self.inner.script_filter_cache.lock() {
                    *g = Some(computed);
                }
                out
            }
        };
        Ok(ScriptedHistoryPage {
            page: HistoryPage {
                total: total as u64,
                rows,
            },
            scanned,
            capped,
            errors,
            first_error,
        })
    }
}
