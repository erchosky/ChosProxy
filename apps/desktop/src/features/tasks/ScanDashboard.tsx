import { useEffect, useState } from "react";
import type { PhaseState } from "../../bindings/PhaseState";
import type { ScanSummary } from "../../bindings/ScanSummary";
import type { TaskRow } from "../../bindings/TaskRow";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { api } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { SEVERITIES } from "../findings/labels";

const PHASE_TONE: Record<PhaseState, "ok" | "warn" | "neutral" | "accent"> = {
  pending: "neutral",
  running: "accent",
  done: "ok",
  stopped: "warn",
};

const SEVERITY_TONE = {
  critical: "danger",
  high: "danger",
  medium: "warn",
  low: "accent",
  info: "neutral",
} as const;

/** The counters shown, in order: [label, value, explanation]. */
export function scanCounters(s: ScanSummary): [string, number, string][] {
  const base: [string, number, string][] = [
    ["Crawl requests", s.crawlRequests, `${s.crawlErrors} failed`],
    ["URLs found", s.crawlFound, "by the crawl"],
    ["Items read", s.itemsRead, "by the audit"],
    ["Audited", s.audited, "first item of each structural pattern"],
    ["Duplicates", s.duplicates, "same pattern as an audited item"],
    [
      "Skipped",
      s.noResponse + s.outOfScope,
      `${s.noResponse} no response, ${s.outOfScope} out of scope`,
    ],
    ["Scripts", s.scripts, "analyzed as JavaScript"],
  ];
  if (
    s.activeRequests > 0 ||
    s.activePoints > 0 ||
    s.activeErrors > 0 ||
    s.phases.some((p) => p.name === "active audit")
  ) {
    base.push(
      ["Active probes", s.activeRequests, `${s.activeErrors} failed, ${s.activeSkipped} skipped`],
      ["Points tested", s.activePoints, "insertion points audited actively"],
    );
  }
  base.push(["Findings", s.findingsCreated, `created; ${s.findingsUpdated} updated`]);
  return base;
}

/** A scan's dashboard (M26): phases, counters and issues by severity. */
export function ScanDashboard({ task }: { task: TaskRow }) {
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  useEffect(() => {
    let live = true;
    api
      .scanSummary(task.id)
      .then((s) => live && setSummary(s))
      .catch(() => live && setSummary(null));
    return () => {
      live = false;
    };
  }, [task.id, task.progress, task.status, task.requests]);
  if (!summary) return null;
  return (
    <section className="scan-dashboard" aria-label="Scan dashboard">
      <ol className="scan-phases" aria-label="Phases">
        {summary.phases.map((p) => (
          <li key={p.name}>
            <Badge tone={PHASE_TONE[p.state]}>{p.state}</Badge> {p.name}
          </li>
        ))}
      </ol>
      <dl className="scan-counters">
        {scanCounters(summary).map(([label, value, detail]) => (
          <div key={label} title={detail}>
            <dt>{label}</dt>
            <dd>{value.toLocaleString()}</dd>
          </div>
        ))}
      </dl>
      <div className="scan-severities">
        <span className="muted">Issues reported:</span>
        {SEVERITIES.filter((s) => summary.issuesBySeverity[s]).map((s) => (
          <Badge key={s} tone={SEVERITY_TONE[s]}>
            {summary.issuesBySeverity[s]} {s}
          </Badge>
        ))}
        {Object.keys(summary.issuesBySeverity).length === 0 && <span className="muted">none</span>}
        <span className="spacer" />
        <Button
          size="sm"
          variant="ghost"
          icon="shield"
          onClick={() => useAppStore.getState().setView("findings")}
        >
          Open Findings
        </Button>
      </div>
    </section>
  );
}
