import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import { useAppStore } from "../state/app";
import { useInterceptStore } from "../state/intercept";
import { activeIncludes, useScopeStore } from "../state/scope";

export function TopBar() {
  const project = useAppStore((s) => s.project);
  const info = useAppStore((s) => s.info);
  const listeners = useAppStore((s) => s.listeners);
  const setPaletteOpen = useAppStore((s) => s.setPaletteOpen);
  const browser = useAppStore((s) => s.browser);
  const browsers = useAppStore((s) => s.browsers);
  const busy = useAppStore((s) => s.busy);
  const launchBrowser = useAppStore((s) => s.launchBrowser);
  const closeBrowser = useAppStore((s) => s.closeBrowser);
  const setView = useAppStore((s) => s.setView);
  const intercept = useInterceptStore((s) => s.state);
  const setIntercept = useInterceptStore((s) => s.setEnabled);
  const scopeRules = useScopeStore((s) => activeIncludes(s.config));
  const running = listeners.filter((l) => l.state.state === "running");
  const failed = listeners.some((l) => l.state.state === "failed");
  const isMac = navigator.platform.toLowerCase().includes("mac");

  return (
    <header className="topbar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true">
          ‹•›
        </span>
        <span>{info?.productName}</span>
      </div>
      {project && (
        <div className="project-pill" title={project.path}>
          <Icon name={project.kind === "temporary" ? "temp" : "file"} size={14} />
          <span>{project.name}</span>
          {project.kind === "temporary" && <Badge>temporary</Badge>}
          {project.readOnly && <Badge tone="warn">read-only</Badge>}
        </div>
      )}
      <div className="proxy-pill" title="Proxy listeners">
        <span className={`dot ${running.length ? "dot-ok" : failed ? "dot-danger" : ""}`} />
        {running.length
          ? running.map((l) => (l.state.state === "running" ? l.state.address : "")).join(", ")
          : failed
            ? "Proxy failed to start"
            : "Proxy stopped"}
      </div>
      {project && !project.readOnly && (
        <button
          type="button"
          className={`intercept-pill${intercept.enabled ? " on" : ""}`}
          aria-pressed={intercept.enabled}
          title="Toggle Intercept (pause matching messages)"
          onClick={() => void setIntercept(!intercept.enabled)}
        >
          <Icon name="pause" size={13} />
          Intercept {intercept.enabled ? "on" : "off"}
          {intercept.pending.length > 0 && (
            <span
              className="pill-count"
              role="link"
              onClick={(e) => {
                e.stopPropagation();
                setView("intercept");
              }}
            >
              {intercept.pending.length} waiting
            </span>
          )}
        </button>
      )}
      {project && (
        <button
          type="button"
          className={`scope-pill${scopeRules > 0 ? " on" : ""}`}
          title="Edit the scope"
          onClick={() => setView("scope")}
        >
          <Icon name="target" size={13} />
          {scopeRules > 0
            ? `Scope: ${scopeRules} rule${scopeRules === 1 ? "" : "s"}`
            : "No scope defined"}
        </button>
      )}
      <span className="spacer" />
      {project &&
        (browser?.running ? (
          <Button
            size="sm"
            icon="stop"
            disabled={busy !== null}
            onClick={() => void closeBrowser()}
          >
            Close {browser.browser?.name ?? "browser"}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="primary"
            icon="play"
            disabled={busy !== null || running.length === 0}
            title={running.length === 0 ? "Start a proxy listener first" : undefined}
            onClick={() =>
              browsers.length === 0 ? setView("https") : void launchBrowser(null, null)
            }
          >
            Open browser
          </Button>
        ))}
      <button type="button" className="palette-trigger" onClick={() => setPaletteOpen(true)}>
        <Icon name="search" size={14} />
        <span>Commands</span>
        <kbd>{isMac ? "⌘" : "Ctrl"} K</kbd>
      </button>
    </header>
  );
}
