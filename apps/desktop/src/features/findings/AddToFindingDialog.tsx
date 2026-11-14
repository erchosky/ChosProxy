import { useEffect, useState } from "react";
import type { Confidence } from "../../bindings/Confidence";
import type { FindingRow } from "../../bindings/FindingRow";
import type { FindingSeverity } from "../../bindings/FindingSeverity";
import { Button } from "../../components/Button";
import { Segmented } from "../../components/Tabs";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useFindingsStore } from "../../state/findings";
import { CONFIDENCES, SEVERITIES } from "./labels";

/** Documents the chosen request as a new finding or as more evidence. */
export function AddToFindingDialog() {
  const adding = useFindingsStore((s) => s.adding);
  if (!adding) return null;
  return <Dialog key={adding.exchangeId} exchangeId={adding.exchangeId} label={adding.label} />;
}

function Dialog({ exchangeId, label }: { exchangeId: number; label: string }) {
  const close = useFindingsStore((s) => s.closeAdd);
  const notify = useAppStore((s) => s.notify);
  const [mode, setMode] = useState<"new" | "existing">("new");
  const [existing, setExisting] = useState<FindingRow[]>([]);
  const [target, setTarget] = useState<number | null>(null);
  const [title, setTitle] = useState("");
  const [kind, setKind] = useState("");
  const [severity, setSeverity] = useState<FindingSeverity>("low");
  const [confidence, setConfidence] = useState<Confidence>("tentative");
  const [description, setDescription] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api
      .findings({ search: null, severities: [], statuses: [], inScopeOnly: false, source: null })
      .then((rows) => {
        setExisting(rows);
        setTarget(rows[0]?.id ?? null);
      })
      .catch(() => setExisting([]));
  }, []);

  async function save() {
    setSaving(true);
    setError(null);
    try {
      if (mode === "new") {
        const id = await api.findingCreate({
          exchangeId,
          kind,
          title,
          severity,
          confidence,
          description,
          remediation: "",
          evidenceNote: note,
        });
        notify("info", `Finding #${id} created`);
      } else if (target !== null) {
        await api.findingAddEvidence(target, exchangeId, note);
        notify("info", `Added as evidence to finding #${target}`);
      }
      close();
      void useFindingsStore.getState().load();
      void useAppStore.getState().refreshStats();
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setSaving(false);
    }
  }

  const valid = mode === "new" ? title.trim() !== "" && kind.trim() !== "" : target !== null;
  return (
    <div className="palette-backdrop" role="presentation" onClick={close}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Add to finding"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        <header className="dialog-head">
          <strong>Document {label}</strong>
          <Segmented<"new" | "existing">
            label="Finding"
            value={mode}
            onChange={setMode}
            items={[
              { id: "new", label: "New finding" },
              { id: "existing", label: "Add to existing", disabled: existing.length === 0 },
            ]}
          />
        </header>
        {mode === "new" ? (
          <div className="dialog-form">
            <input
              aria-label="Title"
              placeholder="Title, e.g. Admin page reachable without login"
              maxLength={300}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
            <input
              aria-label="Type"
              placeholder="Type, e.g. Access control"
              maxLength={300}
              value={kind}
              onChange={(e) => setKind(e.target.value)}
            />
            <div className="dialog-row">
              <label>
                Severity{" "}
                <select
                  value={severity}
                  onChange={(e) => setSeverity(e.target.value as FindingSeverity)}
                >
                  {SEVERITIES.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Confidence{" "}
                <select
                  value={confidence}
                  onChange={(e) => setConfidence(e.target.value as Confidence)}
                >
                  {CONFIDENCES.map((c) => (
                    <option key={c} value={c}>
                      {c}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <textarea
              aria-label="Description"
              placeholder="What is the problem? (can be completed later)"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
        ) : (
          <div className="dialog-form">
            <select
              aria-label="Finding"
              value={target ?? ""}
              onChange={(e) => setTarget(Number(e.target.value))}
            >
              {existing.map((f) => (
                <option key={f.id} value={f.id}>
                  #{f.id} [{f.severity}] {f.title} — {f.host}
                </option>
              ))}
            </select>
          </div>
        )}
        <div className="dialog-form">
          <input
            aria-label="Evidence note"
            placeholder="What does this request show? (optional)"
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
        </div>
        {error && <p className="field-error">{error}</p>}
        <footer className="form-actions">
          <Button variant="primary" disabled={!valid || saving} onClick={() => void save()}>
            {mode === "new" ? "Create finding" : "Add evidence"}
          </Button>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
        </footer>
      </div>
    </div>
  );
}
