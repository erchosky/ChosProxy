import { useState } from "react";
import { Button, IconButton } from "../../components/Button";
import {
  type Condition,
  type ConditionField,
  type ConditionOp,
  type Conditions,
  FIELDS,
  conditionProblem,
  conditionsScript,
  newCondition,
  opsFor,
} from "../../lib/conditions";
import { scriptId } from "../../lib/scriptTemplates";
import { useAppStore } from "../../state/app";
import { useScriptsStore } from "../../state/scripts";
import { useHistoryCtx } from "./historyContext";

/** Conditions on headers, bodies, status… for this list (run in the
 * ChosScripts sandbox as a filter script). */
export function ConditionBuilder({
  onClose,
  binding,
}: {
  onClose(): void;
  /** Where the conditions live when it is not a traffic list (the site map). */
  binding?: { applied: Conditions | null; set(conditions: Conditions | null): void };
}) {
  const useStore = useHistoryCtx();
  const listApplied = useStore((s) => s.conditions);
  const listSet = useStore((s) => s.setConditions);
  const applied = binding ? binding.applied : listApplied;
  const setConditions = binding ? binding.set : listSet;
  const [draft, setDraft] = useState<Conditions>(
    () => applied ?? { match: "all", items: [newCondition()] },
  );
  const problems = draft.items.map(conditionProblem);
  const invalid = problems.some((p) => p !== null);
  const set = (i: number, patch: Partial<Condition>) =>
    setDraft((d) => ({
      ...d,
      items: d.items.map((c, j) => {
        if (j !== i) return c;
        const next = { ...c, ...patch };
        // Keep the operator valid for the field.
        if (!opsFor(next.field).some((o) => o.id === next.op)) {
          next.op = opsFor(next.field)[0]?.id ?? "contains";
        }
        return next;
      }),
    }));

  async function saveAsScript() {
    const lib = useScriptsStore.getState().library;
    if (!lib) return;
    const name = "Conditions filter";
    const id = scriptId(
      name,
      lib.config.scripts.map((s) => s.id),
    );
    const ok = await useScriptsStore.getState().save({
      ...lib.config,
      scripts: [
        ...lib.config.scripts,
        {
          id,
          name,
          kind: "filter",
          enabled: true,
          description: "Made by the condition builder.",
          source: conditionsScript(draft),
        },
      ],
    });
    if (ok) useAppStore.getState().notify("info", `Saved as the filter script “${name}”`);
  }

  return (
    <section className="card condition-builder" aria-label="Conditions">
      <div className="condition-head">
        <strong>Show rows where</strong>
        <select
          aria-label="Match"
          value={draft.match}
          onChange={(e) => setDraft({ ...draft, match: e.target.value as "all" | "any" })}
        >
          <option value="all">all conditions hold</option>
          <option value="any">any condition holds</option>
        </select>
        <span className="spacer" />
        <IconButton icon="close" label="Close conditions" onClick={onClose} />
      </div>
      {draft.items.map((c, i) => {
        const field = FIELDS.find((f) => f.id === c.field);
        const needsValue = c.op !== "exists" && c.op !== "missing";
        return (
          <div key={i} className="condition-row">
            <select
              aria-label={`Field of condition ${i + 1}`}
              value={c.field}
              onChange={(e) => set(i, { field: e.target.value as ConditionField })}
            >
              {FIELDS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </select>
            {field?.header && (
              <input
                type="text"
                placeholder="Header name"
                aria-label={`Header of condition ${i + 1}`}
                value={c.name}
                onChange={(e) => set(i, { name: e.target.value })}
              />
            )}
            <select
              aria-label={`Operator of condition ${i + 1}`}
              value={c.op}
              onChange={(e) => set(i, { op: e.target.value as ConditionOp })}
            >
              {opsFor(c.field).map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                </option>
              ))}
            </select>
            {needsValue && (
              <input
                type="text"
                className="grow"
                spellCheck={false}
                aria-label={`Value of condition ${i + 1}`}
                value={c.value}
                onChange={(e) => set(i, { value: e.target.value })}
              />
            )}
            <IconButton
              icon="trash"
              label={`Remove condition ${i + 1}`}
              onClick={() => setDraft((d) => ({ ...d, items: d.items.filter((_, j) => j !== i) }))}
            />
            {problems[i] && <span className="field-error">{problems[i]}</span>}
          </div>
        );
      })}
      <div className="form-actions">
        <Button
          size="sm"
          icon="plus"
          onClick={() => setDraft((d) => ({ ...d, items: [...d.items, newCondition()] }))}
        >
          Add condition
        </Button>
        <span className="spacer" />
        <Button
          size="sm"
          variant="ghost"
          disabled={invalid || draft.items.length === 0}
          onClick={() => void saveAsScript()}
        >
          Save as filter script
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={!applied}
          onClick={() => {
            setConditions(null);
            onClose();
          }}
        >
          Clear
        </Button>
        <Button size="sm" variant="primary" disabled={invalid} onClick={() => setConditions(draft)}>
          Apply
        </Button>
      </div>
    </section>
  );
}
