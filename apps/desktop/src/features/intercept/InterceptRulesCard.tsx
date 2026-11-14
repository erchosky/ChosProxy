import { useMemo, useState } from "react";
import type { InterceptRules } from "../../bindings/InterceptRules";
import type { ProxyConfig } from "../../bindings/ProxyConfig";
import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { METHODS, toggle } from "../../lib/filters";
import { useAppStore } from "../../state/app";
import { linesToHosts, regexProblem, validateHostPatterns } from "../https/hostPatterns";

const MIME_CATEGORIES = ["HTML", "JSON", "script", "XML", "text", "form", "CSS", "image"] as const;

export function InterceptRulesCard() {
  const proxyConfig = useAppStore((s) => s.proxyConfig);
  if (!proxyConfig) return null;
  return <RulesForm key={JSON.stringify(proxyConfig.intercept)} config={proxyConfig} />;
}

function RulesForm({ config }: { config: ProxyConfig }) {
  const save = useAppStore((s) => s.saveProxyConfig);
  const busy = useAppStore((s) => s.busy);
  const [rules, setRules] = useState<InterceptRules>(config.intercept);
  const [include, setInclude] = useState(config.intercept.include_hosts.join("\n"));
  const [exclude, setExclude] = useState(config.intercept.exclude_hosts.join("\n"));
  const errors = useMemo(
    () => [...validateHostPatterns(include), ...validateHostPatterns(exclude)],
    [include, exclude],
  );
  const next: InterceptRules = {
    ...rules,
    include_hosts: linesToHosts(include),
    exclude_hosts: linesToHosts(exclude),
  };
  const dirty = JSON.stringify(next) !== JSON.stringify(config.intercept);
  const patch = (p: Partial<InterceptRules>) => setRules((r) => ({ ...r, ...p }));

  return (
    <section className="card intercept-rules">
      <h2>Rules</h2>
      <div className="rules-grid">
        <Switch
          label="Pause requests"
          checked={rules.requests}
          onChange={(v) => patch({ requests: v })}
        />
        <Switch
          label="Pause responses"
          checked={rules.responses}
          onChange={(v) => patch({ responses: v })}
        />
        <Switch
          label="Only requests with parameters"
          checked={rules.only_with_parameters}
          onChange={(v) => patch({ only_with_parameters: v })}
        />
        <Switch
          label="Ignore static assets"
          checked={rules.ignore_static_assets}
          onChange={(v) => patch({ ignore_static_assets: v })}
        />
        <Switch
          label="Pause WebSocket messages to the server"
          checked={rules.websocket_to_server}
          onChange={(v) => patch({ websocket_to_server: v })}
        />
        <Switch
          label="Pause WebSocket messages to the client"
          checked={rules.websocket_to_client}
          onChange={(v) => patch({ websocket_to_client: v })}
        />
        <Switch
          label="Only in-scope targets"
          checked={rules.only_in_scope}
          onChange={(v) => patch({ only_in_scope: v })}
        />
      </div>
      <div className="field">
        <div>
          <div className="field-label">Methods</div>
          <div className="field-help">None selected = every method.</div>
        </div>
        <div className="chip-group" role="group" aria-label="Methods to pause">
          {METHODS.map((m) => (
            <button
              key={m}
              type="button"
              className="chip"
              aria-pressed={rules.methods.includes(m)}
              onClick={() => patch({ methods: toggle(rules.methods, m) })}
            >
              {m}
            </button>
          ))}
        </div>
      </div>
      <div className="field">
        <div>
          <div className="field-label">Response types</div>
          <div className="field-help">None selected = every type.</div>
        </div>
        <div className="chip-group" role="group" aria-label="Response types to pause">
          {MIME_CATEGORIES.map((m) => (
            <button
              key={m}
              type="button"
              className="chip"
              aria-pressed={rules.response_mime.includes(m)}
              onClick={() => patch({ response_mime: toggle(rules.response_mime, m) })}
            >
              {m}
            </button>
          ))}
        </div>
      </div>
      <div className="field">
        <div>
          <div className="field-label">Only URLs matching</div>
          <div className="field-help">
            A regular expression, case-insensitive, e.g. <code>/api/</code>. Empty = every URL.
          </div>
        </div>
        <div>
          <input
            type="text"
            spellCheck={false}
            aria-label="Only pause URLs matching"
            aria-invalid={regexProblem(rules.url_regex) !== null}
            value={rules.url_regex}
            onChange={(e) => patch({ url_regex: e.target.value })}
          />
          {regexProblem(rules.url_regex) && (
            <div className="field-error">{regexProblem(rules.url_regex)}</div>
          )}
        </div>
      </div>
      <div className="field">
        <div>
          <div className="field-label">Only these hosts</div>
          <div className="field-help">
            One per line, <code>example.com</code> or <code>*.example.com</code>. Empty = all hosts.
            For scope, use “Only in-scope targets” above.
          </div>
        </div>
        <textarea
          rows={3}
          aria-label="Hosts to pause"
          value={include}
          onChange={(e) => setInclude(e.target.value)}
        />
      </div>
      <div className="field">
        <div>
          <div className="field-label">Never these hosts</div>
        </div>
        <textarea
          rows={3}
          aria-label="Hosts never paused"
          value={exclude}
          onChange={(e) => setExclude(e.target.value)}
        />
      </div>
      {errors.map((e) => (
        <div key={e} className="field-error">
          {e}
        </div>
      ))}
      <div className="form-actions">
        <span className="spacer" />
        <Button
          variant="ghost"
          disabled={!dirty}
          onClick={() => {
            setRules(config.intercept);
            setInclude(config.intercept.include_hosts.join("\n"));
            setExclude(config.intercept.exclude_hosts.join("\n"));
          }}
        >
          Discard changes
        </Button>
        <Button
          variant="primary"
          disabled={
            !dirty || errors.length > 0 || regexProblem(rules.url_regex) !== null || busy !== null
          }
          onClick={() => void save({ ...config, intercept: next })}
        >
          Save rules
        </Button>
      </div>
    </section>
  );
}
