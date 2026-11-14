import { useEffect, useState } from "react";
import type { FindingRow } from "../../bindings/FindingRow";
import type { SuppressionRule } from "../../bindings/SuppressionRule";
import { Button, IconButton } from "../../components/Button";
import { EmptyState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { formatDateTime } from "../../lib/format";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useFindingsStore } from "../../state/findings";

/** "Suppress similar" (M25): findings of this type on this host (and path)
 * are false positives; automatic sources stop reporting them. */
export function SuppressDialog({ row, onClose }: { row: FindingRow; onClose(): void }) {
  const [host, setHost] = useState(row.host);
  const [pathPrefix, setPathPrefix] = useState("");
  const [reason, setReason] = useState("");
  const [markOpen, setMarkOpen] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function add() {
    setBusy(true);
    setError(null);
    try {
      const marked = await api.suppressionAdd(
        { kind: row.kind, host, pathPrefix, reason },
        markOpen,
      );
      useAppStore
        .getState()
        .notify(
          "info",
          marked > 0
            ? `Rule added; ${marked} open finding(s) marked as false positives`
            : "Rule added",
        );
      await useFindingsStore.getState().load();
      onClose();
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-form"
        role="dialog"
        aria-modal="true"
        aria-label="Suppress similar findings"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Suppress “{row.kind}”</strong>
        </header>
        <p className="muted">
          Passive analysis, scans and plugins will not report this type of finding where the rule
          matches. Findings you created by hand are never affected. Delete the rule to report them
          again.
        </p>
        <div className="dialog-row">
          <label className="inline-field">
            Host
            <input
              value={host}
              placeholder="any host"
              onChange={(e) => setHost(e.target.value)}
              title="app.example, *.example for its subdomains, or empty for any host"
            />
          </label>
          <label className="inline-field">
            Path prefix
            <input
              value={pathPrefix}
              placeholder="any path (e.g. /static/)"
              onChange={(e) => setPathPrefix(e.target.value)}
            />
          </label>
        </div>
        <label className="field-stack">
          Reason
          <input
            value={reason}
            placeholder="Why these are false positives (kept in the notes)"
            onChange={(e) => setReason(e.target.value)}
          />
        </label>
        <Switch
          checked={markOpen}
          onChange={setMarkOpen}
          label="Mark the open findings it matches as false positives"
        />
        {error && <p className="callout callout-danger">{error}</p>}
        <div className="form-actions">
          <span className="spacer" />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void add()}>
            Add rule
          </Button>
        </div>
      </div>
    </div>
  );
}

/** The project's suppression rules, to review and delete. */
export function SuppressionsDialog({ onClose }: { onClose(): void }) {
  const [rules, setRules] = useState<SuppressionRule[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = () =>
    api
      .suppressions()
      .then(setRules)
      .catch((e: unknown) => setError(toIpcError(e).message));

  useEffect(() => {
    api
      .suppressions()
      .then(setRules)
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, []);

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-wide"
        role="dialog"
        aria-modal="true"
        aria-label="Suppression rules"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Suppression rules</strong>
          <Button size="sm" variant="ghost" onClick={onClose}>
            Close
          </Button>
        </header>
        {error && <p className="callout callout-danger">{error}</p>}
        {rules && rules.length === 0 && (
          <EmptyState title="No suppression rules">
            Open a finding and choose “Suppress similar…” to stop automatic checks from reporting it
            again.
          </EmptyState>
        )}
        {rules && rules.length > 0 && (
          <ul className="suppression-list">
            {rules.map((r) => (
              <li key={r.id} className="dialog-row">
                <span className="grow">
                  <strong>{r.kind}</strong> on {r.host || "any host"}
                  {r.pathPrefix && <span className="mono"> {r.pathPrefix}…</span>}
                  {r.reason && <span className="muted"> — {r.reason}</span>}
                  <span className="muted"> · {formatDateTime(r.createdAt)}</span>
                </span>
                <IconButton
                  icon="trash"
                  label={`Delete the rule for ${r.kind}`}
                  onClick={() =>
                    void api
                      .suppressionDelete(r.id)
                      .then(reload)
                      .catch((e: unknown) => setError(toIpcError(e).message))
                  }
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
