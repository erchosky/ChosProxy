import { useMemo, useState } from "react";
import { mergeRules, parseScopeList } from "../../lib/scopeImport";
import { Segmented } from "../../components/Tabs";
import { useAppStore } from "../../state/app";
import type { ScopeConfig } from "../../bindings/ScopeConfig";
import type { ScopeDecision } from "../../bindings/ScopeDecision";
import type { ScopeProtocol } from "../../bindings/ScopeProtocol";
import type { ScopeRule } from "../../bindings/ScopeRule";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { toIpcError } from "../../services/ipc";
import { activeIncludes, useScopeStore } from "../../state/scope";

const NEW_RULE: ScopeRule = {
  enabled: true,
  protocol: "any",
  host: "",
  includeSubdomains: false,
  port: null,
  path: "",
  regex: false,
  comment: "",
};

export function ScopeView() {
  const config = useScopeStore((s) => s.config);
  const error = useScopeStore((s) => s.error);
  const load = useScopeStore((s) => s.load);
  if (error) return <ErrorState title="Could not load the scope" message={error} onRetry={load} />;
  if (!config) return <LoadingState />;
  // Re-mount the editor when the saved rules change (e.g. "Add to scope" elsewhere).
  return <ScopeEditor key={JSON.stringify(config)} saved={config} />;
}

function ScopeSource() {
  const project = useAppStore((s) => s.project);
  const projectOverride = useScopeStore((s) => s.projectOverride);
  const setOverride = useScopeStore((s) => s.setOverride);
  if (!project) return null;
  return (
    <div className="scope-source">
      <span className="field-label">This project uses</span>
      <Segmented<"global" | "project">
        label="Scope used by this project"
        value={projectOverride ? "project" : "global"}
        onChange={(v) => void setOverride(v === "project")}
        items={[
          {
            id: "global",
            label: "Use global scope",
            title: "Shared by every project (scope.toml)",
          },
          {
            id: "project",
            label: "Override for this project",
            title: "Stored in the project file",
          },
        ]}
      />
    </div>
  );
}

function ScopeEditor({ saved }: { saved: ScopeConfig }) {
  const save = useScopeStore((s) => s.save);
  const saving = useScopeStore((s) => s.saving);
  const [draft, setDraft] = useState<ScopeConfig>(saved);
  const [saveError, setSaveError] = useState<string | null>(null);
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const setList = (list: "include" | "exclude", rules: ScopeRule[]) =>
    setDraft((d) => ({ ...d, [list]: rules }));

  return (
    <div className="page scope-view">
      <div className="page-header">
        <div>
          <h1>Scope</h1>
          <p className="page-sub">
            A request is <strong>in scope</strong> when an enabled include rule matches it and no
            exclude rule does. With no include rules nothing is in scope. History, the site map,
            search and Intercept can be limited to the scope; later automated tools will refuse
            anything outside it.
          </p>
          <ScopeSource />
        </div>
        <Badge tone={activeIncludes(saved) > 0 ? "accent" : "neutral"}>
          {activeIncludes(saved) > 0
            ? `${activeIncludes(saved)} include rule${activeIncludes(saved) === 1 ? "" : "s"}`
            : "No scope defined"}
        </Badge>
      </div>
      <RuleTable
        title="Include"
        help="Targets you are authorized to test."
        rules={draft.include}
        onChange={(r) => setList("include", r)}
      />
      <RuleTable
        title="Exclude"
        help="Never in scope, even if an include rule matches (e.g. logout or third-party hosts)."
        rules={draft.exclude}
        onChange={(r) => setList("exclude", r)}
      />
      <ScopeImporter onImport={(list, rules) => setList(list, mergeRules(draft[list], rules))} />
      <label className="check">
        <input
          type="checkbox"
          checked={draft.skip_out_of_scope_proxy}
          onChange={(e) => setDraft((d) => ({ ...d, skip_out_of_scope_proxy: e.target.checked }))}
        />
        Don’t store out-of-scope proxy traffic
        <span className="muted">
          — it is still forwarded, but kept out of the history, WebSockets and the cookie jar.
          Ignored while no scope is defined.
        </span>
      </label>
      {saveError && (
        <div className="callout callout-danger selectable" role="alert">
          {saveError}
        </div>
      )}
      <div className="form-actions">
        <Button
          variant="primary"
          disabled={!dirty || saving}
          onClick={async () => setSaveError(await save(draft))}
        >
          {saving ? "Saving…" : "Save scope"}
        </Button>
        <Button
          variant="ghost"
          disabled={!dirty || saving}
          onClick={() => {
            setDraft(saved);
            setSaveError(null);
          }}
        >
          Revert
        </Button>
        {dirty && <span className="muted">Unsaved changes — the test below uses saved rules.</span>}
      </div>
      <ScopeTester saved={saved} />
    </div>
  );
}

interface RuleTableProps {
  title: string;
  help: string;
  rules: ScopeRule[];
  onChange(rules: ScopeRule[]): void;
}

