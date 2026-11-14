import { useEffect, useState } from "react";
import type { FindingDetail } from "../../bindings/FindingDetail";
import type { FindingSeverity } from "../../bindings/FindingSeverity";
import type { FindingStatus } from "../../bindings/FindingStatus";
import type { Confidence } from "../../bindings/Confidence";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { toggle } from "../../lib/filters";
import { formatDateTime } from "../../lib/format";
import { dialogs } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useAiStore } from "../../state/ai";
import { useFindingsStore } from "../../state/findings";
import { CONFIDENCES, SEVERITIES, STATUSES, statusLabel } from "./labels";
import { IssueDefinitionsDialog } from "./IssueDefinitionsDialog";
import { SuppressDialog, SuppressionsDialog } from "./SuppressDialogs";

const SEARCH_DEBOUNCE_MS = 250;

export function FindingsView() {
  const load = useFindingsStore((s) => s.load);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <div className="history-split">
      <SplitPane
        direction="vertical"
        initial={0.38}
        first={<FindingTable />}
        second={<FindingPanel />}
      />
    </div>
  );
}

function FindingFilters() {
  const filter = useFindingsStore((s) => s.filter);
  const total = useFindingsStore((s) => s.rows.length);
  const setFilter = useFindingsStore((s) => s.setFilter);
  const [search, setSearch] = useState(filter.search);
  useEffect(() => {
    if (search === filter.search) return;
    const t = window.setTimeout(() => setFilter({ search }), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [search, filter.search, setFilter]);
  return (
    <div className="filters" role="search">
      <label className="filter-search">
        <Icon name="search" size={14} />
        <input
          type="search"
          placeholder="Filter findings…"
          aria-label="Filter findings"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </label>
      <div className="chip-group" role="group" aria-label="Severity">
        {SEVERITIES.map((s) => (
          <button
            key={s}
            type="button"
            className={`chip sev-chip sev-${s}`}
            aria-pressed={filter.severities.includes(s)}
            onClick={() => setFilter({ severities: toggle(filter.severities, s) })}
          >
            {s}
          </button>
        ))}
      </div>
      <div className="chip-group" role="group" aria-label="Status">
        {STATUSES.map((s) => (
          <button
            key={s.value}
            type="button"
            className="chip"
            aria-pressed={filter.statuses.includes(s.value)}
            onClick={() => setFilter({ statuses: toggle(filter.statuses, s.value) })}
          >
            {s.label}
          </button>
        ))}
      </div>
      <Switch
        label="In scope only"
        checked={filter.inScopeOnly}
        onChange={(inScopeOnly) => setFilter({ inScopeOnly })}
      />
      <label className="finding-source">
        Source{" "}
        <select
          value={filter.source}
          onChange={(e) => setFilter({ source: e.target.value as typeof filter.source })}
        >
          <option value="">all</option>
          <option value="manual">documented by you</option>
          <option value="passive">passive analysis</option>
          <option value="plugin">plugins</option>
          <option value="oast">OAST interactions</option>
          <option value="dom">DOM Inspector</option>
        </select>
      </label>
      <span className="spacer" />
      <span className="muted">{total} findings</span>
    </div>
  );
}

/** Polling interval of the passive analysis status while the view is open. */
const PASSIVE_POLL_MS = 3000;

/** Where passive analysis stands, with its actions. */
function PassiveBar() {
  const passive = useFindingsStore((s) => s.passive);
  const { loadPassive, reanalyze } = useFindingsStore.getState();
  const [rules, setRules] = useState(false);
  const [definitions, setDefinitions] = useState(false);
  useEffect(() => {
    void loadPassive();
    const t = window.setInterval(() => void loadPassive(), PASSIVE_POLL_MS);
    return () => window.clearInterval(t);
  }, [loadPassive]);
  if (!passive) return null;
  const openSettings = () => useAppStore.getState().setView("settings");
  if (!passive.enabled) {
    return (
      <div className="passive-bar" role="status">
        <Icon name="shield" size={14} />
        <span>Passive analysis is off: new traffic is not checked.</span>
        <span className="spacer" />
        <Button size="sm" variant="ghost" onClick={openSettings}>
          Settings
        </Button>
      </div>
    );
  }
  const pending = Math.max(0, passive.lastExchangeId - passive.analyzedUpTo);
  const scope = !passive.inScopeOnly
    ? "all proxy traffic"
    : passive.scopeDefined
      ? "in-scope traffic"
      : "all traffic (no scope defined yet)";
  return (
    <div className="passive-bar" role="status">
      <Icon name="shield" size={14} />
      <span>
        Passive analysis of {scope}:{" "}
        {pending > 0 ? (
          <strong>{pending.toLocaleString()} requests to check…</strong>
        ) : (
          "up to date"
        )}
        {" · "}
        {passive.checksEnabled}/{passive.checksTotal} checks
      </span>
      <span className="spacer" />
      <Button
        size="sm"
        variant="ghost"
        title="Check every request of the project again (existing findings are kept and merged)"
        onClick={() => void reanalyze()}
      >
        Analyze all again
      </Button>
      <Button
        size="sm"
        variant="ghost"
        title="Rules that stop automatic checks from reporting false positives again"
        onClick={() => setRules(true)}
      >
        Suppressions
      </Button>
      <Button
        size="sm"
        variant="ghost"
        title="What each automatic check reports, why it matters and how to fix it"
        onClick={() => setDefinitions(true)}
      >
        Issue definitions
      </Button>
      <Button size="sm" variant="ghost" onClick={openSettings}>
        Settings
      </Button>
      {rules && <SuppressionsDialog onClose={() => setRules(false)} />}
      {definitions && <IssueDefinitionsDialog onClose={() => setDefinitions(false)} />}
    </div>
  );
}

function FindingTable() {
  const rows = useFindingsStore((s) => s.rows);
  const loaded = useFindingsStore((s) => s.loaded);
  const error = useFindingsStore((s) => s.error);
  const selectedId = useFindingsStore((s) => s.selectedId);
  const { select, load } = useFindingsStore.getState();
  return (
    <div className="organizer-list">
      <FindingFilters />
      <PassiveBar />
      {error ? (
        <ErrorState title="Could not load findings" message={error} onRetry={load} />
      ) : !loaded ? (
        <LoadingState />
      ) : rows.length === 0 ? (
        <EmptyState title="No findings">
          Passive analysis adds findings here as traffic goes through the proxy. You can also
          document a problem yourself with “Finding” on any request in HTTP history, the Logger or
          Search. Each finding keeps copies of the requests that show it.
        </EmptyState>
      ) : (
        <div className="endpoint-table-wrap">
          <table className="endpoint-table organizer-table">
            <thead>
              <tr>
                <th>Severity</th>
                <th>Confidence</th>
                <th>Title</th>
                <th>Type</th>
                <th>Host</th>
                <th>Path</th>
                <th className="num">Evidence</th>
                <th>Source</th>
                <th>Status</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((f) => (
                <tr
                  key={f.id}
                  className={selectedId === f.id ? "selected" : undefined}
                  aria-selected={selectedId === f.id}
                  onClick={() => select(f.id)}
                >
                  <td>
                    <span className={`sev sev-${f.severity}`}>{f.severity}</span>
                  </td>
                  <td className="muted">{f.confidence}</td>
                  <td>{f.title}</td>
                  <td className="muted">{f.kind}</td>
                  <td>
                    <span className={`scope-dot${f.inScope ? " in" : ""}`} /> {f.host}
                  </td>
                  <td className="mono" title={f.url}>
                    {f.path}
                  </td>
                  <td className="num">{f.evidenceCount}</td>
                  <td className="muted">{f.source}</td>
                  <td>
                    <span className={`org-status fstatus-${f.status}`}>
                      {statusLabel(f.status)}
                    </span>
                  </td>
                  <td className="muted">{formatDateTime(f.updatedAt)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function FindingPanel() {
  const detail = useFindingsStore((s) => s.detail);
  const select = useFindingsStore((s) => s.select);
  if (detail.status === "idle") return <EmptyState title="Select a finding" />;
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error") {
    return (
      <ErrorState
        title="Could not load the finding"
        message={detail.message}
        onRetry={() => select(detail.id)}
      />
    );
  }
  return (
    <FindingEditor
      key={`${detail.id}:${detail.detail.row.updatedAt}:${detail.detail.evidence.length}`}
      detail={detail.detail}
    />
  );
}

function FindingEditor({ detail }: { detail: FindingDetail }) {
  const { update, remove, removeEvidence } = useFindingsStore.getState();
  const { row } = detail;
  const [title, setTitle] = useState(row.title);
  const [kind, setKind] = useState(row.kind);
  const [severity, setSeverity] = useState<FindingSeverity>(row.severity);
  const [confidence, setConfidence] = useState<Confidence>(row.confidence);
  const [status, setStatus] = useState<FindingStatus>(row.status);
  const [description, setDescription] = useState(detail.description);
  const [remediation, setRemediation] = useState(detail.remediation);
  const [notes, setNotes] = useState(detail.notes);
  const [evidenceIndex, setEvidenceIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [suppress, setSuppress] = useState(false);
  const dirty =
    title !== row.title ||
    kind !== row.kind ||
    severity !== row.severity ||
    confidence !== row.confidence ||
    status !== row.status ||
    description !== detail.description ||
    remediation !== detail.remediation ||
    notes !== detail.notes;
  const evidence = detail.evidence[Math.min(evidenceIndex, detail.evidence.length - 1)];

  return (
    <SplitPane
      direction="horizontal"
      initial={0.45}
      first={
        <div className="finding-editor">
          <div className="finding-head">
            <input
              className="grow"
              aria-label="Title"
              value={title}
              maxLength={300}
              onChange={(e) => setTitle(e.target.value)}
            />
            <Badge>{row.source}</Badge>
          </div>
          <div className="dialog-row">
            <label>
              Severity{" "}
              <select
                value={severity}
                onChange={(e) => setSeverity(e.target.value as FindingSeverity)}
              >
                {SEVERITIES.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Confidence{" "}
              <select
                value={confidence}
                onChange={(e) => setConfidence(e.target.value as Confidence)}
              >
                {CONFIDENCES.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Status{" "}
              <select value={status} onChange={(e) => setStatus(e.target.value as FindingStatus)}>
                {STATUSES.map((s) => (
                  <option key={s.value} value={s.value}>
                    {s.label}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <input
            aria-label="Type"
            placeholder="Type"
            value={kind}
            maxLength={300}
            onChange={(e) => setKind(e.target.value)}
          />
          <p className="mono muted selectable">{row.url}</p>
          <label className="field-label" htmlFor="finding-description">
            Description
          </label>
          <textarea
            id="finding-description"
            rows={4}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <label className="field-label" htmlFor="finding-remediation">
            Remediation
          </label>
          <textarea
            id="finding-remediation"
            rows={3}
            value={remediation}
            onChange={(e) => setRemediation(e.target.value)}
          />
          <label className="field-label" htmlFor="finding-notes">
            Notes (how you verified it, next steps)
          </label>
          <textarea
            id="finding-notes"
            rows={3}
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
          />
          {error && <p className="field-error">{error}</p>}
          <div className="form-actions">
            <Button
              variant="primary"
              size="sm"
              disabled={!dirty}
              onClick={async () =>
                setError(
                  await update(row.id, {
                    kind,
                    title,
                    severity,
                    confidence,
                    status,
                    description,
                    remediation,
                    notes,
                  }),
                )
              }
            >
              Save
            </Button>
            {row.source !== "manual" && (
              <Button
                size="sm"
                variant="ghost"
                title="Stop automatic checks from reporting this type of finding on this host"
                onClick={() => setSuppress(true)}
              >
                Suppress similar…
              </Button>
            )}
            {suppress && <SuppressDialog row={row} onClose={() => setSuppress(false)} />}
            <Button
              size="sm"
              variant="ghost"
              title="Explain the finding, how to confirm and fix it, or draft report text"
              onClick={() =>
                useAiStore.getState().open({
                  title: `Finding #${row.id}`,
                  attachments: [{ kind: "finding", id: row.id }],
                  task: "explainFinding",
                })
              }
            >
              ChosAI…
            </Button>
            <span className="spacer" />
            <Button
              size="sm"
              variant="ghost"
              icon="trash"
              onClick={async () => {
                const ok = await dialogs.confirm(
                  `Delete finding #${row.id} and its evidence copies? To keep a record, mark it as a false positive instead.`,
                  "Delete finding?",
                  "Delete",
                );
                if (ok) setError(await remove(row.id));
              }}
            >
              Delete
            </Button>
          </div>
          <span className="muted">
            Created {formatDateTime(row.createdAt)} · updated {formatDateTime(row.updatedAt)}
          </span>
        </div>
      }
      second={
        <div className="finding-evidence">
          <div className="filters">
            <strong>Evidence</strong>
            <select
              aria-label="Evidence"
              value={evidenceIndex}
              onChange={(e) => setEvidenceIndex(Number(e.target.value))}
            >
              {detail.evidence.map((ev, i) => (
                <option key={ev.row.id} value={i}>
                  {i + 1}. {ev.row.method} {ev.row.url}
                  {ev.row.statusCode !== null ? ` → ${ev.row.statusCode}` : ""}
                </option>
              ))}
            </select>
            {evidence && (
              <IconButton
                icon="trash"
                label="Remove this evidence"
                onClick={() => void removeEvidence(evidence.row.id)}
              />
            )}
          </div>
          {!evidence ? (
            <EmptyState title="No evidence left" />
          ) : (
            <>
              {evidence.row.note && <p className="evidence-note selectable">{evidence.row.note}</p>}
              <div className="exchange-split">
                <SplitPane
                  direction="vertical"
                  first={
                    <MessageViewer
                      title="Request"
                      message={evidence.request}
                      emptyText="No request"
                    />
                  }
                  second={
                    <MessageViewer
                      title="Response"
                      message={evidence.response}
                      emptyText="No response"
                    />
                  }
                />
              </div>
            </>
          )}
        </div>
      }
    />
  );
}
