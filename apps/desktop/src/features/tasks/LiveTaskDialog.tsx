import { useState } from "react";
import type { LiveMode } from "../../bindings/LiveMode";
import type { LiveTaskConfig } from "../../bindings/LiveTaskConfig";
import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useTasksStore } from "../../state/tasks";
import { CheckPicker } from "./CheckPicker";

/** Traffic sources a live task can follow (as in `LIVE_SOURCES`). */
export const LIVE_SOURCES = [
  "proxy",
  "repeater",
  "fuzzer",
  "crawler",
  "scanner",
  "sequencer",
  "plugin",
  "discovery",
] as const;

export function defaultLiveConfig(mode: LiveMode): LiveTaskConfig {
  return {
    name: "",
    mode,
    sources: ["proxy"],
    inScopeOnly: true,
    urlContains: "",
    disabledChecks: [],
    includeExisting: false,
    resumeAfter: 0,
  };
}

/** Starts a live task (M26): a passive audit or passive crawl of the
 * traffic as it arrives. Live tasks never send requests. */
export function LiveTaskDialog({ onClose }: { onClose(): void }) {
  const [config, setConfig] = useState<LiveTaskConfig>(() => defaultLiveConfig("audit"));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const edit = (patch: Partial<LiveTaskConfig>) => setConfig({ ...config, ...patch });

  async function start() {
    setBusy(true);
    setError(null);
    try {
      const task = await api.liveTaskStart(config);
      useAppStore.getState().notify("info", `${task.name} started`);
      await useTasksStore.getState().refresh();
      useTasksStore.getState().select(task.id);
      onClose();
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-form"
        role="dialog"
        aria-modal="true"
        aria-label="New live task"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>New live task</strong>
        </header>
        <p className="muted">
          A live task follows the traffic as it is stored and keeps going until you stop it, even
          across closing and reopening the project. It only reads traffic: it never sends a request.
        </p>
        <div className="dialog-row" role="radiogroup" aria-label="What it does">
          <label className="inline-field">
            <input
              type="radio"
              checked={config.mode === "audit"}
              onChange={() => edit({ mode: "audit" })}
            />
            Live passive audit (findings from the passive checks)
          </label>
          <label className="inline-field">
            <input
              type="radio"
              checked={config.mode === "crawl"}
              onChange={() => edit({ mode: "crawl" })}
            />
            Live passive crawl (URLs linked to but not requested)
          </label>
        </div>
        <label className="field-stack">
          Name (optional)
          <input
            value={config.name}
            maxLength={80}
            placeholder={
              config.mode === "audit" ? "Live passive audit (proxy)" : "Live passive crawl (proxy)"
            }
            onChange={(e) => edit({ name: e.target.value })}
          />
        </label>
        <fieldset className="dialog-row">
          <legend>Traffic from</legend>
          {LIVE_SOURCES.map((s) => (
            <label key={s} className="inline-field">
              <input
                type="checkbox"
                checked={config.sources.includes(s)}
                onChange={(e) =>
                  edit({
                    sources: e.target.checked
                      ? [...config.sources, s]
                      : config.sources.filter((x) => x !== s),
                  })
                }
              />
              {s}
            </label>
          ))}
        </fieldset>
        <div className="dialog-row">
          <Switch
            checked={config.inScopeOnly}
            onChange={(inScopeOnly) => edit({ inScopeOnly })}
            label="In scope only"
          />
          <Switch
            checked={config.includeExisting}
            onChange={(includeExisting) => edit({ includeExisting })}
            label="Start with the traffic already stored"
          />
        </div>
        <label className="field-stack">
          Only URLs containing (optional)
          <input
            className="mono"
            value={config.urlContains}
            maxLength={500}
            placeholder="/api/"
            onChange={(e) => edit({ urlContains: e.target.value })}
          />
        </label>
        {config.mode === "audit" && (
          <CheckPicker
            disabled={config.disabledChecks}
            onChange={(disabledChecks) => edit({ disabledChecks })}
          />
        )}
        {error && <p className="callout callout-danger">{error}</p>}
        <div className="form-actions">
          <span className="spacer" />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon="bolt"
            disabled={busy || config.sources.length === 0}
            onClick={() => void start()}
          >
            Start
          </Button>
        </div>
      </div>
    </div>
  );
}
