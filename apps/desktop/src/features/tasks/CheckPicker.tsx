import { useEffect, useState } from "react";
import type { ActiveCheck } from "../../bindings/ActiveCheck";
import type { PassiveCheck } from "../../bindings/PassiveCheck";
import { api } from "../../services/ipc";

/** The group a check is listed under. */
export function groupOf(
  id: string,
): "Active audit" | "JavaScript" | "Across requests" | "Single message" {
  if (id.startsWith("active-")) return "Active audit";
  if (id.startsWith("js-")) return "JavaScript";
  if (id.startsWith("cross-")) return "Across requests";
  return "Single message";
}

/** Turns a check on (removes it from the disabled list) or off. */
export function toggleCheck(disabled: string[], id: string, on: boolean): string[] {
  const rest = disabled.filter((d) => d !== id);
  return on ? rest : [...rest, id];
}

const GROUPS = ["Single message", "JavaScript", "Across requests", "Active audit"] as const;

/** Per-check selection (M26 / M13): passive and active checks with a box, grouped. */
export function CheckPicker({
  disabled,
  includeActive = false,
  onChange,
}: {
  disabled: string[];
  includeActive?: boolean;
  onChange(disabled: string[]): void;
}) {
  const [passive, setPassive] = useState<PassiveCheck[]>([]);
  const [active, setActive] = useState<ActiveCheck[]>([]);
  useEffect(() => {
    let live = true;
    api
      .passiveChecks()
      .then((c) => live && setPassive(c))
      .catch(() => live && setPassive([]));
    if (includeActive) {
      api
        .activeChecks()
        .then((c) => live && setActive(c))
        .catch(() => live && setActive([]));
    }
    return () => {
      live = false;
    };
  }, [includeActive]);
  const checks: (PassiveCheck | ActiveCheck)[] = includeActive ? [...passive, ...active] : passive;
  if (checks.length === 0) return null;
  const on = checks.filter((c) => !disabled.includes(c.id)).length;
  return (
    <details className="check-picker">
      <summary>
        Checks: {on} of {checks.length} on
      </summary>
      <div className="dialog-row">
        <button type="button" className="link-button" onClick={() => onChange([])}>
          All
        </button>
        <button
          type="button"
          className="link-button"
          onClick={() => onChange(checks.map((c) => c.id))}
        >
          None
        </button>
      </div>
      {GROUPS.map((group) => {
        const list = checks.filter((c) => groupOf(c.id) === group);
        if (list.length === 0) return null;
        return (
          <fieldset key={group}>
            <legend>{group}</legend>
            {list.map((c) => (
              <label key={c.id} className="check-picker-item" title={c.summary}>
                <input
                  type="checkbox"
                  checked={!disabled.includes(c.id)}
                  onChange={(e) => onChange(toggleCheck(disabled, c.id, e.target.checked))}
                />
                <span>{c.kind}</span>
                <span className="muted">{c.severity}</span>
              </label>
            ))}
          </fieldset>
        );
      })}
    </details>
  );
}
