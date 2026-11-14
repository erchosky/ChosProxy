import { useEffect, useState } from "react";
import type { AuthScheme } from "../../bindings/AuthScheme";
import type { CredentialInfo } from "../../bindings/CredentialInfo";
import type { HostOverride } from "../../bindings/HostOverride";
import type { PlatformAuthRule } from "../../bindings/PlatformAuthRule";
import type { NetworkConfig } from "../../bindings/NetworkConfig";
import type { UpstreamKind } from "../../bindings/UpstreamKind";
import type { UpstreamRule } from "../../bindings/UpstreamRule";
import { Button, IconButton } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import {
  UPSTREAM_KINDS,
  isHostPattern,
  moveItem,
  newUpstreamRule,
  overrideProblem,
  upstreamProblem,
} from "../../lib/network";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

/** Settings → Network: upstream proxies, host name overrides, timeouts. */
export function NetworkSection() {
  const [saved, setSaved] = useState<NetworkConfig | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let alive = true;
    api.networkConfig().then(
      (c) => alive && setSaved(c),
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
  }, [attempt]);

  if (error) {
    return (
      <ErrorState
        message={error}
        onRetry={() => {
          setError(null);
          setAttempt((n) => n + 1);
        }}
      />
    );
  }
  if (!saved) return <LoadingState />;
  return <NetworkForm key={JSON.stringify(saved)} saved={saved} onSaved={setSaved} />;
}

