import { useEffect, useState } from "react";
import type { SessionRule } from "../../bindings/SessionRule";
import { Button, IconButton } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { api, toIpcError } from "../../services/ipc";
import { useSessionDraft } from "../../state/ai";
import { useAppStore } from "../../state/app";

const TOOLS = ["repeater", "fuzzer", "sequencer", "crawler", "plugin", "agent"];

const csv = (v: string) =>
  v
    .split(",")
    .map((x) => x.trim())
    .filter(Boolean);

function newRule(n: number): SessionRule {
  return {
    enabled: true,
    name: `Session ${n}`,
    hosts: [],
    tools: [],
    useCookieJar: true,
    expiredStatuses: [401],
    expiredRegex: "",
    macroRequests: [],
    extract: [],
    headers: [],
  };
}

/** Project → Session rules: keep tool requests logged in (M24). */
export function SessionRulesCard() {
  const [saved, setSaved] = useState<SessionRule[] | null>(null);
  const [draft, setDraft] = useState<SessionRule[]>([]);
  const [error, setError] = useState<string | null>(null);
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    let loaded = false;
    // A rule suggested by ChosAI joins the draft, unsaved, for review.
    const added = (rule: SessionRule) =>
      useAppStore
        .getState()
        .notify("info", `“${rule.name}” from ChosAI was added below: review it, then Save.`);
    api.sessionRules().then(
      (r) => {
        if (!alive) return;
        loaded = true;
        const offered = useSessionDraft.getState().take();
        setSaved(r);
        setDraft(offered ? [...r, offered] : r);
        if (offered) added(offered);
      },
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    const unsubscribe = useSessionDraft.subscribe((s) => {
      if (!loaded || !s.rule) return;
      const rule = useSessionDraft.getState().take();
      if (!rule) return;
      setDraft((d) => [...d, rule]);
      added(rule);
    });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, []);

  const set = (i: number, patch: Partial<SessionRule>) =>
    setDraft((d) => d.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  async function save() {
    try {
      await api.saveSessionRules(draft);
      setSaved(draft);
      notify("info", "Session rules saved.");
    } catch (e) {
      notify("error", `Not saved: ${toIpcError(e).message}`);
    }
  }

  if (error) return <ErrorState title="Could not load session rules" message={error} />;
  if (!saved) return <LoadingState />;
  return (
    <section className="card" aria-label="Session rules">
      <h2>Session rules</h2>
      <p className="muted rule-hint">
        Keep Repeater, Fuzzer, Sequencer, crawler, plugin and ChosAgent requests logged in. A rule
        adds the cookie jar and headers; when a response shows the session expired (status or
        regex), it runs its login macro — stored requests from the history, by number — and sends
        again once. Use <code>{"{{name}}"}</code> for extracted values and{" "}
        <code>{"{{cred:id.username}}"}</code> / <code>{"{{cred:id.secret}}"}</code> for keyring
        credentials, in headers and in the macro requests.
      </p>
      <ol className="rule-list">
        {draft.map((r, i) => (
          <li key={i} className="rule-row">
            <div className="rule-fields">
              <label className="check">
                <input
                  type="checkbox"
                  checked={r.enabled}
                  onChange={(e) => set(i, { enabled: e.target.checked })}
                />
                On
              </label>
              <label>
                Name
                <input
                  type="text"
                  value={r.name}
                  onChange={(e) => set(i, { name: e.target.value })}
                />
              </label>
              <label className="grow">
                Hosts (comma-separated, empty = all)
                <input
                  type="text"
                  value={r.hosts.join(", ")}
                  onChange={(e) => set(i, { hosts: csv(e.target.value) })}
                />
              </label>
              <label className="grow">
                Tools (empty = all: {TOOLS.join(", ")})
                <input
                  type="text"
                  value={r.tools.join(", ")}
                  onChange={(e) => set(i, { tools: csv(e.target.value) })}
                />
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={r.useCookieJar}
                  onChange={(e) => set(i, { useCookieJar: e.target.checked })}
                />
                Cookie jar
              </label>
            </div>
            <div className="rule-actions">
              <IconButton
                icon="trash"
                label={`Remove ${r.name}`}
                onClick={() => setDraft((d) => d.filter((_, j) => j !== i))}
              />
            </div>
            <div className="rule-fields rule-summary">
              <label>
                Expired when status is
                <input
                  type="text"
                  value={r.expiredStatuses.join(", ")}
                  onChange={(e) =>
                    set(i, {
                      expiredStatuses: csv(e.target.value)
                        .map(Number)
                        .filter((n) => Number.isInteger(n) && n > 99 && n < 600),
                    })
                  }
                />
              </label>
              <label className="grow">
                …or the response matches (regex)
                <input
                  type="text"
                  spellCheck={false}
                  value={r.expiredRegex}
                  onChange={(e) => set(i, { expiredRegex: e.target.value })}
                />
              </label>
              <label>
                Login macro (history #, in order)
                <input
                  type="text"
                  value={r.macroRequests.join(", ")}
                  onChange={(e) =>
                    set(i, {
                      macroRequests: csv(e.target.value)
                        .map(Number)
                        .filter((n) => Number.isInteger(n) && n > 0),
                    })
                  }
                />
              </label>
            </div>
            <div className="rule-fields rule-summary">
              <label className="grow">
                Extract (one per line: name = regex, group 1)
                <textarea
                  rows={2}
                  spellCheck={false}
                  value={r.extract.map((x) => `${x.name} = ${x.regex}`).join("\n")}
                  onChange={(e) =>
                    set(i, {
                      extract: e.target.value
                        .split("\n")
                        .map((l) => l.split(/ = (.*)/))
                        .filter((p) => (p[0] ?? "").trim() && p[1])
                        .map((p) => ({ name: (p[0] ?? "").trim(), regex: p[1] ?? "" })),
                    })
                  }
                />
              </label>
              <label className="grow">
                Headers (one per line: Name: value)
                <textarea
                  rows={2}
                  spellCheck={false}
                  value={r.headers.map((h) => `${h.name}: ${h.value}`).join("\n")}
                  onChange={(e) =>
                    set(i, {
                      headers: e.target.value
                        .split("\n")
                        .map((l) => l.split(/:\s?(.*)/))
                        .filter((p) => (p[0] ?? "").trim())
                        .map((p) => ({ name: (p[0] ?? "").trim(), value: p[1] ?? "" })),
                    })
                  }
                />
              </label>
            </div>
          </li>
        ))}
      </ol>
      <div className="form-actions">
        <Button
          size="sm"
          icon="plus"
          onClick={() => setDraft((d) => [...d, newRule(d.length + 1)])}
        >
          Add session rule
        </Button>
        <span className="spacer" />
        <Button variant="ghost" disabled={!dirty} onClick={() => setDraft(saved)}>
          Revert
        </Button>
        <Button variant="primary" disabled={!dirty} onClick={() => void save()}>
          Save rules
        </Button>
      </div>
    </section>
  );
}
