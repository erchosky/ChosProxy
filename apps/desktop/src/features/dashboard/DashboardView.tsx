import { useEffect } from "react";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { LoadingState } from "../../components/States";
import { useAppStore } from "../../state/app";
import { activeIncludes, useScopeStore } from "../../state/scope";
import { modLabel } from "../../app/shortcuts";
import { ListenerControls } from "../proxy/ListenerControls";

function Metric({ label, value, hint }: { label: string; value: number | string; hint?: string }) {
  return (
    <div className="metric" title={hint}>
      <div className="metric-value">
        {typeof value === "number" ? value.toLocaleString() : value}
      </div>
      <div className="metric-label">{label}</div>
    </div>
  );
}

export function DashboardView() {
  const stats = useAppStore((s) => s.stats);
  const listeners = useAppStore((s) => s.listeners);
  const notices = useAppStore((s) => s.notices);
  const project = useAppStore((s) => s.project);
  const refreshStats = useAppStore((s) => s.refreshStats);
  const setView = useAppStore((s) => s.setView);
  const closeProject = useAppStore((s) => s.closeProject);
  const ca = useAppStore((s) => s.ca);
  const browser = useAppStore((s) => s.browser);
  const browsers = useAppStore((s) => s.browsers);
  const busy = useAppStore((s) => s.busy);
  const scopeRules = useScopeStore((s) => activeIncludes(s.config));
  const launchBrowser = useAppStore((s) => s.launchBrowser);

  useEffect(() => {
    void refreshStats();
  }, [refreshStats]);

  const running = listeners.find((l) => l.state.state === "running");
  const address = running?.state.state === "running" ? running.state.address : null;
  const problems = notices.filter((n) => n.level !== "info").slice(0, 5);

  return (
    <div className="page dashboard">
      <div className="page-header">
        <div>
          <h1>{project?.name}</h1>
          <p className="page-sub selectable">{project?.path}</p>
        </div>
        <Button onClick={() => void closeProject()}>Close project</Button>
      </div>

      <div className="dashboard-grid">
        <section className="card">
          <h2>Traffic</h2>
          {stats ? (
            <div className="metrics">
              <Metric label="Requests" value={stats.project.exchanges} />
              <Metric label="Hosts" value={stats.project.hosts} />
              <Metric
                label="Endpoints"
                value={stats.project.endpoints}
                hint="Distinct host + path"
              />
              <Metric label="Errors" value={stats.project.errors} hint="Exchanges that failed" />
              <Metric
                label="HTTPS inspected"
                value={stats.proxy.intercepted}
                hint="HTTPS connections decrypted with the local CA"
              />
              <Metric
                label="Not decrypted"
                value={stats.proxy.tunnels}
                hint="Passthrough hosts, interception off, or non-HTTP protocols"
              />
              <Metric
                label="HTTPS rejected"
                value={stats.proxy.tlsClientFailures}
                hint="Clients that do not trust the local CA"
              />
              <Metric label="Open connections" value={stats.proxy.activeConnections} />
              <Metric
                label="Findings (high+)"
                value={stats.findings.high + stats.findings.critical}
                hint={`Open or confirmed: ${stats.findings.critical} critical, ${stats.findings.high} high, ${stats.findings.medium} medium, ${stats.findings.low} low, ${stats.findings.info} info`}
              />
              <Metric
                label="Findings (all open)"
                value={
                  stats.findings.critical +
                  stats.findings.high +
                  stats.findings.medium +
                  stats.findings.low +
                  stats.findings.info
                }
              />
            </div>
          ) : (
            <LoadingState />
          )}
          {stats && stats.capturesDropped > 0 && (
            <p className="callout callout-warn">
              {stats.capturesDropped} exchanges were forwarded but could not be stored.
            </p>
          )}
          <Button size="sm" onClick={() => setView("history")}>
            Open HTTP history
          </Button>
        </section>

        <section className="card">
          <h2>Proxy listeners</h2>
          <ListenerControls />
        </section>

        <section className="card">
          <h2>Getting started</h2>
          <ol className="checklist">
            <li data-done={address !== null}>
              Proxy listening{" "}
              {address ? <code>{address}</code> : <Badge tone="warn">not running</Badge>}
            </li>
            <li data-done={ca?.available ?? false}>
              Local CA{" "}
              {ca?.available ? (
                <Badge tone="ok">ready</Badge>
              ) : (
                <Badge tone="danger">unavailable — see HTTPS &amp; browser</Badge>
              )}
            </li>
            <li data-done={browser?.running ?? false}>
              {browser?.running ? (
                <>Lab browser running ({browser.browser?.name}).</>
              ) : (
                <>
                  Open the isolated lab browser{" "}
                  <Button
                    size="sm"
                    variant="primary"
                    icon="play"
                    disabled={address === null || browsers.length === 0 || busy !== null}
                    onClick={() => void launchBrowser(null, null)}
                  >
                    Open browser
                  </Button>
                  {browsers.length === 0 && (
                    <>
                      {" "}
                      — no supported browser found (<code>sudo apt install chromium</code>)
                    </>
                  )}
                </>
              )}
            </li>
            <li data-done={scopeRules > 0}>
              {scopeRules > 0 ? (
                <>
                  Scope defined ({scopeRules} rule{scopeRules === 1 ? "" : "s"}).
                </>
              ) : (
                <>
                  Define the scope: only what you are authorized to test.{" "}
                  <Button size="sm" onClick={() => setView("scope")}>
                    Edit scope
                  </Button>
                </>
              )}
            </li>
            <li>Browse the target; HTTP and HTTPS requests appear in HTTP history.</li>
            <li>
              Using another browser? Point it at <code>{address ?? "the listener"}</code> and trust
              the CA from <code>http://chosproxy/</code> (testing profile only).
            </li>
          </ol>
          <p className="muted">
            <kbd>{modLabel()}+K</kbd> runs any command; <kbd>?</kbd> lists the keyboard shortcuts.
          </p>
        </section>

        <section className="card">
          <h2>Recent problems</h2>
          {problems.length === 0 ? (
            <p className="muted">No problems reported.</p>
          ) : (
            <ul className="problem-list">
              {problems.map((n) => (
                <li key={n.id} className={`notice-${n.level} selectable`}>
                  {n.message}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}
