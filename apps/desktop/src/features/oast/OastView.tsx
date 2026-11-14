import { useEffect, useState } from "react";
import type { OastConfig } from "../../bindings/OastConfig";
import type { OastMode } from "../../bindings/OastMode";
import type { OastPayloadRow } from "../../bindings/OastPayloadRow";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { CodeView } from "../../components/message/CodeView";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { formatClock, formatDateTime } from "../../lib/format";
import { copyText } from "../../services/clipboard";
import { useAppStore } from "../../state/app";
import { openContextMenu } from "../../state/contextMenu";
import { useOastStore } from "../../state/oast";
import { exchangeMenu, sendToRepeater } from "../../state/tools";

const MODES: { id: OastMode; label: string; help: string }[] = [
  { id: "off", label: "Off", help: "No OAST server; nothing listens and nothing is polled." },
  {
    id: "local",
    label: "Local",
    help: "A server inside ChosProxy, for labs that can reach this machine.",
  },
  {
    id: "selfHosted",
    label: "Self-hosted",
    help: "Your own chosoast server with a DNS zone delegated to it (see docs/OAST.md).",
  },
];

/** ChosOAST: out-of-band interaction testing. */
export function OastView() {
  const loaded = useOastStore((s) => s.loaded);
  const error = useOastStore((s) => s.error);
  const config = useOastStore((s) => s.config);
  const status = useOastStore((s) => s.status);
  const [showSettings, setShowSettings] = useState(false);
  useEffect(() => {
    void useOastStore.getState().load();
  }, []);

  if (!loaded) return <LoadingState />;
  if (error || !config || !status)
    return (
      <ErrorState
        title="Could not load OAST"
        message={error ?? "unknown error"}
        onRetry={() => void useOastStore.getState().load()}
      />
    );
  const off = config.mode === "off";
  return (
    <div className="page oast-page">
      <div className="page-header">
        <div>
          <h1>OAST</h1>
          <p className="page-sub">
            Unique addresses to put in requests: if the target later looks one up or fetches it, the
            interaction appears here, linked to the request that carried it.
          </p>
        </div>
        <div className="dialog-row">
          <Badge
            tone={off ? "neutral" : status.error ? "danger" : status.running ? "ok" : "warn"}
            title={status.error ?? undefined}
          >
            {off
              ? "off"
              : status.error
                ? "not working"
                : `${MODES.find((m) => m.id === config.mode)?.label ?? ""} · receiving`}
          </Badge>
          <Button
            variant="ghost"
            icon="settings"
            aria-expanded={showSettings || off}
            onClick={() => setShowSettings((v) => !v)}
          >
            Settings
          </Button>
        </div>
      </div>
      {status.error && <p className="callout callout-warn">{status.error}</p>}
      {(showSettings || off) && (
        <OastSettings
          key={JSON.stringify(config)}
          initial={config}
          onSaved={() => setShowSettings(false)}
        />
      )}
      {!off && <OastWorkspace />}
    </div>
  );
}

