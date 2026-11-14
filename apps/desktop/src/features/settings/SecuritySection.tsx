import { useCallback, useEffect, useState } from "react";
import type { SecurityCheckStatus } from "../../bindings/SecurityCheckStatus";
import type { SecurityOverview } from "../../bindings/SecurityOverview";
import { Button } from "../../components/Button";
import { api, toIpcError } from "../../services/ipc";
import { type ViewId, useAppStore } from "../../state/app";

const STATUS_LABEL: Record<SecurityCheckStatus, string> = {
  ok: "OK",
  warn: "Warning",
  info: "Info",
  unavailable: "Unavailable",
};

const STATUS_CLASS: Record<SecurityCheckStatus, string> = {
  ok: "sev sev-info",
  warn: "sev sev-medium",
  info: "sev sev-low",
  unavailable: "muted",
};

export function SecuritySection() {
  const setView = useAppStore((s) => s.setView);
  const project = useAppStore((s) => s.project);
  const [overview, setOverview] = useState<SecurityOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const next = await api.securityOverview();
      setOverview(next);
      setError(null);
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let active = true;
    api
      .securityOverview()
      .then((next) => {
        if (!active) return;
        setOverview(next);
        setError(null);
      })
      .catch((e) => {
        if (!active) return;
        setError(toIpcError(e).message);
      });
    return () => {
      active = false;
    };
  }, [project?.path, project?.encrypted, project?.readOnly]);

  return (
    <section className="card" id="security-center" aria-label="Security Center">
      <div className="row" style={{ justifyContent: "space-between", alignItems: "center" }}>
        <div>
          <h2>Security Center</h2>
          <p className="field-help">
            Live local security and privacy posture across certificates, listeners, Automation API,
            plugins, ChosAI/ChosAgent, network rules, ChosOAST, TLS verification, project
            encryption, temporary files, and credential storage.
          </p>
        </div>
        <Button size="sm" variant="ghost" disabled={loading} onClick={() => void refresh()}>
          {loading ? "Checking…" : "Refresh"}
        </Button>
      </div>

      {error && <p className="field-error">{error}</p>}

      {overview && (
        <>
          <div className="row" style={{ gap: 12, marginBottom: 12 }}>
            <span className="sev sev-info">{overview.okCount} OK</span>
            <span className="sev sev-medium">{overview.warnCount} Warning</span>
            <span className="sev sev-low">{overview.infoCount} Info</span>
            {overview.unavailableCount > 0 && (
              <span className="muted">{overview.unavailableCount} Unavailable</span>
            )}
          </div>

          <table className="endpoint-table" aria-label="Security checks">
            <thead>
              <tr>
                <th>Status</th>
                <th>Area</th>
                <th>Summary & details</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {overview.checks.map((c) => (
                <tr key={c.id}>
                  <td>
                    <span className={STATUS_CLASS[c.status]}>{STATUS_LABEL[c.status]}</span>
                  </td>
                  <td>
                    <strong>{c.title}</strong>
                  </td>
                  <td>
                    <div>{c.summary}</div>
                    <div className="muted" style={{ fontSize: "0.85em" }}>
                      {c.detail}
                    </div>
                  </td>
                  <td>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Review ${c.title}`}
                      onClick={() => setView(c.targetView as ViewId)}
                    >
                      Review
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
}
