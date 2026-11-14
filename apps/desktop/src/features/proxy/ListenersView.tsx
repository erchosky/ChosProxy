import { useMemo, useState } from "react";
import type { ProxyConfig } from "../../bindings/ProxyConfig";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { useAppStore } from "../../state/app";
import { ListenerControls } from "./ListenerControls";
import { fromDraft, type ListenerDraft, newDraft, toDraft, validateDraft } from "./listenerDraft";

export function ListenersView() {
  const proxyConfig = useAppStore((s) => s.proxyConfig);
  if (!proxyConfig) return <LoadingState />;
  // Re-keying resets the drafts whenever the saved configuration changes.
  return <ListenersEditor key={JSON.stringify(proxyConfig)} proxyConfig={proxyConfig} />;
}

function ListenersEditor({ proxyConfig }: { proxyConfig: ProxyConfig }) {
  const saveProxyConfig = useAppStore((s) => s.saveProxyConfig);
  const busy = useAppStore((s) => s.busy);
  const [drafts, setDrafts] = useState<ListenerDraft[]>(() => proxyConfig.listeners.map(toDraft));

  const errors = useMemo(() => drafts.map((d) => validateDraft(d, drafts)), [drafts]);
  const hasErrors = errors.some((e) => Object.keys(e).length > 0);
  const dirty = useMemo(
    () => JSON.stringify(drafts.map(fromDraft)) !== JSON.stringify(proxyConfig.listeners),
    [drafts, proxyConfig],
  );

  function update(i: number, patch: Partial<ListenerDraft>) {
    setDrafts((ds) => ds.map((d, j) => (j === i ? { ...d, ...patch } : d)));
  }

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1>Proxy listeners</h1>
          <p className="page-sub">
            Each listener is an HTTP proxy endpoint. Loopback addresses are only reachable from this
            machine.
          </p>
        </div>
      </div>

      <section className="card">
        <h2>Status</h2>
        <ListenerControls />
      </section>

      <section className="card">
        <h2>Configuration</h2>
        <div className="listener-editor" role="table" aria-label="Listener configuration">
          <div className="listener-editor-row listener-editor-head" role="row">
            <span>Id</span>
            <span>Interface</span>
            <span>Port</span>
            <span>Start with project</span>
            <span>Allow remote connections</span>
            <span title="HTTP/2 to clients, and clients that do not know they use a proxy">
              HTTP/2 · Invisible
            </span>
            <span />
          </div>
          {drafts.map((d, i) => {
            const e = errors[i] ?? {};
            return (
              <div className="listener-editor-row" role="row" key={i}>
                <span>
                  <input
                    type="text"
                    aria-label="Listener id"
                    value={d.id}
                    aria-invalid={!!e.id}
                    onChange={(ev) => update(i, { id: ev.target.value })}
                  />
                  {e.id && <div className="field-error">{e.id}</div>}
                </span>
                <span>
                  <input
                    type="text"
                    aria-label="Bind address"
                    value={d.bindAddress}
                    aria-invalid={!!e.bindAddress}
                    onChange={(ev) => update(i, { bindAddress: ev.target.value })}
                  />
                  {e.bindAddress && <div className="field-error">{e.bindAddress}</div>}
                </span>
                <span>
                  <input
                    type="number"
                    aria-label="Port"
                    min={1}
                    max={65535}
                    value={d.port}
                    aria-invalid={!!e.port}
                    onChange={(ev) => update(i, { port: ev.target.value })}
                  />
                  {e.port && <div className="field-error">{e.port}</div>}
                </span>
                <span>
                  <Switch
                    label=""
                    checked={d.startOnOpen}
                    onChange={(v) => update(i, { startOnOpen: v })}
                  />
                </span>
                <span>
                  <Switch
                    label=""
                    checked={d.allowRemote}
                    onChange={(v) => update(i, { allowRemote: v })}
                  />
                  {d.allowRemote && <Badge tone="warn">exposed to network</Badge>}
                </span>
                <span className="listener-invisible">
                  <Switch
                    label="HTTP/2"
                    checked={d.http2}
                    onChange={(v) => update(i, { http2: v })}
                  />
                  <Switch
                    label="Invisible"
                    checked={d.invisible}
                    onChange={(v) => update(i, { invisible: v })}
                  />
                  {d.invisible && (
                    <label className="inline-field">
                      TLS port
                      <input
                        type="number"
                        aria-label="TLS port for invisible clients"
                        min={1}
                        max={65535}
                        value={d.invisibleTlsPort}
                        aria-invalid={!!e.invisibleTlsPort}
                        onChange={(ev) => update(i, { invisibleTlsPort: ev.target.value })}
                      />
                    </label>
                  )}
                  {d.invisible && (
                    <label className="inline-field">
                      Redirect to
                      <input
                        type="text"
                        placeholder="host:port (optional)"
                        aria-label="Redirect invisible traffic to"
                        value={d.redirect}
                        aria-invalid={!!e.redirect}
                        onChange={(ev) => update(i, { redirect: ev.target.value })}
                      />
                    </label>
                  )}
                  <label className="inline-field">
                    Certificate
                    <select
                      aria-label="Certificate for intercepted HTTPS"
                      value={d.certificate}
                      onChange={(ev) =>
                        update(i, { certificate: ev.target.value as typeof d.certificate })
                      }
                    >
                      <option value="shared">ChosProxy CA</option>
                      <option value="ownCa">its own CA</option>
                      <option value="file">a certificate file</option>
                    </select>
                  </label>
                  {d.certificate === "file" && (
                    <>
                      <input
                        type="text"
                        className="mono"
                        placeholder="certificate.pem (full path)"
                        aria-label="Certificate file"
                        value={d.certFile}
                        aria-invalid={!!e.certFile}
                        onChange={(ev) => update(i, { certFile: ev.target.value })}
                      />
                      <input
                        type="text"
                        className="mono"
                        placeholder="private-key.pem (full path)"
                        aria-label="Private key file"
                        value={d.keyFile}
                        aria-invalid={!!e.keyFile}
                        onChange={(ev) => update(i, { keyFile: ev.target.value })}
                      />
                    </>
                  )}
                  {d.certificate === "ownCa" && (
                    <span className="muted">
                      Clients of this listener trust its CA: download it from http://chosproxy/cert
                      through this listener. The lab browser trusts the ChosProxy CA only.
                    </span>
                  )}
                  {e.certFile && <div className="field-error">{e.certFile}</div>}
                  {e.keyFile && <div className="field-error">{e.keyFile}</div>}
                  {e.invisible && <div className="field-error">{e.invisible}</div>}
                  {e.redirect && <div className="field-error">{e.redirect}</div>}
                  {e.invisibleTlsPort && <div className="field-error">{e.invisibleTlsPort}</div>}
                </span>
                <span>
                  <IconButton
                    icon="trash"
                    label={`Remove ${d.id}`}
                    onClick={() => setDrafts((ds) => ds.filter((_, j) => j !== i))}
                  />
                </span>
              </div>
            );
          })}
        </div>
        {drafts.some((d) => d.http2) && (
          <p className="callout">
            With HTTP/2 on, browsers may use HTTP/2 for intercepted HTTPS. Each request and response
            is read whole before it is passed on (up to 64 MB), shown in the history as{" "}
            <code>HTTP/2</code>, and sent to the server over HTTP/2 when it offers it, otherwise
            over HTTP/1.1.
          </p>
        )}
        {drafts.some((d) => d.invisible) && (
          <p className="callout">
            Invisible listeners also accept clients that are not configured to use a proxy (thick
            clients, devices): plain HTTP goes where its Host header says and TLS where its server
            name says. Point the client here with a hosts-file entry, then give the real address of
            that host name in Settings → Network → Host name overrides, or requests loop back here.
          </p>
        )}
        {drafts.some((d) => d.allowRemote) && (
          <p className="callout callout-danger">
            Allowing remote connections lets other machines on the network use this proxy and see
            the traffic it relays. Require credentials below and only enable this on an isolated lab
            network.
          </p>
        )}
        <div className="form-actions">
          <Button
            icon="plus"
            onClick={() => setDrafts((ds) => [...ds, newDraft(ds)])}
            disabled={drafts.length >= 16}
          >
            Add listener
          </Button>
          <span className="spacer" />
          <Button
            variant="ghost"
            disabled={!dirty}
            onClick={() => setDrafts(proxyConfig.listeners.map(toDraft))}
          >
            Discard changes
          </Button>
          <Button
            variant="primary"
            disabled={!dirty || hasErrors || busy !== null}
            onClick={() =>
              // Keep every other section (e.g. the HTTPS policy) unchanged.
              void saveProxyConfig({ ...proxyConfig, listeners: drafts.map(fromDraft) })
            }
          >
            Save and apply
          </Button>
        </div>
        <p className="muted">
          Running listeners whose settings change are restarted automatically.
        </p>
      </section>

      <section className="card">
        <h2>Authentication</h2>
        <p className="field-help">
          Clients must send these credentials (HTTP Basic, <code>Proxy-Authorization</code>) to use
          the listener. Only a salted hash of the password is stored. Basic credentials travel in
          clear text to the listener: use them on trusted networks only.
        </p>
        {proxyConfig.listeners.map((l) => (
          <ListenerAuthRow key={l.id} id={l.id} username={l.auth?.username ?? null} />
        ))}
      </section>
    </div>
  );
}

