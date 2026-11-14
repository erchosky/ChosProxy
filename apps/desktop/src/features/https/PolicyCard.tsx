import { useMemo, useState } from "react";
import type { ProxyConfig } from "../../bindings/ProxyConfig";
import { Button } from "../../components/Button";
import { LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { useAppStore } from "../../state/app";
import { linesToHosts, regexProblem, validateHostPatterns } from "./hostPatterns";

export function PolicyCard() {
  const proxyConfig = useAppStore((s) => s.proxyConfig);
  if (!proxyConfig) return <LoadingState />;
  // Re-keyed so the form resets after a save.
  return <PolicyForm key={JSON.stringify(proxyConfig.tls)} config={proxyConfig} />;
}

function PolicyForm({ config }: { config: ProxyConfig }) {
  const save = useAppStore((s) => s.saveProxyConfig);
  const busy = useAppStore((s) => s.busy);
  const [intercept, setIntercept] = useState(config.tls.intercept);
  const [passthrough, setPassthrough] = useState(config.tls.passthrough_hosts.join("\n"));
  const [skip, setSkip] = useState(config.tls.skip_verification_hosts.join("\n"));
  const [passRegex, setPassRegex] = useState(config.tls.passthrough_regex);
  const regexError = regexProblem(passRegex);
  const passErrors = useMemo(() => validateHostPatterns(passthrough), [passthrough]);
  const skipErrors = useMemo(() => validateHostPatterns(skip), [skip]);
  const next = {
    intercept,
    passthrough_hosts: linesToHosts(passthrough),
    skip_verification_hosts: linesToHosts(skip),
    passthrough_regex: passRegex.trim(),
  };
  const dirty = JSON.stringify(next) !== JSON.stringify(config.tls);
  const invalid = passErrors.length > 0 || skipErrors.length > 0 || regexError !== null;

  return (
    <section className="card">
      <h2>Interception policy</h2>
      <div className="field">
        <div>
          <div className="field-label">Inspect HTTPS</div>
          <div className="field-help">
            When off, HTTPS connections are relayed without decryption (like before Milestone 2).
          </div>
        </div>
        <Switch label={intercept ? "On" : "Off"} checked={intercept} onChange={setIntercept} />
      </div>
      <div className="field">
        <div>
          <div className="field-label">Never decrypt these hosts</div>
          <div className="field-help">
            One per line: <code>example.com</code>, <code>*.example.com</code>, or with a port (
            <code>example.com:8443</code>) for that port only. Use for apps with certificate pinning
            or sites outside your authorization.
          </div>
        </div>
        <div>
          <textarea
            rows={4}
            aria-label="Hosts never decrypted"
            aria-invalid={passErrors.length > 0}
            value={passthrough}
            onChange={(e) => setPassthrough(e.target.value)}
          />
          {passErrors.map((e) => (
            <div key={e} className="field-error">
              {e}
            </div>
          ))}
        </div>
      </div>
      <div className="field">
        <div>
          <div className="field-label">Also never decrypt (regex)</div>
          <div className="field-help">
            A regular expression matched against <code>host:port</code>, case-insensitive, e.g.{" "}
            <code>^pay\..*:443$</code>. Empty = off.
          </div>
        </div>
        <div>
          <input
            type="text"
            spellCheck={false}
            aria-label="Never decrypt hosts matching"
            aria-invalid={regexError !== null}
            value={passRegex}
            onChange={(e) => setPassRegex(e.target.value)}
          />
          {regexError && <div className="field-error">{regexError}</div>}
        </div>
      </div>
      <div className="field">
        <div>
          <div className="field-label">Skip certificate verification for</div>
          <div className="field-help">
            Upstream servers whose certificate is not checked — for labs with self-signed
            certificates. Every affected request is marked in its notes. Loopback is listed by
            default.
          </div>
        </div>
        <div>
          <textarea
            rows={4}
            aria-label="Hosts without certificate verification"
            aria-invalid={skipErrors.length > 0}
            value={skip}
            onChange={(e) => setSkip(e.target.value)}
          />
          {skipErrors.map((e) => (
            <div key={e} className="field-error">
              {e}
            </div>
          ))}
        </div>
      </div>
      <div className="form-actions">
        <span className="spacer" />
        <Button
          variant="ghost"
          disabled={!dirty}
          onClick={() => {
            setIntercept(config.tls.intercept);
            setPassthrough(config.tls.passthrough_hosts.join("\n"));
            setSkip(config.tls.skip_verification_hosts.join("\n"));
            setPassRegex(config.tls.passthrough_regex);
          }}
        >
          Discard changes
        </Button>
        <Button
          variant="primary"
          disabled={!dirty || invalid || busy !== null}
          onClick={() => void save({ ...config, tls: next })}
        >
          Save policy
        </Button>
      </div>
      <p className="muted">Applies to new connections.</p>
    </section>
  );
}