function OastSettings({ initial, onSaved }: { initial: OastConfig; onSaved(): void }) {
  const [c, setC] = useState(initial);
  const saving = useOastStore((s) => s.saving);
  const { saveConfig, testServer } = useOastStore.getState();
  const local = c.local;
  const remote = c.selfHosted;
  const setLocal = (patch: Partial<OastConfig["local"]>) =>
    setC({ ...c, local: { ...local, ...patch } });
  const setRemote = (patch: Partial<OastConfig["selfHosted"]>) =>
    setC({ ...c, selfHosted: { ...remote, ...patch } });
  const exposed = c.mode === "local" && local.bind !== "127.0.0.1" && local.bind !== "::1";

  return (
    <section className="card oast-settings" aria-label="OAST settings">
      <p className="callout">
        Out-of-band testing makes the target contact a server you control. Use it only on systems
        you are authorized to test. Interactions are stored in the project; nothing is sent to
        ChosProxy&apos;s authors.
      </p>
      <fieldset className="oast-modes">
        <legend>Mode</legend>
        {MODES.map((m) => (
          <label key={m.id} className="oast-mode">
            <input
              type="radio"
              name="oast-mode"
              checked={c.mode === m.id}
              onChange={() => setC({ ...c, mode: m.id })}
            />
            <span>
              <strong>{m.label}</strong> <span className="muted">— {m.help}</span>
            </span>
          </label>
        ))}
      </fieldset>

      {c.mode === "local" && (
        <div className="oast-fields">
          <label>
            Listen on
            <input
              className="mono"
              value={local.bind}
              onChange={(e) => setLocal({ bind: e.target.value.trim() })}
            />
          </label>
          <label>
            HTTP port
            <input
              type="number"
              min={1}
              max={65535}
              value={local.httpPort}
              onChange={(e) => setLocal({ httpPort: Number(e.target.value) })}
            />
          </label>
          <label title="Host or IP placed in payload URLs: how the target reaches this machine">
            Address in payloads
            <input
              className="mono"
              value={local.advertisedHost}
              onChange={(e) => setLocal({ advertisedHost: e.target.value.trim() })}
            />
          </label>
          <Switch
            label="Answer DNS"
            checked={local.dnsEnabled}
            onChange={(dnsEnabled) => setLocal({ dnsEnabled })}
          />
          {local.dnsEnabled && (
            <>
              <label>
                DNS port (UDP)
                <input
                  type="number"
                  min={1}
                  max={65535}
                  value={local.dnsPort}
                  onChange={(e) => setLocal({ dnsPort: Number(e.target.value) })}
                />
              </label>
              <label>
                DNS zone
                <input
                  className="mono"
                  value={local.zone}
                  onChange={(e) => setLocal({ zone: e.target.value.trim() })}
                />
              </label>
            </>
          )}
          <Switch
            label="Accept mail (SMTP)"
            checked={local.smtpEnabled}
            onChange={(smtpEnabled) => setLocal({ smtpEnabled })}
          />
          {local.smtpEnabled && (
            <>
              <label>
                SMTP port
                <input
                  type="number"
                  min={1}
                  max={65535}
                  value={local.smtpPort}
                  onChange={(e) => setLocal({ smtpPort: Number(e.target.value) })}
                />
              </label>
              <p className="muted">
                Mail sent to this port is recorded as an interaction when it carries a payload
                identifier (for example <span className="mono">&lt;id&gt;@{local.zone}</span>). It
                is never delivered or relayed.
              </p>
            </>
          )}
          {exposed && (
            <p className="callout callout-warn">
              Listening on {local.bind} makes the OAST server reachable from the network. Use a lab
              network you control.
            </p>
          )}
          {local.dnsEnabled && (
            <p className="muted">
              DNS payloads only work when the target resolves {local.zone} through this server (for
              example a lab DNS forwarding that zone here).
            </p>
          )}
        </div>
      )}

      {c.mode === "selfHosted" && (
        <div className="oast-fields">
          <label>
            Zone
            <input
              className="mono"
              placeholder="oast.example.com"
              value={remote.zone}
              onChange={(e) => setRemote({ zone: e.target.value.trim() })}
            />
          </label>
          <label>
            API host
            <input
              className="mono"
              placeholder="203.0.113.5"
              value={remote.apiHost}
              onChange={(e) => setRemote({ apiHost: e.target.value.trim() })}
            />
          </label>
          <label>
            API port
            <input
              type="number"
              min={1}
              max={65535}
              value={remote.apiPort}
              onChange={(e) => setRemote({ apiPort: Number(e.target.value) })}
            />
          </label>
          <label>
            Access token
            <input
              type="password"
              autoComplete="off"
              className="mono"
              value={remote.token}
              onChange={(e) => setRemote({ token: e.target.value.trim() })}
            />
          </label>
          <label className="oast-wide">
            Certificate SHA-256
            <input
              className="mono"
              placeholder="aa:bb:…"
              value={remote.fingerprint}
              onChange={(e) => setRemote({ fingerprint: e.target.value.trim() })}
            />
          </label>
          <label>
            Poll every (seconds)
            <input
              type="number"
              min={1}
              max={3600}
              value={c.pollSeconds}
              onChange={(e) => setC({ ...c, pollSeconds: Number(e.target.value) })}
            />
          </label>
        </div>
      )}

      <div className="dialog-row">
        <Button
          variant="primary"
          disabled={saving}
          onClick={() => void saveConfig(c).then((ok) => ok && onSaved())}
        >
          Save
        </Button>
        {c.mode === "selfHosted" && (
          <Button variant="ghost" onClick={() => void testServer(c)}>
            Test connection
          </Button>
        )}
      </div>
    </section>
  );
}

