import { useCallback, useEffect, useState } from "react";
import type { TempCleanup } from "../../bindings/TempCleanup";
import type { TempKind } from "../../bindings/TempKind";
import type { TempStatus } from "../../bindings/TempStatus";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { formatBytes, formatDateTime } from "../../lib/format";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

const KIND: Record<TempKind, string> = {
  temporaryProject: "Temporary project",
  browserProfile: "Lab browser profile",
  reportStaging: "Report being written",
  other: "Not ChosProxy's",
};

/** A short summary of a cleanup for a notice. */
export function cleanupSummary(done: TempCleanup): string {
  const freed =
    done.removed === 0
      ? "Nothing to remove."
      : `Removed ${done.removed} item(s), ${formatBytes(done.bytes)} freed.`;
  return done.failed.length === 0 ? freed : `${freed} Kept: ${done.failed.join("; ")}`;
}

/** Settings → Storage (M31): ChosProxy's temporary directory. */
export function StorageSection() {
  const [status, setStatus] = useState<TempStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const notify = useAppStore((s) => s.notify);

  const load = useCallback(async () => {
    try {
      setStatus(await api.tempStatus());
      setError(null);
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }, []);
  useEffect(() => {
    let alive = true;
    api.tempStatus().then(
      (s) => alive && setStatus(s),
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
  }, []);

  async function act(run: () => Promise<TempCleanup>) {
    setBusy(true);
    try {
      notify("info", cleanupSummary(await run()));
      await load();
    } catch (e) {
      notify("error", toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  if (error) return <ErrorState title="Could not read the temporary files" message={error} />;
  if (!status) return <LoadingState />;
  const old = status.entries.filter((e) => e.removable);
  return (
    <section className="card" aria-label="Storage">
      <h2>Storage</h2>
      <p className="muted rule-hint">
        Temporary projects and lab browser profiles live in{" "}
        <code className="selectable">{status.dir}</code> ({formatBytes(status.totalBytes)}).
        ChosProxy removes its own leftovers when it closes. Files from other runs — perhaps another
        ChosProxy open right now — are only removed when you ask, once unchanged for a day. Files it
        did not make, and project files elsewhere, are never touched.
      </p>
      <div className="form-actions">
        <Button
          size="sm"
          variant="primary"
          disabled={busy || status.cleanableBytes === 0}
          onClick={() => void act(() => api.tempCleanup())}
        >
          Clean now ({formatBytes(status.cleanableBytes)})
        </Button>
        {old.length > 0 && (
          <Button
            size="sm"
            disabled={busy}
            onClick={() =>
              void (async () => {
                const ok = await dialogs.confirm(
                  `Remove ${old.length} leftover(s) of earlier runs (${formatBytes(
                    old.reduce((n, e) => n + e.bytes, 0),
                  )})? Temporary projects among them cannot be recovered.`,
                  "Remove old leftovers?",
                  "Remove",
                );
                if (ok) await act(() => api.tempRemove(old.map((e) => e.name)));
              })()
            }
          >
            Remove old leftovers…
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void load()}>
          Refresh
        </Button>
      </div>
      {status.entries.length > 0 && (
        <table className="endpoint-table" aria-label="Temporary files">
          <thead>
            <tr>
              <th>Name</th>
              <th>Kind</th>
              <th>Size</th>
              <th>Changed</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {status.entries.map((e) => (
              <tr key={e.name}>
                <td className="mono">{e.name}</td>
                <td>{KIND[e.kind]}</td>
                <td>{formatBytes(e.bytes)}</td>
                <td>{e.modifiedAt ? formatDateTime(e.modifiedAt) : ""}</td>
                <td>
                  {e.inUse ? (
                    <Badge tone="ok">in use</Badge>
                  ) : e.cleanable ? (
                    <Badge>this run</Badge>
                  ) : e.removable ? (
                    <Badge tone="warn">old leftover</Badge>
                  ) : e.kind === "other" ? (
                    <Badge>kept</Badge>
                  ) : (
                    <Badge>other run</Badge>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {status.more > 0 && <p className="muted">{status.more} more not listed.</p>}
    </section>
  );
}
