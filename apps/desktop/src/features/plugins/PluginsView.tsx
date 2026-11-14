import { useEffect, useState } from "react";
import type { PluginInfo } from "../../bindings/PluginInfo";
import type { PluginPermission } from "../../bindings/PluginPermission";
import type { PluginStatus } from "../../bindings/PluginStatus";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { formatClock } from "../../lib/format";
import { copyText } from "../../services/clipboard";
import { useAppStore } from "../../state/app";
import { usePluginsStore } from "../../state/plugins";

const STATUS: Record<
  PluginStatus,
  { tone: "ok" | "warn" | "danger" | "neutral" | "accent"; label: string }
> = {
  disabled: { tone: "neutral", label: "disabled" },
  stopped: { tone: "neutral", label: "enabled · starts with a project" },
  starting: { tone: "accent", label: "starting" },
  running: { tone: "ok", label: "running" },
  crashed: { tone: "danger", label: "stopped with an error" },
  needsApproval: { tone: "warn", label: "changed · approve again" },
  invalid: { tone: "danger", label: "cannot be used" },
};

/** Plugins: install, review permissions, enable, read their logs. */
export function PluginsView() {
  const plugins = usePluginsStore((s) => s.plugins);
  const dir = usePluginsStore((s) => s.dir);
  const loaded = usePluginsStore((s) => s.loaded);
  const error = usePluginsStore((s) => s.error);
  const { load, installExample, installFrom, setRequireSigned, setTrusted } =
    usePluginsStore.getState();
  const trust = usePluginsStore((s) => s.trust);
  useEffect(() => {
    void load();
  }, [load]);

  if (!loaded) return <LoadingState />;
  if (error)
    return (
      <ErrorState title="Could not load plugins" message={error} onRetry={() => void load()} />
    );
  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1>Plugins</h1>
          <p className="page-sub">
            Plugins add checks to ChosProxy. Each one only gets the permissions you grant here.
          </p>
        </div>
        <div className="dialog-row">
          <Button variant="ghost" icon="restart" onClick={() => void load()}>
            Reload
          </Button>
          <Button icon="folder" onClick={() => void installFrom()}>
            Install from folder…
          </Button>
          {!plugins.some((p) => p.id === "hello-plugin") && (
            <Button icon="plus" onClick={() => void installExample()}>
              Install the HelloPlugin example
            </Button>
          )}
        </div>
      </div>

      <section className="card plugin-warning" role="note">
        <strong>A plugin is a program that runs on this computer with your user’s rights.</strong>
        <p>
          The permissions below decide what it can do <em>through ChosProxy</em> (receive traffic,
          add findings, annotate the history); ChosProxy checks every message it sends. They do not
          stop the program from reading your files or using the network on its own, so install only
          plugins you trust. A plugin whose files change must be approved again.
        </p>
        <div className="dialog-row">
          <span className="muted">Plugins folder:</span>
          <code className="mono selectable">{dir}</code>
          <Button
            size="sm"
            variant="ghost"
            icon="copy"
            onClick={() =>
              void copyText(dir).then((ok) =>
                useAppStore
                  .getState()
                  .notify(ok ? "info" : "error", ok ? "Path copied" : "Could not copy"),
              )
            }
          >
            Copy
          </Button>
        </div>
        <p className="muted">
          Each plugin is a folder named after its id with a <code>plugin.toml</code> manifest. See
          PLUGIN_SDK.md to write one.
        </p>
      </section>

      <section className="card" aria-label="Signatures">
        <h2>Signatures</h2>
        <p className="muted">
          Publishers can sign plugins (<code>chosplugin sign</code>); a signature proves who made
          the files and that nothing changed since. It never grants permissions by itself.
        </p>
        <Switch
          label="Only run plugins signed by a trusted publisher"
          checked={trust?.require_signed ?? false}
          onChange={(v) => void setRequireSigned(v)}
        />
        {(trust?.trusted.length ?? 0) > 0 && (
          <ul className="trusted-list">
            {trust?.trusted.map((t) => (
              <li key={t.key}>
                <strong>{t.name}</strong> <code className="mono">{t.key}</code>{" "}
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void setTrusted(t.key, t.name, false)}
                >
                  Stop trusting
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {plugins.length === 0 ? (
        <EmptyState title="No plugins installed">
          Copy a plugin folder into the plugins folder, or install the HelloPlugin example: it flags
          responses with an X-Debug-Token header.
        </EmptyState>
      ) : (
        plugins.map((p) => (
          // Re-keyed on what is saved so the checkboxes reset after a change.
          <PluginCard key={`${p.id}:${p.enabled}:${p.granted.join(",")}`} plugin={p} />
        ))
      )}
    </div>
  );
}

function SignatureLine({ plugin: p }: { plugin: PluginInfo }) {
  const setTrusted = usePluginsStore.getState().setTrusted;
  const s = p.signature;
  const tone =
    s.state === "trusted"
      ? "ok"
      : s.state === "invalid"
        ? "danger"
        : s.state === "untrusted"
          ? "warn"
          : "neutral";
  const label =
    s.state === "trusted"
      ? `Signed by ${s.publisher} (trusted)`
      : s.state === "untrusted"
        ? `Signed by “${s.publisher}”, key not trusted`
        : s.state === "invalid"
          ? "Signature does not match the files"
          : "Not signed";
  return (
    <div className="dialog-row">
      <Badge tone={tone} title={s.problem ?? s.fingerprint ?? undefined}>
        {label}
      </Badge>
      {s.fingerprint && <span className="muted mono">key {s.fingerprint}</span>}
      {s.state === "untrusted" && s.key && (
        <Button
          size="sm"
          variant="ghost"
          title="Compare the fingerprint with the one the publisher gave you first"
          onClick={() => void setTrusted(s.key ?? "", s.publisher ?? "Publisher", true)}
        >
          Trust this publisher
        </Button>
      )}
    </div>
  );
}

function PluginCard({ plugin: p }: { plugin: PluginInfo }) {
  const { set, showLogs } = usePluginsStore.getState();
  const logsFor = usePluginsStore((s) => s.logsFor);
  const [granted, setGranted] = useState<PluginPermission[]>(p.granted);
  const changed =
    granted.length !== p.granted.length || granted.some((g) => !p.granted.includes(g));
  const status = STATUS[p.status];
  const toggle = (perm: PluginPermission, on: boolean) =>
    setGranted((g) => (on ? [...g, perm] : g.filter((x) => x !== perm)));

  return (
    <section className="card plugin-card" aria-label={p.name}>
      <header className="plugin-head">
        <div>
          <h2>
            {p.name} <span className="muted">{p.version}</span>
          </h2>
          {p.author && <span className="muted">by {p.author}</span>}
        </div>
        <Badge tone={status.tone} title={p.message ?? undefined}>
          {status.label}
        </Badge>
      </header>
      {p.description && <p>{p.description}</p>}
      <SignatureLine plugin={p} />
      {p.message && (
        <p className={p.status === "needsApproval" ? "muted" : "field-error"}>{p.message}</p>
      )}
      {p.command.length > 0 && (
        <p className="muted">
          Runs <code className="mono">{p.command.join(" ")}</code> in{" "}
          <code className="mono selectable">{p.dir}</code>
        </p>
      )}
      {p.requested.length > 0 && (
        <fieldset className="plugin-permissions">
          <legend>Permissions it asks for (none is granted until you tick it)</legend>
          {p.requested.map((r) => (
            <label key={r.permission} className="plugin-permission">
              <input
                type="checkbox"
                checked={granted.includes(r.permission)}
                onChange={(e) => toggle(r.permission, e.target.checked)}
              />
              <span>
                <code className="mono">{r.permission}</code>
                <span className="muted"> — {r.description}</span>
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {p.status !== "invalid" && (
        <div className="dialog-row">
          {!p.enabled ? (
            <Button variant="primary" icon="play" onClick={() => void set(p.id, true, granted)}>
              Enable with these permissions
            </Button>
          ) : p.status === "needsApproval" ? (
            <Button variant="primary" onClick={() => void set(p.id, true, granted)}>
              Approve the new files and enable
            </Button>
          ) : (
            <Button
              disabled={!changed}
              onClick={() => void set(p.id, true, granted)}
              title="Restarts the plugin with the new permissions"
            >
              Apply permissions
            </Button>
          )}
          {p.enabled && (
            <Button variant="ghost" icon="stop" onClick={() => void set(p.id, false, granted)}>
              Disable
            </Button>
          )}
          <span className="spacer" />
          <Button
            size="sm"
            variant="ghost"
            icon="log"
            aria-expanded={logsFor === p.id}
            onClick={() => void showLogs(logsFor === p.id ? null : p.id)}
          >
            {logsFor === p.id ? "Hide log" : "Log"}
          </Button>
        </div>
      )}
      {logsFor === p.id && <PluginLog />}
    </section>
  );
}

function PluginLog() {
  const logs = usePluginsStore((s) => s.logs);
  if (logs.length === 0) return <p className="muted">Nothing logged in this session.</p>;
  return (
    <ol className="task-log plugin-log" aria-label="Plugin log">
      {logs.map((l, i) => (
        <li key={i} className={`task-log-${l.level}`}>
          <span className="muted mono">{formatClock(l.at)}</span>
          <span className="selectable">{l.message}</span>
        </li>
      ))}
    </ol>
  );
}