function OastWorkspace() {
  return (
    <div className="oast-workspace">
      <SplitPane
        direction="horizontal"
        initial={0.36}
        first={<PayloadList />}
        second={
          <SplitPane
            direction="vertical"
            initial={0.45}
            first={<InteractionList />}
            second={<InteractionDetail />}
          />
        }
      />
    </div>
  );
}

function copy(text: string) {
  void copyText(text).then((ok) =>
    useAppStore.getState().notify(ok ? "info" : "error", ok ? `Copied ${text}` : "Could not copy"),
  );
}

function PayloadList() {
  const payloads = useOastStore((s) => s.payloads);
  const filterId = useOastStore((s) => s.filterId);
  const status = useOastStore((s) => s.status);
  const { newPayload, setFilter } = useOastStore.getState();
  const [note, setNote] = useState("");
  const canCreate = status?.error === null;
  return (
    <section className="oast-payloads" aria-label="Payloads">
      <form
        className="dialog-row"
        onSubmit={(e) => {
          e.preventDefault();
          void newPayload(note, "url").then((p) => p && setNote(""));
        }}
      >
        <input
          aria-label="Note for the new payload"
          placeholder="Note (where you will use it)"
          maxLength={500}
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
        <Button type="submit" variant="primary" icon="plus" disabled={!canCreate}>
          New payload
        </Button>
      </form>
      <p className="muted oast-hint">
        The new URL is copied; paste it where the target may fetch it.
      </p>
      {payloads.length === 0 ? (
        <EmptyState title="No payloads yet">Create one and put it in a request.</EmptyState>
      ) : (
        <ul className="oast-payload-list">
          {payloads.map((p) => (
            <PayloadItem
              key={p.id}
              p={p}
              active={filterId === p.correlationId}
              onSelect={() => void setFilter(filterId === p.correlationId ? null : p.correlationId)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function PayloadItem({
  p,
  active,
  onSelect,
}: {
  p: OastPayloadRow;
  active: boolean;
  onSelect(): void;
}) {
  return (
    <li className={`oast-payload${active ? " selected" : ""}`}>
      <button type="button" className="oast-payload-head" aria-pressed={active} onClick={onSelect}>
        <span className="mono">{p.correlationId}</span>
        {p.interactions > 0 ? (
          <Badge tone="warn">
            {p.interactions} interaction{p.interactions === 1 ? "" : "s"}
          </Badge>
        ) : (
          <span className="muted">no interactions</span>
        )}
      </button>
      {p.note && <span className="oast-note">{p.note}</span>}
      <span className="oast-payload-values">
        <code className="mono selectable">{p.url}</code>
        <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(p.url)}>
          URL
        </Button>
        {p.dnsName && (
          <>
            <code className="mono selectable">{p.dnsName}</code>
            <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(p.dnsName ?? "")}>
              DNS
            </Button>
          </>
        )}
      </span>
      <span className="muted">
        Created {formatDateTime(p.createdAt)} · carried by {p.requests} stored request
        {p.requests === 1 ? "" : "s"}
      </span>
    </li>
  );
}

function InteractionList() {
  const interactions = useOastStore((s) => s.interactions);
  const filterId = useOastStore((s) => s.filterId);
  const detail = useOastStore((s) => s.detail);
  const { selectInteraction, setFilter } = useOastStore.getState();
  const selected = detail.status === "idle" ? null : detail.id;
  return (
    <section className="oast-interactions" aria-label="Interactions">
      <div className="dialog-row">
        <strong>Interactions</strong>
        {filterId && (
          <span className="chip" aria-label="Filter">
            <span className="mono">{filterId}</span>
            <button
              type="button"
              className="chip-clear"
              aria-label="Show all"
              onClick={() => void setFilter(null)}
            >
              ×
            </button>
          </span>
        )}
      </div>
      {interactions.length === 0 ? (
        <EmptyState title="No interactions yet">
          They appear here as soon as something contacts the server with one of this project&apos;s
          payloads.
        </EmptyState>
      ) : (
        <div className="endpoint-table-wrap">
          <table className="endpoint-table organizer-table">
            <thead>
              <tr>
                <th>Time</th>
                <th>Type</th>
                <th>From</th>
                <th>What</th>
                <th>Payload</th>
              </tr>
            </thead>
            <tbody>
              {interactions.map((i) => (
                <tr
                  key={i.id}
                  className={selected === i.id ? "selected" : undefined}
                  aria-selected={selected === i.id}
                  onClick={() => selectInteraction(i.id)}
                >
                  <td className="num">{formatClock(i.receivedAt)}</td>
                  <td>
                    <Badge
                      tone={i.protocol === "dns" ? "accent" : i.protocol === "smtp" ? "ok" : "warn"}
                    >
                      {i.protocol.toUpperCase()}
                    </Badge>
                  </td>
                  <td className="mono">{i.remote}</td>
                  <td className="mono" title={i.summary}>
                    {i.summary}
                  </td>
                  <td className="muted" title={i.correlationId}>
                    {i.note || i.correlationId}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function InteractionDetail() {
  const detail = useOastStore((s) => s.detail);
  const { selectInteraction } = useOastStore.getState();
  if (detail.status === "idle") return <EmptyState title="Select an interaction" />;
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error")
    return (
      <ErrorState
        title="Could not load the interaction"
        message={detail.message}
        onRetry={() => selectInteraction(detail.id)}
      />
    );
  const d = detail.detail;
  return (
    <div className="oast-detail">
      <div className="exchange-summary">
        <Badge tone={d.row.protocol === "dns" ? "accent" : "warn"}>
          {d.row.protocol.toUpperCase()}
        </Badge>
        <span className="mono selectable">{d.row.remote}</span>
        <span className="muted">{formatDateTime(d.row.receivedAt)}</span>
      </div>
      <div className="oast-detail-body">
        <CodeView
          text={d.text}
          language={d.row.protocol === "http" || d.row.protocol === "https" ? "http" : "text"}
          wrap
          label="Interaction"
        />
      </div>
      <div className="oast-requests">
        <strong>Requests that carried {d.row.correlationId}</strong>
        {d.requests.length === 0 ? (
          <p className="muted">
            None stored yet. Send the payload through the proxy, the Repeater or another tool so
            ChosProxy can link the interaction to its request.
          </p>
        ) : (
          <ul>
            {d.requests.map((r) => (
              <li
                key={r.id}
                onContextMenu={(e) =>
                  openContextMenu(e, "Request actions", exchangeMenu({ id: r.id, url: r.url }))
                }
              >
                <span className="muted">#{r.id}</span> <span className="mono">{r.method}</span>{" "}
                <span className="mono selectable">{r.url}</span>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="repeater"
                  onClick={() => sendToRepeater(r.id)}
                >
                  Repeater
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
