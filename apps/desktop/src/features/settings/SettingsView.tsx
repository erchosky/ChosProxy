import { KeyBindingsSection } from "./KeyBindingsSection";
import { AgentSection } from "./AgentSection";
import { AiSection } from "./AiSection";
import { StorageSection } from "./StorageSection";
import { AutomationSection } from "./AutomationSection";
import { KeyringArea } from "./CredentialsSection";
import { NetworkSection } from "./NetworkSection";
import { SecuritySection } from "./SecuritySection";
import { useEffect, useState } from "react";
import { SettingsTransferCard } from "./SettingsTransferCard";
import type { AppSettings } from "../../bindings/AppSettings";
import type { LogLevel } from "../../bindings/LogLevel";
import type { PassiveCheck } from "../../bindings/PassiveCheck";
import type { PassiveSettings } from "../../bindings/PassiveSettings";
import type { Theme } from "../../bindings/Theme";
import { Button } from "../../components/Button";
import { LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { Segmented } from "../../components/Tabs";
import { api } from "../../services/ipc";
import { useAppStore } from "../../state/app";

const MB = 1024 * 1024;
const MIN_MB = 1;
const MAX_MB = 256;

const THEMES = [
  { id: "dark", label: "Dark" },
  { id: "light", label: "Light" },
  { id: "system", label: "System" },
] as const satisfies readonly { id: Theme; label: string }[];

const LEVELS: LogLevel[] = ["error", "warn", "info", "debug", "trace"];

export function SettingsView() {
  const settings = useAppStore((s) => s.settings);
  const info = useAppStore((s) => s.info);
  if (!settings) return <LoadingState />;
  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1>Settings</h1>
          <p className="page-sub">Application settings apply to every project.</p>
        </div>
      </div>
      <SecuritySection />
      <AppearanceSection settings={settings} />
      <KeyBindingsSection settings={settings} />
      <PassiveSection settings={settings} />
      <NetworkSection />
      <KeyringArea />
      <AutomationSection />
      <AiSection />
      <AgentSection />
      {/* Re-keyed on the saved values it edits so the form resets after a save. */}
      <SettingsForm
        key={JSON.stringify([settings.capture, settings.logging])}
        settings={settings}
      />
      <SettingsTransferCard />
      <StorageSection />
      <section className="card">
        <h2>Locations</h2>
        <dl className="paths selectable">
          <dt>Configuration</dt>
          <dd className="mono">{info?.configDir}</dd>
          <dt>Projects (default)</dt>
          <dd className="mono">{info?.dataDir}</dd>
          <dt>Logs</dt>
          <dd className="mono">{info?.logDir}</dd>
          <dt>Version</dt>
          <dd className="mono">{info?.version}</dd>
        </dl>
        <p className="muted">
          Everything stays on this machine: no telemetry, no analytics, no cloud.
        </p>
      </section>
    </div>
  );
}

/** Theme applies immediately, independently of the form below. */
function AppearanceSection({ settings }: { settings: AppSettings }) {
  const save = useAppStore((s) => s.saveSettings);
  return (
    <section className="card">
      <h2>Appearance</h2>
      <div className="field">
        <div>
          <div className="field-label">Theme</div>
          <div className="field-help">Applied immediately.</div>
        </div>
        <Segmented<Theme>
          items={THEMES}
          value={settings.ui.theme}
          label="Theme"
          onChange={(theme) => void save({ ...settings, ui: { ...settings.ui, theme } })}
        />
      </div>
    </section>
  );
}

