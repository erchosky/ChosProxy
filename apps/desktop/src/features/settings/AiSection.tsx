import { useEffect, useState } from "react";
import type { AiConfig } from "../../bindings/AiConfig";
import type { AiMode } from "../../bindings/AiMode";
import type { CredentialInfo } from "../../bindings/CredentialInfo";
import { Button } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { Segmented } from "../../components/Tabs";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

/** Typical base URLs of local model servers. */
export const LOCAL_SERVERS: { label: string; provider: AiConfig["provider"]; url: string }[] = [
  { label: "Ollama", provider: "ollama", url: "http://127.0.0.1:11434" },
  { label: "LM Studio", provider: "openAiCompatible", url: "http://127.0.0.1:1234" },
  { label: "llama.cpp server", provider: "openAiCompatible", url: "http://127.0.0.1:8080" },
];

/** Settings → ChosAI (M29). */
export function AiSection() {
  const [config, setConfig] = useState<AiConfig | null>(null);
  const [credentials, setCredentials] = useState<CredentialInfo[]>([]);
  const [patterns, setPatterns] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    api.aiConfig().then(
      (c) => {
        if (!alive) return;
        setConfig(c);
        setPatterns(c.redactPatterns.join("\n"));
      },
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    api.credentials().then(
      (l) => alive && setCredentials(l),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);

  if (error) return <ErrorState title="Could not read the ChosAI settings" message={error} />;
  if (!config) return <LoadingState />;
  const edit = (patch: Partial<AiConfig>) => setConfig({ ...config, ...patch });
  const full = (): AiConfig => ({
    ...config,
    redactPatterns: patterns
      .split("\n")
      .map((p) => p.trim())
      .filter(Boolean),
  });

  async function save() {
    try {
      const saved = await api.saveAiConfig(full());
      setConfig(saved);
      notify("info", "ChosAI settings saved");
    } catch (e) {
      notify("error", `Not saved: ${toIpcError(e).message}`);
    }
  }

  async function test() {
    setTesting(true);
    try {
      const a = await api.aiTest(full());
      notify(
        "info",
        `${a.model} answered in ${(a.elapsedMs / 1000).toFixed(1)} s: ${a.text.slice(0, 60)}`,
      );
    } catch (e) {
      notify("error", toIpcError(e).message);
    } finally {
      setTesting(false);
    }
  }

  return (
    <section className="card" aria-label="ChosAI">
      <h2>ChosAI</h2>
      <p className="muted rule-hint">
        Explanations, summaries and report wording from a language model. Only the items you attach
        are sent, with credentials and secrets redacted; you can always preview the exact text. A
        model on this machine (Ollama, LM Studio, llama.cpp) keeps everything local. An external
        provider only gets a context you have previewed. ChosAI writes text; it never sends traffic.
      </p>
      <Segmented<AiMode>
        items={[
          { id: "disabled", label: "Off" },
          { id: "local", label: "Local model" },
          { id: "external", label: "External provider" },
        ]}
        value={config.mode}
        onChange={(mode) => edit({ mode })}
        label="ChosAI mode"
      />
      {config.mode !== "disabled" && (
        <>
          {config.mode === "local" && (
            <div className="dialog-row">
              {LOCAL_SERVERS.map((s) => (
                <Button
                  key={s.label}
                  size="sm"
                  variant="ghost"
                  onClick={() => edit({ provider: s.provider, baseUrl: s.url })}
                >
                  {s.label}
                </Button>
              ))}
            </div>
          )}
          <div className="dialog-row">
            <label className="inline-field">
              API
              <select
                value={config.provider}
                onChange={(e) => edit({ provider: e.target.value as AiConfig["provider"] })}
              >
                <option value="ollama">Ollama</option>
                <option value="openAiCompatible">OpenAI-compatible</option>
              </select>
            </label>
            <label className="inline-field">
              URL
              <input
                className="mono"
                value={config.baseUrl}
                onChange={(e) => edit({ baseUrl: e.target.value })}
              />
            </label>
            <label className="inline-field">
              Model
              <input value={config.model} onChange={(e) => edit({ model: e.target.value })} />
            </label>
          </div>
          <div className="dialog-row">
            <label className="inline-field">
              API key
              <select
                value={config.apiKeyCredential ?? ""}
                onChange={(e) => edit({ apiKeyCredential: e.target.value || null })}
              >
                <option value="">None</option>
                {credentials.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.label} (Settings → Credentials)
                  </option>
                ))}
              </select>
            </label>
            <label className="inline-field">
              Timeout (s)
              <input
                type="number"
                min={5}
                max={900}
                value={config.timeoutSecs}
                onChange={(e) => edit({ timeoutSecs: Number(e.target.value) })}
              />
            </label>
            <label className="inline-field">
              Context limit (characters)
              <input
                type="number"
                min={1000}
                max={400000}
                value={config.maxContextChars}
                onChange={(e) => edit({ maxContextChars: Number(e.target.value) })}
              />
            </label>
          </div>
        </>
      )}
      <label className="field-stack">
        Also redact (regular expressions, one per line)
        <textarea
          className="mono"
          rows={2}
          value={patterns}
          placeholder="e.g. customer-\d+"
          onChange={(e) => setPatterns(e.target.value)}
        />
      </label>
      <div className="form-actions">
        <Button variant="primary" size="sm" onClick={() => void save()}>
          Save
        </Button>
        {config.mode !== "disabled" && (
          <Button size="sm" variant="ghost" disabled={testing} onClick={() => void test()}>
            {testing ? "Testing…" : "Test the model"}
          </Button>
        )}
      </div>
    </section>
  );
}