function ListenerAuthRow({ id, username }: { id: string; username: string | null }) {
  const setListenerAuth = useAppStore((s) => s.setListenerAuth);
  const busy = useAppStore((s) => s.busy);
  const [editing, setEditing] = useState(false);
  const [user, setUser] = useState(username ?? "");
  const [password, setPassword] = useState("");
  const userError =
    user.trim() === "" ? "Required" : user.includes(":") ? "Cannot contain ':'" : null;

  if (!editing) {
    return (
      <div className="auth-row">
        <span className="mono">{id}</span>
        {username ? (
          <Badge tone="ok">requires user {username}</Badge>
        ) : (
          <Badge>no authentication</Badge>
        )}
        <span className="spacer" />
        <Button size="sm" onClick={() => setEditing(true)}>
          {username ? "Change password" : "Require credentials"}
        </Button>
        {username && (
          <Button
            size="sm"
            variant="ghost"
            disabled={busy !== null}
            onClick={() => void setListenerAuth(id, null)}
          >
            Remove
          </Button>
        )}
      </div>
    );
  }
  return (
    <form
      className="auth-row"
      onSubmit={async (e) => {
        e.preventDefault();
        if (userError || password === "") return;
        const ok = await setListenerAuth(id, { username: user.trim(), password });
        if (ok) {
          setPassword("");
          setEditing(false);
        }
      }}
    >
      <span className="mono">{id}</span>
      <input
        aria-label="User name"
        placeholder="user name"
        autoComplete="off"
        value={user}
        aria-invalid={!!userError}
        onChange={(e) => setUser(e.target.value)}
      />
      <input
        type="password"
        aria-label="Password"
        placeholder="password"
        autoComplete="new-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <Button
        type="submit"
        size="sm"
        variant="primary"
        disabled={busy !== null || !!userError || password === ""}
      >
        Save
      </Button>
      <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
        Cancel
      </Button>
    </form>
  );
}
