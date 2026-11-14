import { useEffect, useState } from "react";
import type { MatchReplaceRule } from "../../bindings/MatchReplaceRule";
import type { MatchTarget } from "../../bindings/MatchTarget";
import type { ProxyConfig } from "../../bindings/ProxyConfig";
import { Button, IconButton } from "../../components/Button";
import {
  MATCH_TARGETS,
  describeMatchRule,
  isScriptTarget,
  matchProblem,
  moveItem,
  newMatchRule,
} from "../../lib/network";
import { useAppStore } from "../../state/app";
import { useScriptsStore } from "../../state/scripts";

/** Rules that change proxied requests and responses automatically. */
export function MatchReplaceCard() {
  const proxyConfig = useAppStore((s) => s.proxyConfig);
  if (!proxyConfig) return null;
  return <RulesEditor key={JSON.stringify(proxyConfig.match_replace)} config={proxyConfig} />;
}

function RulesEditor({ config }: { config: ProxyConfig }) {
  const save = useAppStore((s) => s.saveProxyConfig);
  const busy = useAppStore((s) => s.busy !== null);
  const [rules, setRules] = useState<MatchReplaceRule[]>(config.match_replace);
  const dirty = JSON.stringify(rules) !== JSON.stringify(config.match_replace);
  const problems = rules.map(matchProblem);
  const invalid = problems.some((p) => p !== null);
  const library = useScriptsStore((s) => s.library);
  const loadScripts = useScriptsStore((s) => s.load);
  useEffect(() => {
    if (!library) void loadScripts();
  }, [library, loadScripts]);
  const rewriteScripts = library?.config.scripts.filter((s) => s.kind === "rewrite") ?? [];
  const set = (i: number, patch: Partial<MatchReplaceRule>) =>
    setRules((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  return (
    <section className="card match-replace" aria-label="Match and Replace">
      <h2>Match &amp; Replace</h2>
      <p className="muted rule-hint">
        Changes every proxied message that matches, in order, before Intercept shows it. Header
        rules work on each header line: an empty “find” adds the replacement as a new header, an
        empty result removes the line. Compressed or chunked bodies are decoded first. HTTP history
        keeps the original next to the changed message.
      </p>
      {rules.length === 0 ? (
        <p className="muted">No rules.</p>
      ) : (
        <ol className="rule-list">
          {rules.map((r, i) => (
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
                  Where
                  <select
                    value={r.target}
                    aria-label={`Part changed by rule ${i + 1}`}
                    onChange={(e) => set(i, { target: e.target.value as MatchTarget })}
                  >
                    {MATCH_TARGETS.map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                </label>
                {isScriptTarget(r.target) ? (
                  <label className="grow">
                    Rewrite script
                    <select
                      value={r.match}
                      aria-label={`Rewrite script of rule ${i + 1}`}
                      onChange={(e) => set(i, { match: e.target.value })}
                    >
                      <option value="">Choose…</option>
                      {rewriteScripts.map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.name}
                          {s.enabled ? "" : " (off)"}
                        </option>
                      ))}
                    </select>
                  </label>
                ) : (
                  <label className="grow">
                    Find
                    <input
                      type="text"
                      spellCheck={false}
                      value={r.match}
                      aria-label={`Text to find, rule ${i + 1}`}
                      onChange={(e) => set(i, { match: e.target.value })}
                    />
                  </label>
                )}
                {!isScriptTarget(r.target) && (
                  <label className="grow">
                    Replace with
                    <input
                      type="text"
                      spellCheck={false}
                      value={r.replace}
                      aria-label={`Replacement, rule ${i + 1}`}
                      onChange={(e) => set(i, { replace: e.target.value })}
                    />
                  </label>
                )}
                {!isScriptTarget(r.target) && (
                  <label className="check" title="Regular expression; $1 inserts a group">
                    <input
                      type="checkbox"
                      checked={r.regex}
                      onChange={(e) => set(i, { regex: e.target.checked })}
                    />
                    Regex
                  </label>
                )}
                <label>
                  Comment
                  <input
                    type="text"
                    value={r.comment}
                    aria-label={`Comment of rule ${i + 1}`}
                    onChange={(e) => set(i, { comment: e.target.value })}
                  />
                </label>
              </div>
              <div className="rule-actions">
                <IconButton
                  icon="arrowUp"
                  label="Move up"
                  disabled={i === 0}
                  onClick={() => setRules((rs) => moveItem(rs, i, -1))}
                />
                <IconButton
                  icon="arrowDown"
                  label="Move down"
                  disabled={i === rules.length - 1}
                  onClick={() => setRules((rs) => moveItem(rs, i, 1))}
                />
                <IconButton
                  icon="trash"
                  label={`Remove rule ${i + 1}`}
                  onClick={() => setRules((rs) => rs.filter((_, j) => j !== i))}
                />
              </div>
              <div className="rule-summary">
                {problems[i] ? (
                  <span className="field-error">{problems[i]}</span>
                ) : (
                  <span className="muted">{describeMatchRule(r)}</span>
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
      <div className="form-actions">
        <Button size="sm" icon="plus" onClick={() => setRules((rs) => [...rs, newMatchRule()])}>
          Add rule
        </Button>
        <span className="spacer" />
        <Button
          variant="ghost"
          disabled={!dirty || busy}
          onClick={() => setRules(config.match_replace)}
        >
          Revert
        </Button>
        <Button
          variant="primary"
          disabled={!dirty || busy || invalid}
          onClick={() => void save({ ...config, match_replace: rules })}
        >
          Save rules
        </Button>
      </div>
    </section>
  );
}
