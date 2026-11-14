import { useEffect, useState } from "react";
import type { ReportFormat } from "../../bindings/ReportFormat";
import { Button } from "../../components/Button";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useReportStore } from "../../state/report";
import { SEVERITIES, statusLabel } from "../findings/labels";

const PREVIEW_DELAY_MS = 400;

const FORMATS: { id: ReportFormat; label: string }[] = [
  { id: "html", label: "Export HTML…" },
  { id: "markdown", label: "Export Markdown…" },
  { id: "json", label: "Export JSON…" },
];

/** Builds an assessment report from chosen findings, with a live preview. */
export function ReportView() {
  const project = useAppStore((s) => s.project);
  const load = useReportStore((s) => s.load);
  useEffect(() => {
    void load();
  }, [load, project?.path]);
  if (!project) return null;
  return (
    <div className="history-split">
      <SplitPane
        direction="horizontal"
        initial={0.42}
        first={<ReportForm />}
        second={<ReportPreview />}
      />
    </div>
  );
}

function ReportForm() {
  const form = useReportStore((s) => s.form);
  const patch = useReportStore((s) => s.patch);
  const error = useReportStore((s) => s.error);
  const exporting = useReportStore((s) => s.exporting);
  const exportAs = useReportStore((s) => s.exportAs);
  const selected = useReportStore((s) => s.selected);
  return (
    <div className="report-form finding-editor">
      <h2>Report</h2>
      <label>
        <span className="field-label">Title</span>
        <input value={form.title} onChange={(e) => patch({ title: e.target.value })} />
      </label>
      <div className="dialog-row">
        <label className="report-grow">
          <span className="field-label">Prepared for</span>
          <input
            value={form.client}
            placeholder="Client or system owner"
            onChange={(e) => patch({ client: e.target.value })}
          />
        </label>
        <label className="report-grow">
          <span className="field-label">Prepared by</span>
          <input
            value={form.author}
            placeholder="Tester or team"
            onChange={(e) => patch({ author: e.target.value })}
          />
        </label>
      </div>
      <label>
        <span className="field-label">Executive summary</span>
        <textarea
          rows={4}
          value={form.executiveSummary}
          placeholder="What was tested, the overall result and the most important risks."
          onChange={(e) => patch({ executiveSummary: e.target.value })}
        />
      </label>
      <label>
        <span className="field-label">Methodology</span>
        <textarea
          rows={3}
          value={form.methodology}
          onChange={(e) => patch({ methodology: e.target.value })}
        />
      </label>
      <FindingChooser />
      <h3>Evidence</h3>
      <div className="dialog-row">
        <Switch
          label="Include requests and responses"
          checked={form.includeEvidence}
          onChange={(includeEvidence) => patch({ includeEvidence })}
        />
        <Switch
          label="Redact credentials"
          checked={form.redactSecrets}
          disabled={!form.includeEvidence}
          onChange={(redactSecrets) => patch({ redactSecrets })}
        />
        <label>
          Up to{" "}
          <KibInput
            value={form.maxEvidenceKib}
            disabled={!form.includeEvidence}
            onChange={(maxEvidenceKib) => patch({ maxEvidenceKib })}
          />{" "}
          KiB per message
        </label>
      </div>
      <p className="field-help">
        Redaction replaces the values of Authorization, Cookie, Set-Cookie and API key or token
        headers. Bodies are not changed: review the evidence before sharing the report.
      </p>
      {form.includeEvidence && !form.redactSecrets && (
        <p className="callout callout-warn">
          Credentials in the captured headers will appear in the report as they were sent.
        </p>
      )}
      {error && <p className="field-error">{error}</p>}
      <div className="form-actions">
        {FORMATS.map((f, i) => (
          <Button
            key={f.id}
            variant={i === 0 ? "primary" : "secondary"}
            size="sm"
            disabled={exporting !== null || selected === null}
            onClick={() => void exportAs(f.id)}
          >
            {f.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

/** Size limit field. The text being typed is kept apart from the value so
 * the field can be emptied and retyped; it snaps to a valid value on blur. */
function KibInput(props: { value: number; disabled: boolean; onChange(value: number): void }) {
  const [text, setText] = useState<string | null>(null);
  return (
    <input
      type="number"
      className="report-kib"
      min={1}
      max={1024}
      value={text ?? props.value}
      disabled={props.disabled}
      onChange={(e) => {
        setText(e.target.value);
        const n = Number.parseInt(e.target.value, 10);
        if (Number.isFinite(n)) props.onChange(Math.min(1024, Math.max(1, n)));
      }}
      onBlur={() => setText(null)}
    />
  );
}

function FindingChooser() {
  const findings = useReportStore((s) => s.findings);
  const selected = useReportStore((s) => s.selected);
  const setSelected = useReportStore((s) => s.setSelected);
  const chooseDefault = useReportStore((s) => s.chooseDefault);
  if (selected === null) return <LoadingState />;
  const chosen = new Set(selected);
  const sorted = [...findings].sort(
    (a, b) =>
      SEVERITIES.indexOf(a.severity) - SEVERITIES.indexOf(b.severity) ||
      a.title.localeCompare(b.title),
  );
  return (
    <fieldset className="report-findings">
      <legend className="field-label">
        Findings ({selected.length} of {findings.length})
      </legend>
      {findings.length === 0 ? (
        <p className="muted">
          No findings yet. Document them from HTTP history with the "Finding" action.
        </p>
      ) : (
        <>
          <div className="dialog-row">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setSelected(findings.map((f) => f.id))}
            >
              All
            </Button>
            <Button size="sm" variant="ghost" onClick={chooseDefault}>
              All but false positives
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setSelected([])}>
              None
            </Button>
          </div>
          <ul className="report-finding-list">
            {sorted.map((f) => (
              <li key={f.id}>
                <label>
                  <input
                    type="checkbox"
                    checked={chosen.has(f.id)}
                    onChange={(e) =>
                      setSelected(
                        e.target.checked
                          ? [...selected, f.id]
                          : selected.filter((id) => id !== f.id),
                      )
                    }
                  />
                  <span className={`sev-${f.severity} report-sev`}>{f.severity}</span>
                  <span className="report-title">{f.title}</span>
                  <span className="muted">{statusLabel(f.status)}</span>
                </label>
              </li>
            ))}
          </ul>
        </>
      )}
    </fieldset>
  );
}

function ReportPreview() {
  const form = useReportStore((s) => s.form);
  const selected = useReportStore((s) => s.selected);
  const [html, setHtml] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (selected === null) return;
    let stale = false;
    const t = window.setTimeout(() => {
      api
        .reportPreview({ ...form, findingIds: selected })
        .then((h) => {
          if (stale) return;
          setHtml(h);
          setError(null);
        })
        .catch((e: unknown) => {
          if (!stale) setError(toIpcError(e).message);
        });
    }, PREVIEW_DELAY_MS);
    return () => {
      stale = true;
      window.clearTimeout(t);
    };
  }, [form, selected, attempt]);

  if (error)
    return (
      <ErrorState
        title="Could not build the preview"
        message={error}
        onRetry={() => setAttempt((n) => n + 1)}
      />
    );
  if (html === null) return <LoadingState />;
  if (selected?.length === 0 && !form.executiveSummary.trim())
    return (
      <EmptyState title="Nothing to report yet">
        Choose findings and write a summary; the preview updates as you type.
      </EmptyState>
    );
  return (
    <div className="render-view">
      <p className="render-note muted">
        Preview of the HTML report. The exported file has no scripts and loads nothing from the
        network.
      </p>
      {/* No allow-* token: no scripts, forms, popups or same-origin access. */}
      <iframe title="Report preview" sandbox="" referrerPolicy="no-referrer" srcDoc={html} />
    </div>
  );
}