function NetworkForm({
  saved,
  onSaved,
}: {
  saved: NetworkConfig;
  onSaved: (c: NetworkConfig) => void;
}) {
  const notify = useAppStore((s) => s.notify);
  const [draft, setDraft] = useState<NetworkConfig>(saved);
  const [saving, setSaving] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const authProblems = draft.auth.filter((a) => !isHostPattern(a.host) || a.credential === "");
  const problems = [
    ...authProblems.map(() => "auth"),
    ...draft.upstream.map(upstreamProblem),
    ...draft.hosts.map(overrideProblem),
  ].filter((p): p is string => p !== null);

  const setRule = (i: number, patch: Partial<UpstreamRule>) =>
    setDraft((d) => ({
      ...d,
      upstream: d.upstream.map((r, j) => (j === i ? { ...r, ...patch } : r)),
    }));
  const [creds, setCreds] = useState<CredentialInfo[]>([]);
  useEffect(() => {
    let alive = true;
    api.credentials().then(
      (c) => alive && setCreds(c),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);
  const setAuth = (i: number, patch: Partial<PlatformAuthRule>) =>
    setDraft((d) => ({ ...d, auth: d.auth.map((a, j) => (j === i ? { ...a, ...patch } : a)) }));
  const setHost = (i: number, patch: Partial<HostOverride>) =>
    setDraft((d) => ({ ...d, hosts: d.hosts.map((h, j) => (j === i ? { ...h, ...patch } : h)) }));

  async function save() {
    setSaving(true);
    setServerError(null);
    try {
      const next = await api.saveNetworkConfig(draft);
      notify("info", "Network settings saved; new connections use them.");
      onSaved(next);
    } catch (e) {
      setServerError(toIpcError(e).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="card network-settings" aria-label="Network">
      <h2>Network</h2>
      <p className="muted rule-hint">
        Every outgoing connection follows these settings: the proxy, Repeater, the Fuzzer, the
        crawler and the other tools.
      </p>

      <h3>Upstream proxies</h3>
      <p className="muted rule-hint">
        The first enabled rule whose host matches is used; no match means a direct connection.
        Credentials are stored in <code>network.toml</code>, readable only by your user, and are
        never logged or exported.
      </p>
      {draft.upstream.length === 0 ? (
        <p className="muted">No rules: every connection is direct.</p>
      ) : (
        <ol className="rule-list">
          {draft.upstream.map((r, i) => {
            const problem = upstreamProblem(r);
            return (
              <li key={i} className="rule-row">
                <div className="rule-fields">
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={r.enabled}
                      onChange={(e) => setRule(i, { enabled: e.target.checked })}
                    />
                    On
                  </label>
                  <label>
                    Hosts
                    <input
                      type="text"
                      value={r.host}
                      aria-label={`Hosts of rule ${i + 1}`}
                      onChange={(e) => setRule(i, { host: e.target.value.trim() })}
                    />
                  </label>
                  <label>
                    Through
                    <select
                      value={r.kind}
                      aria-label={`Connection of rule ${i + 1}`}
                      onChange={(e) => setRule(i, { kind: e.target.value as UpstreamKind })}
                    >
                      {UPSTREAM_KINDS.map((k) => (
                        <option key={k.id} value={k.id}>
                          {k.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  {r.kind !== "direct" && (
                    <>
                      <label>
                        Proxy host
                        <input
                          type="text"
                          value={r.proxyHost}
                          aria-label={`Proxy host of rule ${i + 1}`}
                          onChange={(e) => setRule(i, { proxyHost: e.target.value.trim() })}
                        />
                      </label>
                      <label>
                        Port
                        <input
                          type="number"
                          min={1}
                          max={65535}
                          value={r.proxyPort || ""}
                          aria-label={`Proxy port of rule ${i + 1}`}
                          onChange={(e) => setRule(i, { proxyPort: Number(e.target.value) })}
                        />
                      </label>
                      <label>
                        User
                        <input
                          type="text"
                          autoComplete="off"
                          value={r.username}
                          aria-label={`Proxy user of rule ${i + 1}`}
                          onChange={(e) => setRule(i, { username: e.target.value })}
                        />
                      </label>
                      <label>
                        Password
                        <input
                          type="password"
                          autoComplete="new-password"
                          value={r.password}
                          aria-label={`Proxy password of rule ${i + 1}`}
                          onChange={(e) => setRule(i, { password: e.target.value })}
                        />
                      </label>
                      {r.kind === "socks5" && (
                        <label
                          className="check"
                          title="Let the proxy resolve host names (otherwise they are resolved here)"
                        >
                          <input
                            type="checkbox"
                            checked={r.remoteDns}
                            onChange={(e) => setRule(i, { remoteDns: e.target.checked })}
                          />
                          DNS on the proxy
                        </label>
                      )}
                    </>
                  )}
                </div>
                <div className="rule-actions">
                  <IconButton
                    icon="arrowUp"
                    label="Move up"
                    disabled={i === 0}
                    onClick={() =>
                      setDraft((d) => ({ ...d, upstream: moveItem(d.upstream, i, -1) }))
                    }
                  />
                  <IconButton
                    icon="arrowDown"
                    label="Move down"
                    disabled={i === draft.upstream.length - 1}
                    onClick={() =>
                      setDraft((d) => ({ ...d, upstream: moveItem(d.upstream, i, 1) }))
                    }
                  />
                  <IconButton
                    icon="trash"
                    label={`Remove rule ${i + 1}`}
                    onClick={() =>
                      setDraft((d) => ({ ...d, upstream: d.upstream.filter((_, j) => j !== i) }))
                    }
                  />
                </div>
                {problem && <div className="field-error">{problem}</div>}
              </li>
            );
          })}
        </ol>
      )}
      <div className="form-actions">
        <Button
          size="sm"
          icon="plus"
          onClick={() => setDraft((d) => ({ ...d, upstream: [...d.upstream, newUpstreamRule()] }))}
        >
          Add upstream rule
        </Button>
      </div>

      <h3>Platform authentication</h3>
      <p className="muted rule-hint">
        When a matching host answers 401 with a Basic or Digest challenge, Repeater, the Fuzzer, the
        crawler and plugins send the request again once with a credential from Settings →
        Credentials. The browser handles its own login prompts.
      </p>
      {draft.auth.length > 0 && (
        <ul className="rule-list">
          {draft.auth.map((a, i) => (
            <li key={i} className="rule-row">
              <div className="rule-fields">
                <label className="check">
                  <input
                    type="checkbox"
                    checked={a.enabled}
                    onChange={(e) => setAuth(i, { enabled: e.target.checked })}
                  />
                  On
                </label>
                <label>
                  Host
                  <input
                    type="text"
                    value={a.host}
                    aria-label={`Host of authentication rule ${i + 1}`}
                    onChange={(e) => setAuth(i, { host: e.target.value.trim() })}
                  />
                </label>
                <label>
                  Scheme
                  <select
                    value={a.scheme}
                    aria-label={`Scheme of authentication rule ${i + 1}`}
                    onChange={(e) => setAuth(i, { scheme: e.target.value as AuthScheme })}
                  >
                    <option value="basic">Basic</option>
                    <option value="digest">Digest</option>
                    <option value="ntlm">NTLMv2</option>
                  </select>
                </label>
                <label>
                  Credential
                  <select
                    value={a.credential}
                    aria-label={`Credential of authentication rule ${i + 1}`}
                    onChange={(e) => setAuth(i, { credential: e.target.value })}
                  >
                    <option value="">Choose…</option>
                    {creds.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.label}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <div className="rule-actions">
                <IconButton
                  icon="trash"
                  label={`Remove authentication rule ${i + 1}`}
                  onClick={() =>
                    setDraft((d) => ({ ...d, auth: d.auth.filter((_, j) => j !== i) }))
                  }
                />
              </div>
              {(!isHostPattern(a.host) || a.credential === "") && (
                <div className="field-error">Enter a host and choose a credential</div>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className="form-actions">
        <Button
          size="sm"
          icon="plus"
          disabled={creds.length === 0}
          title={
            creds.length === 0 ? "Add a credential in Settings → Credentials first" : undefined
          }
          onClick={() =>
            setDraft((d) => ({
              ...d,
              auth: [
                ...d.auth,
                { enabled: true, host: "", scheme: "basic", credential: creds[0]?.id ?? "" },
              ],
            }))
          }
        >
          Add authentication rule
        </Button>
      </div>

      <h3>Host name overrides</h3>
      <p className="muted rule-hint">
        Resolve these names to fixed addresses instead of using DNS (also through upstream proxies).
        Needed for invisible listeners whose clients reach the proxy through a hosts-file entry.
      </p>
      {draft.hosts.length > 0 && (
        <ul className="rule-list">
          {draft.hosts.map((h, i) => {
            const problem = overrideProblem(h);
            return (
              <li key={i} className="rule-row">
                <div className="rule-fields">
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={h.enabled}
                      onChange={(e) => setHost(i, { enabled: e.target.checked })}
                    />
                    On
                  </label>
                  <label>
                    Host
                    <input
                      type="text"
                      value={h.host}
                      aria-label={`Host of override ${i + 1}`}
                      onChange={(e) => setHost(i, { host: e.target.value.trim() })}
                    />
                  </label>
                  <label>
                    Address
                    <input
                      type="text"
                      value={h.address}
                      aria-label={`Address of override ${i + 1}`}
                      onChange={(e) => setHost(i, { address: e.target.value.trim() })}
                    />
                  </label>
                </div>
                <div className="rule-actions">
                  <IconButton
                    icon="trash"
                    label={`Remove override ${i + 1}`}
                    onClick={() =>
                      setDraft((d) => ({ ...d, hosts: d.hosts.filter((_, j) => j !== i) }))
                    }
                  />
                </div>
                {problem && <div className="field-error">{problem}</div>}
              </li>
            );
          })}
        </ul>
      )}
      <div className="form-actions">
        <Button
          size="sm"
          icon="plus"
          onClick={() =>
            setDraft((d) => ({
              ...d,
              hosts: [...d.hosts, { enabled: true, host: "", address: "127.0.0.1" }],
            }))
          }
        >
          Add host override
        </Button>
      </div>

      <h3>Timeouts</h3>
      <div className="timeouts">
        {(
          [
            ["connectSecs", "Connect (DNS, TCP, proxy)"],
            ["responseSecs", "Waiting for the response"],
            ["idleSecs", "Silence while a body transfers"],
          ] as const
        ).map(([key, label]) => (
          <label key={key}>
            {label}
            <span className="inline-field">
              <input
                type="number"
                min={1}
                max={3600}
                value={draft.timeouts[key]}
                onChange={(e) =>
                  setDraft((d) => ({
                    ...d,
                    timeouts: { ...d.timeouts, [key]: Number(e.target.value) },
                  }))
                }
              />
              s
            </span>
          </label>
        ))}
      </div>
      <p className="muted rule-hint">
        The Fuzzer, Sequencer, crawler and discovery have their own per-task timeout.
      </p>

      {serverError && <p className="callout callout-danger">{serverError}</p>}
      <div className="form-actions">
        <span className="spacer" />
        <Button variant="ghost" disabled={!dirty || saving} onClick={() => setDraft(saved)}>
          Revert
        </Button>
        <Button
          variant="primary"
          disabled={!dirty || saving || problems.length > 0}
          onClick={() => void save()}
        >
          Save network settings
        </Button>
      </div>
    </section>
  );
}
