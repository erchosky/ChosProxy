import { useEffect, useState } from "react";
import type { ApiStatus } from "../../bindings/ApiStatus";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { copyText } from "../../services/clipboard";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

/** An example call for the docs line under the token. */
export function curlExample(status: Pick<ApiStatus, "config">): string {
  return `curl -H "Authorization: Bearer $CHOSPROXY_API_TOKEN" http://127.0.0.1:${status.config.port}/v1/status`;
}

/** Settings → Automation API (M28): a token-protected API on 127.0.0.1. */
export function AutomationSection() {
  const [status, setStatus] = useState<ApiStatus | null>(null);
  const [port, setPort] = useState(8790);
  const [error, setError] = useState<string | null>(null);
  const [showToken, setShowToken] = useState(false);
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    api.apiStatus().then(
      (s) => {
        if (!alive) return;
        setStatus(s);
        setPort(s.config.port);
      },
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
  }, []);

  async function save(enabled: boolean) {
    try {
      const s = await api.saveApiSettings({ enabled, port });
      setStatus(s);
      if (s.error) notify("error", s.error);
      else notify("info", enabled ? `Automation API on ${s.address ?? ""}` : "Automation API off");
    } catch (e) {
      notify("error", `Not saved: ${toIpcError(e).message}`);
    }
  }

  async function regenerate() {
    const ok = await dialogs.confirm(
      "Scripts using the current token will stop working until they use the new one.",
      "Regenerate the API token?",
      "Regenerate",
    );
    if (!ok) return;
    try {
      setStatus(await api.regenerateApiToken());
      notify("info", "New API token created.");
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }

  if (error) return <ErrorState title="Could not read the API settings" message={error} />;
  if (!status) return <LoadingState />;
  return (
    <section className="card" aria-label="Automation API">
      <h2>
        Automation API{" "}
        <Badge tone={status.running ? "ok" : "neutral"}>
          {status.running ? `on ${status.address ?? ""}` : "off"}
        </Badge>
      </h2>
      <p className="muted rule-hint">
        A JSON API for scripts and the <code>chosproxy-cli</code> command, on 127.0.0.1 only. Every
        call needs the token below; requests from web pages are refused. It can do what this window
        can do (scope, crawls, scans, sends, reports) with the same limits. See docs/API.md.
      </p>
      <div className="dialog-row">
        <Switch
          checked={status.config.enabled}
          onChange={(enabled) => void save(enabled)}
          label="Listen on 127.0.0.1"
        />
        <label className="inline-field">
          Port
          <input
            type="number"
            min={1024}
            max={65535}
            value={port}
            onChange={(e) => setPort(Number(e.target.value))}
          />
        </label>
        {port !== status.config.port && (
          <Button size="sm" onClick={() => void save(status.config.enabled)}>
            Apply port
          </Button>
        )}
      </div>
      {status.error && <p className="callout callout-danger">{status.error}</p>}
      {status.token && (
        <div className="dialog-row">
          <span className="mono selectable">
            {showToken ? status.token : `${status.token.slice(0, 6)}…`}
          </span>
          <Button size="sm" variant="ghost" onClick={() => setShowToken((v) => !v)}>
            {showToken ? "Hide" : "Show"}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon="copy"
            onClick={() => {
              void copyText(status.token);
              notify("info", "API token copied");
            }}
          >
            Copy token
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void regenerate()}>
            Regenerate…
          </Button>
        </div>
      )}
      <p className="muted rule-hint">
        Token file: <span className="mono selectable">{status.tokenFile}</span>
        <br />
        Try: <span className="mono selectable">{curlExample(status)}</span>
      </p>
    </section>
  );
}
