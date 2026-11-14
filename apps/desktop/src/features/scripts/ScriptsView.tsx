import { useEffect, useState } from "react";
import type { ScriptEntry } from "../../bindings/ScriptEntry";
import type { ScriptKind } from "../../bindings/ScriptKind";
import type { ScriptRun } from "../../bindings/ScriptRun";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { SCRIPT_KINDS, TEMPLATES, scriptId } from "../../lib/scriptTemplates";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useHistoryStore } from "../../state/history";
import { useScriptsStore } from "../../state/scripts";

const kindLabel = (k: ScriptKind) => SCRIPT_KINDS.find((x) => x.id === k)?.label ?? k;

/** Tools → Scripts: the ChosScripts library (ADR-0032). */
export function ScriptsView() {
  const library = useScriptsStore((s) => s.library);
  const error = useScriptsStore((s) => s.error);
  const load = useScriptsStore((s) => s.load);
  const save = useScriptsStore((s) => s.save);
  const importScripts = useScriptsStore((s) => s.importScripts);
  const exportScripts = useScriptsStore((s) => s.exportScripts);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  if (error) return <ErrorState message={error} onRetry={() => void load()} />;
  if (!library) return <LoadingState />;
  const scripts = library.config.scripts;
  const current = scripts.find((s) => s.id === selected) ?? null;

  async function saveAll(next: ScriptEntry[]) {
    if (!library) return false;
    return save({ ...library.config, scripts: next });
  }

  async function create(kind: ScriptKind) {
    const name = `New ${kindLabel(kind).toLowerCase()} script`;
    const id = scriptId(
      name,
      scripts.map((s) => s.id),
    );
    const entry: ScriptEntry = {
      id,
      name,
      kind,
      enabled: false,
      description: "",
      source: TEMPLATES[kind],
    };
    if (await saveAll([...scripts, entry])) setSelected(id);
  }

  return (
    <div className="page scripts-page">
      <div className="page-header">
        <div>
          <h1>Scripts</h1>
          <p className="page-sub">
            Small scripts (Rhai) for checks, history filters and columns, actions and Match &amp;
            Replace. They run in a sandbox: no files, no network, limited time and memory, and they
            only see the message they are given.
          </p>
        </div>
      </div>
      <div className="scripts-layout">
        <aside className="card scripts-list" aria-label="Script library">
          <div className="form-actions">
            <Button size="sm" icon="folder" onClick={() => void importScripts()}>
              Import…
            </Button>
            <Button
              size="sm"
              icon="file"
              disabled={scripts.length === 0}
              onClick={() => void exportScripts([])}
            >
              Export all…
            </Button>
          </div>
          <div className="form-actions">
            {SCRIPT_KINDS.map((k) => (
              <Button
                key={k.id}
                size="sm"
                icon="plus"
                title={k.help}
                onClick={() => void create(k.id)}
              >
                {k.label}
              </Button>
            ))}
          </div>
          {scripts.length === 0 ? (
            <EmptyState title="No scripts yet">
              Start from a template with the buttons above; each one works on the lab server.
            </EmptyState>
          ) : (
            <ul role="listbox" aria-label="Scripts">
              {scripts.map((s) => (
                <li key={s.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={s.id === selected}
                    className={`script-item${s.id === selected ? " selected" : ""}`}
                    onClick={() => setSelected(s.id)}
                  >
                    <span className="script-name">{s.name}</span>
                    <Badge tone="neutral">{kindLabel(s.kind)}</Badge>
                    {s.id in library.problems ? (
                      <Badge tone="danger" title={library.problems[s.id]?.message}>
                        error
                      </Badge>
                    ) : (
                      !s.enabled && <Badge tone="neutral">off</Badge>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>
        {current ? (
          <ScriptEditor
            key={current.id + JSON.stringify(current)}
            entry={current}
            problem={library.problems[current.id]?.message ?? null}
            onSave={async (entry) => {
              await saveAll(scripts.map((s) => (s.id === entry.id ? entry : s)));
            }}
            onDelete={async () => {
              const ok = await dialogs.confirm(
                `Delete “${current.name}”? Rules or views that use it stop using it.`,
                "Delete script?",
                "Delete",
              );
              if (ok && (await saveAll(scripts.filter((s) => s.id !== current.id)))) {
                setSelected(null);
              }
            }}
          />
        ) : (
          <section className="card">
            <EmptyState title="Choose or create a script">
              {SCRIPT_KINDS.map((k) => (
                <p key={k.id}>
                  <strong>{k.label}:</strong> {k.help}
                </p>
              ))}
            </EmptyState>
          </section>
        )}
      </div>
    </div>
  );
}

function ScriptEditor({
  entry,
  problem,
  onSave,
  onDelete,
}: {
  entry: ScriptEntry;
  problem: string | null;
  onSave(entry: ScriptEntry): Promise<void>;
  onDelete(): Promise<void>;
}) {
  const [draft, setDraft] = useState(entry);
  const [exchange, setExchange] = useState(() => {
    const id = useHistoryStore.getState().selectedId;
    return id === null ? "" : String(id);
  });
  const [run, setRun] = useState<ScriptRun | null>(null);
  const [testing, setTesting] = useState(false);
  const dirty = JSON.stringify(draft) !== JSON.stringify(entry);
  const help = SCRIPT_KINDS.find((k) => k.id === draft.kind)?.help;

  async function test() {
    setTesting(true);
    try {
      const id = exchange.trim() === "" ? null : Number(exchange);
      setRun(await api.scriptTest(draft.kind, draft.source, Number.isFinite(id) ? id : null));
    } catch (e) {
      setRun({
        problem: { message: toIpcError(e).message, line: null },
        output: null,
        findings: [],
        printed: [],
        elapsedMs: 0,
      });
    } finally {
      setTesting(false);
    }
  }

  return (
    <section className="card script-editor" aria-label={`Script ${entry.name}`}>
      <div className="script-fields">
        <label>
          Name
          <input
            type="text"
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          />
        </label>
        <label>
          Kind
          <input type="text" value={kindLabel(draft.kind)} readOnly aria-readonly />
        </label>
        <Switch
          label="Enabled"
          checked={draft.enabled}
          onChange={(enabled) => setDraft({ ...draft, enabled })}
        />
      </div>
      <p className="muted rule-hint">{help}</p>
      <label className="script-description">
        Description
        <input
          type="text"
          value={draft.description}
          placeholder="What it does (shown on findings of check scripts)"
          onChange={(e) => setDraft({ ...draft, description: e.target.value })}
        />
      </label>
      <textarea
        className="script-source"
        spellCheck={false}
        rows={18}
        aria-label="Script source"
        value={draft.source}
        onChange={(e) => setDraft({ ...draft, source: e.target.value })}
      />
      {problem && !dirty && <p className="callout callout-danger">Saved version: {problem}</p>}
      <div className="form-actions">
        <label className="inline-field">
          Test on exchange #
          <input
            type="number"
            min={1}
            value={exchange}
            placeholder="none"
            aria-label="Exchange id to test on"
            onChange={(e) => setExchange(e.target.value)}
          />
        </label>
        <Button icon="play" disabled={testing} onClick={() => void test()}>
          {exchange.trim() === "" ? "Check" : "Test"}
        </Button>
        <span className="spacer" />
        <Button variant="danger" icon="trash" onClick={() => void onDelete()}>
          Delete
        </Button>
        <Button variant="ghost" disabled={!dirty} onClick={() => setDraft(entry)}>
          Revert
        </Button>
        <Button
          variant="primary"
          disabled={!dirty || draft.name.trim() === ""}
          onClick={() => void onSave(draft)}
        >
          Save
        </Button>
      </div>
      {run && <RunResult run={run} />}
    </section>
  );
}

function RunResult({ run }: { run: ScriptRun }) {
  return (
    <div className="script-run" aria-live="polite">
      {run.problem ? (
        <p className="callout callout-danger">
          {run.problem.line !== null ? `Line ${run.problem.line}: ` : ""}
          {run.problem.message}
        </p>
      ) : (
        <p className="muted">Ran in {run.elapsedMs} ms.</p>
      )}
      {run.output !== null && run.output !== "" && (
        <pre className="script-output">{run.output}</pre>
      )}
      {run.findings.length > 0 && (
        <ul className="script-findings">
          {run.findings.map((f, i) => (
            <li key={i}>
              <Badge tone={f.severity === "high" || f.severity === "medium" ? "danger" : "warn"}>
                {f.severity}
              </Badge>{" "}
              <strong>{f.title}</strong> <span className="muted">({f.confidence})</span>
              {f.evidence && <div className="muted">{f.evidence}</div>}
            </li>
          ))}
        </ul>
      )}
      {run.printed.length > 0 && (
        <pre className="script-output script-printed">{run.printed.join("\n")}</pre>
      )}
    </div>
  );
}