function RuleTable({ title, help, rules, onChange }: RuleTableProps) {
  const update = (i: number, patch: Partial<ScopeRule>) =>
    onChange(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>{title}</h2>
          <div className="field-help">{help}</div>
        </div>
        <Button size="sm" icon="plus" onClick={() => onChange([...rules, NEW_RULE])}>
          Add rule
        </Button>
      </div>
      {rules.length === 0 ? (
        <p className="muted">No {title.toLowerCase()} rules.</p>
      ) : (
        <div className="scope-rules" role="table" aria-label={`${title} rules`}>
          <div className="scope-rule scope-rule-head" role="row">
            <span>On</span>
            <span>Protocol</span>
            <span>Host</span>
            <span>Port</span>
            <span>Path</span>
            <span>Options</span>
            <span>Comment</span>
            <span />
          </div>
          {rules.map((r, i) => (
            // Rules have no identity besides their position.
            <div className="scope-rule" role="row" key={i}>
              <input
                type="checkbox"
                aria-label="Enabled"
                checked={r.enabled}
                onChange={(e) => update(i, { enabled: e.target.checked })}
              />
              <select
                aria-label="Protocol"
                value={r.protocol}
                onChange={(e) => update(i, { protocol: e.target.value as ScopeProtocol })}
              >
                <option value="any">Any</option>
                <option value="http">HTTP</option>
                <option value="https">HTTPS</option>
              </select>
              <input
                className="mono"
                aria-label="Host"
                placeholder={
                  r.regex ? "regex, e.g. (dev|qa)\\.lab\\.test" : "lab.test (empty = any)"
                }
                value={r.host}
                onChange={(e) => update(i, { host: e.target.value })}
              />
              <input
                className="mono"
                aria-label="Port"
                inputMode="numeric"
                placeholder="any"
                value={r.port ?? ""}
                onChange={(e) => {
                  const v = e.target.value.trim();
                  const n = Number(v);
                  update(i, {
                    port: v === "" ? null : Number.isInteger(n) && n > 0 && n < 65536 ? n : r.port,
                  });
                }}
              />
              <input
                className="mono"
                aria-label="Path"
                placeholder={r.regex ? "regex searched in the path" : "/prefix (empty = any)"}
                value={r.path}
                onChange={(e) => update(i, { path: e.target.value })}
              />
              <span className="scope-rule-options">
                <label title="Also match subdomains of the host">
                  <input
                    type="checkbox"
                    checked={r.includeSubdomains}
                    disabled={r.regex}
                    onChange={(e) => update(i, { includeSubdomains: e.target.checked })}
                  />
                  subdomains
                </label>
                <label title="Host and path are regular expressions">
                  <input
                    type="checkbox"
                    checked={r.regex}
                    onChange={(e) => update(i, { regex: e.target.checked })}
                  />
                  regex
                </label>
              </span>
              <input
                aria-label="Comment"
                value={r.comment}
                onChange={(e) => update(i, { comment: e.target.value })}
              />
              <IconButton
                icon="trash"
                label="Remove rule"
                onClick={() => onChange(rules.filter((_, j) => j !== i))}
              />
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function ScopeTester({ saved }: { saved: ScopeConfig }) {
  const check = useScopeStore((s) => s.check);
  const [url, setUrl] = useState("");
  const [result, setResult] = useState<ScopeDecision | string | null>(null);
  async function test() {
    try {
      setResult(await check(url));
    } catch (e) {
      setResult(toIpcError(e).message);
    }
  }
  const ruleText = (list: "include" | "exclude", i: number | null) => {
    if (i === null) return null;
    const r = saved[list][i];
    return r ? `${list} rule ${i + 1}${r.comment ? ` (${r.comment})` : ""}` : null;
  };
  return (
    <section className="card">
      <h2>Test a URL</h2>
      <form
        className="scope-test"
        onSubmit={(e) => {
          e.preventDefault();
          void test();
        }}
      >
        <input
          className="mono grow"
          aria-label="URL to test"
          placeholder="https://lab.test/api/users?id=1"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
        <Button type="submit" disabled={url.trim() === ""}>
          Check
        </Button>
      </form>
      {typeof result === "string" && <p className="field-error">{result}</p>}
      {result !== null && typeof result !== "string" && (
        <p>
          <Badge tone={result.inScope ? "ok" : "warn"}>
            {result.inScope ? "IN SCOPE" : "OUT OF SCOPE"}
          </Badge>{" "}
          <span className="muted">
            {result.inScope
              ? `Matched ${ruleText("include", result.includeRule)}.`
              : result.excludeRule !== null
                ? `Matched ${ruleText("include", result.includeRule)} but excluded by ${ruleText("exclude", result.excludeRule)}.`
                : "No enabled include rule matches."}
          </span>
        </p>
      )}
    </section>
  );
}

/** Paste a list of hosts or URLs to add them as rules (saved with "Save scope"). */
function ScopeImporter({
  onImport,
}: {
  onImport(list: "include" | "exclude", rules: ScopeRule[]): void;
}) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [list, setListName] = useState<"include" | "exclude">("include");
  const parsed = useMemo(() => parseScopeList(text), [text]);
  if (!open)
    return (
      <div className="form-actions">
        <Button size="sm" variant="ghost" icon="plus" onClick={() => setOpen(true)}>
          Import a list of hosts or URLs…
        </Button>
      </div>
    );
  return (
    <section className="card scope-import" aria-label="Import scope rules">
      <label>
        One host or URL per line (<code>example.com</code>, <code>*.example.com</code>,{" "}
        <code>https://app.example.com:8443/api/</code>)
        <textarea
          rows={5}
          className="mono"
          spellCheck={false}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <div className="dialog-row">
        <select
          aria-label="Add to"
          value={list}
          onChange={(e) => setListName(e.target.value as "include" | "exclude")}
        >
          <option value="include">Add to Include</option>
          <option value="exclude">Add to Exclude</option>
        </select>
        <Button
          variant="primary"
          disabled={parsed.rules.length === 0}
          onClick={() => {
            onImport(list, parsed.rules);
            setText("");
            setOpen(false);
          }}
        >
          Add {parsed.rules.length} rule{parsed.rules.length === 1 ? "" : "s"}
        </Button>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          Cancel
        </Button>
        {parsed.skipped.length > 0 && (
          <span className="field-error">
            Not understood: {parsed.skipped.slice(0, 3).join(", ")}
            {parsed.skipped.length > 3 && ` and ${parsed.skipped.length - 3} more`}
          </span>
        )}
      </div>
    </section>
  );
}
