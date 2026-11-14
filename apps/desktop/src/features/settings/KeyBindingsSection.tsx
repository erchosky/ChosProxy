import { useEffect, useState } from "react";
import type { AppSettings } from "../../bindings/AppSettings";
import type { KeyBinding } from "../../bindings/KeyBinding";
import { BINDABLE, comboOf, displayKeys, keysFor } from "../../app/shortcuts";
import { Button } from "../../components/Button";
import { useAppStore } from "../../state/app";

/** Keyboard shortcuts: change, clear or reset each one (saved immediately). */
export function KeyBindingsSection({ settings }: { settings: AppSettings }) {
  const save = useAppStore((s) => s.saveSettings);
  const overrides = settings.ui.keyBindings;
  const [recording, setRecording] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const store = (next: KeyBinding[]) =>
    void save({ ...settings, ui: { ...settings.ui, keyBindings: next } });
  const set = (action: string, keys: string | null) =>
    store([
      ...overrides.filter((b) => b.action !== action),
      ...(keys === null ? [] : [{ action, keys }]),
    ]);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(null);
        return;
      }
      const combo = comboOf(e);
      if (!combo) return; // a lone modifier: keep waiting
      const other = BINDABLE.find(
        (a) => a.id !== recording && keysFor(a, overrides).includes(combo),
      );
      setMessage(
        other
          ? `${displayKeys(combo)} was also used by “${other.label}”, which no longer has it.`
          : null,
      );
      const next = overrides.filter((b) => b.action !== recording && b.action !== other?.id);
      next.push({ action: recording, keys: combo });
      if (other) next.push({ action: other.id, keys: "" });
      store(next);
      setRecording(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  return (
    <section className="card">
      <h2>Keyboard shortcuts</h2>
      <p className="muted">
        Choose “Change” and press the new keys (Esc to cancel). Shortcuts without Ctrl/Cmd do not
        work while typing in a field.
      </p>
      {message && <p className="callout">{message}</p>}
      <div className="endpoint-table-wrap keybindings">
        <table className="endpoint-table organizer-table">
          <tbody>
            {BINDABLE.map((a) => {
              const keys = keysFor(a, overrides);
              const custom = overrides.some((b) => b.action === a.id);
              return (
                <tr key={a.id}>
                  <td>
                    {a.label} <span className="muted">· {a.group}</span>
                  </td>
                  <td className="mono">
                    {recording === a.id ? (
                      <em>press keys…</em>
                    ) : keys.length > 0 ? (
                      keys.map((k) => <kbd key={k}>{displayKeys(k)}</kbd>)
                    ) : (
                      <span className="muted">none</span>
                    )}
                  </td>
                  <td className="keybinding-actions">
                    <Button size="sm" variant="ghost" onClick={() => setRecording(a.id)}>
                      Change
                    </Button>
                    {keys.length > 0 && (
                      <Button size="sm" variant="ghost" onClick={() => set(a.id, "")}>
                        Clear
                      </Button>
                    )}
                    {custom && (
                      <Button size="sm" variant="ghost" onClick={() => set(a.id, null)}>
                        Reset
                      </Button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}