/** Passive analysis: applied immediately. */
function PassiveSection({ settings }: { settings: AppSettings }) {
  const save = useAppStore((s) => s.saveSettings);
  const [checks, setChecks] = useState<PassiveCheck[]>([]);
  useEffect(() => {
    api
      .passiveChecks()
      .then(setChecks)
      .catch(() => setChecks([]));
  }, []);
  const passive = settings.passive;
  const set = (patch: Partial<PassiveSettings>) =>
    void save({ ...settings, passive: { ...passive, ...patch } });
  const disabled = new Set(passive.disabled_checks);
  const setCheck = (id: string, on: boolean) =>
    set({
      disabled_checks: on
        ? passive.disabled_checks.filter((d) => d !== id)
        : [...passive.disabled_checks, id],
    });
  return (
    <section className="card">
      <h2>Passive analysis</h2>
      <p className="muted">
        Checks every request and response that goes through the proxy for common weaknesses and adds
        them to Findings, with an explanation of each problem. It only reads traffic that already
        happened: nothing is sent and nothing is changed.
      </p>
      <div className="field">
        <div>
          <div className="field-label">Analyze new traffic</div>
          <div className="field-help">Applied immediately; it resumes where it stopped.</div>
        </div>
        <Switch
          label="Analyze new traffic"
          checked={passive.enabled}
          onChange={(enabled) => set({ enabled })}
        />
      </div>
      <div className="field">
        <div>
          <div className="field-label">Only traffic in scope</div>
          <div className="field-help">
            Avoids findings about third-party sites. While no scope is defined, all traffic is
            analyzed.
          </div>
        </div>
        <Switch
          label="Only traffic in scope"
          checked={passive.in_scope_only}
          onChange={(in_scope_only) => set({ in_scope_only })}
        />
      </div>
      <div className="field">
        <div>
          <div className="field-label">Include Repeater traffic</div>
          <div className="field-help">Proxy, crawler and fuzzer traffic are always analyzed.</div>
        </div>
        <Switch
          label="Include Repeater traffic"
          checked={passive.repeater}
          onChange={(repeater) => set({ repeater })}
        />
      </div>
      {checks.length > 0 && (
        <ul className="passive-checks" aria-label="Passive checks">
          {checks.map((c) => (
            <li key={c.id}>
              <Switch
                label={c.kind}
                checked={!disabled.has(c.id)}
                onChange={(on) => setCheck(c.id, on)}
              />
              <span className={`sev sev-${c.severity}`}>up to {c.severity}</span>
              <span className="muted">{c.summary}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function SettingsForm({ settings }: { settings: AppSettings }) {
  const save = useAppStore((s) => s.saveSettings);
  const busy = useAppStore((s) => s.busy);
  const [level, setLevel] = useState<LogLevel>(settings.logging.level);
  const [captureMb, setCaptureMb] = useState(
    String(Math.round(settings.capture.max_message_bytes / MB)),
  );

  const mb = Number(captureMb);
  const captureError =
    !/^\d+$/.test(captureMb) || mb < MIN_MB || mb > MAX_MB ? `Enter ${MIN_MB}–${MAX_MB} MB` : null;
  const next: AppSettings = {
    ...settings,
    logging: { level },
    capture: captureError ? settings.capture : { max_message_bytes: mb * MB },
  };
  const dirty = JSON.stringify(next) !== JSON.stringify(settings) || captureError !== null;

  return (
    <>
      <section className="card">
        <h2>Capture</h2>
        <div className="field">
          <div>
            <div className="field-label">Maximum stored message size</div>
            <div className="field-help">
              Larger requests/responses are still forwarded in full; only the stored copy is cut and
              marked as truncated.
            </div>
          </div>
          <div>
            <input
              type="number"
              min={MIN_MB}
              max={MAX_MB}
              value={captureMb}
              aria-label="Maximum stored message size in MB"
              aria-invalid={captureError !== null}
              onChange={(e) => setCaptureMb(e.target.value)}
            />{" "}
            MB
            {captureError && <div className="field-error">{captureError}</div>}
          </div>
        </div>
      </section>

      <section className="card">
        <h2>Logging</h2>
        <div className="field">
          <div>
            <div className="field-label">Log level</div>
            <div className="field-help">
              Logs never include request/response contents, cookies or credentials.
            </div>
          </div>
          <select
            aria-label="Log level"
            value={level}
            onChange={(e) => setLevel(e.target.value as LogLevel)}
          >
            {LEVELS.map((l) => (
              <option key={l} value={l}>
                {l.toUpperCase()}
              </option>
            ))}
          </select>
        </div>
      </section>

      <div className="form-actions">
        <span className="spacer" />
        <Button
          variant="ghost"
          disabled={!dirty}
          onClick={() => {
            setLevel(settings.logging.level);
            setCaptureMb(String(Math.round(settings.capture.max_message_bytes / MB)));
          }}
        >
          Discard changes
        </Button>
        <Button
          variant="primary"
          disabled={!dirty || captureError !== null || busy !== null}
          onClick={() => void save(next)}
        >
          Save
        </Button>
      </div>
    </>
  );
}
