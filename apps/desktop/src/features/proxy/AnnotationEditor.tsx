import { useState } from "react";
import type { Highlight } from "../../bindings/Highlight";
import type { HistoryRow } from "../../bindings/HistoryRow";
import { Button } from "../../components/Button";
import { HIGHLIGHTS } from "../../lib/filters";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { allHistoryStores } from "../../state/history";
import { useHistoryCtx } from "./historyContext";

/** Highlight colour and notes of one exchange. */
export function AnnotationEditor({ row }: { row: HistoryRow }) {
  // Re-mount per exchange so the draft never leaks between rows.
  return <Editor key={`${row.id}:${row.highlight}:${row.notes}`} row={row} />;
}

function Editor({ row }: { row: HistoryRow }) {
  const notify = useAppStore((s) => s.notify);
  const useStore = useHistoryCtx();
  const [notes, setNotes] = useState(row.notes ?? "");
  const [saving, setSaving] = useState(false);

  async function save(highlight: Highlight | null, text: string) {
    setSaving(true);
    try {
      await api.annotateExchange(row.id, text.trim() === "" ? null : text, highlight);
      for (const store of allHistoryStores) store.getState().invalidate();
      useStore.getState().refreshDetail();
    } catch (e) {
      notify("error", `Could not save the annotation: ${toIpcError(e).message}`);
    } finally {
      setSaving(false);
    }
  }

  const dirty = notes.trim() !== (row.notes ?? "");
  return (
    <div className="annotation">
      <div className="highlight-picker" role="group" aria-label="Highlight">
        {HIGHLIGHTS.map((h) => (
          <button
            key={h}
            type="button"
            className={`swatch hl-${h}`}
            aria-label={`Highlight ${h}`}
            aria-pressed={row.highlight === h}
            title={h}
            disabled={saving}
            onClick={() => void save(row.highlight === h ? null : h, row.notes ?? "")}
          />
        ))}
      </div>
      <input
        className="grow"
        aria-label="Notes"
        placeholder="Add a note…"
        maxLength={10000}
        value={notes}
        onChange={(e) => setNotes(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && dirty) void save(row.highlight, notes);
        }}
      />
      {dirty && (
        <Button size="sm" disabled={saving} onClick={() => void save(row.highlight, notes)}>
          Save note
        </Button>
      )}
    </div>
  );
}
