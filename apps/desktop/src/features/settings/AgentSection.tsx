import { useEffect, useState } from "react";
import type { AgentConfig } from "../../bindings/AgentConfig";
import type { AgentPermission } from "../../bindings/AgentPermission";
import type { AgentToolInfo } from "../../bindings/AgentToolInfo";
import type { AgentToolKind } from "../../bindings/AgentToolKind";
import { Button } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { Segmented } from "../../components/Tabs";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

const ORDER: AgentPermission[] = ["deny", "ask", "allow"];
const LABEL: Record<AgentPermission, string> = { deny: "Deny", ask: "Ask", allow: "Allow" };
const KIND: Record<AgentToolKind, string> = {
  read: "reads the project",
  task: "the task's own plan",
  project: "changes the project",
  send: "sends traffic",
};

/** Permissions a tool can be given (never above its highest). */
export function choicesFor(tool: Pick<AgentToolInfo, "highest">): AgentPermission[] {
  return ORDER.filter((p) => ORDER.indexOf(p) <= ORDER.indexOf(tool.highest));
}

/** Settings → ChosAgent (M30): what each tool may do, and the limits. */
export function AgentSection() {
  const [config, setConfig] = useState<AgentConfig | null>(null);
  const [tools, setTools] = useState<AgentToolInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    Promise.all([api.agentConfig(), api.agentTools()]).then(
      ([c, t]) => {
        if (!alive) return;
        setConfig(c);
        setTools(t);
      },
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
  }, []);

  if (error) return <ErrorState title="Could not read the ChosAgent settings" message={error} />;
  if (!config) return <LoadingState />;
  const permission = (t: AgentToolInfo): AgentPermission => {
    const set = config.permissions[t.name] ?? t.default;
    return ORDER.indexOf(set) > ORDER.indexOf(t.highest) ? t.highest : set;
  };

  async function save() {
    if (!config) return;
    try {
      const saved = await api.saveAgentConfig(config);
      setConfig(saved);
      setTools(await api.agentTools());
      notify("info", "ChosAgent settings saved");
    } catch (e) {
      notify("error", `Not saved: ${toIpcError(e).message}`);
    }
  }

  return (
    <section className="card" aria-label="ChosAgent">
      <h2>ChosAgent</h2>
      <p className="muted rule-hint">
        What the agent may do with each of ChosProxy&apos;s tools. Deny: refused. Ask: waits for
        your approval of the exact call. Allow: runs, and is recorded in the AI activity log. These
        are enforced by ChosProxy, not by the model&apos;s instructions. Sending a request can only
        be Ask or Deny, needs a scope and goes only to targets in it.
      </p>
      <table className="endpoint-table" aria-label="Agent tool permissions">
        <thead>
          <tr>
            <th>Tool</th>
            <th>What it does</th>
            <th>Permission</th>
          </tr>
        </thead>
        <tbody>
          {tools.map((t) => (
            <tr key={t.name}>
              <td className="mono">{t.name}</td>
              <td>
                {t.summary} <span className="muted">({KIND[t.kind]})</span>
              </td>
              <td>
                <Segmented<AgentPermission>
                  items={choicesFor(t).map((p) => ({ id: p, label: LABEL[p] }))}
                  value={permission(t)}
                  onChange={(p) =>
                    setConfig({ ...config, permissions: { ...config.permissions, [t.name]: p } })
                  }
                  label={`Permission of ${t.name}`}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="dialog-row">
        <label className="inline-field">
          Model calls per task
          <input
            type="number"
            min={1}
            max={200}
            value={config.maxSteps}
            onChange={(e) => setConfig({ ...config, maxSteps: Number(e.target.value) })}
          />
        </label>
        <label className="inline-field">
          Requests per task
          <input
            type="number"
            min={0}
            max={500}
            value={config.maxSends}
            onChange={(e) => setConfig({ ...config, maxSends: Number(e.target.value) })}
          />
        </label>
      </div>
      <div className="form-actions">
        <Button variant="primary" size="sm" onClick={() => void save()}>
          Save
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setConfig({ ...config, permissions: {} })}>
          Defaults
        </Button>
      </div>
    </section>
  );
}
