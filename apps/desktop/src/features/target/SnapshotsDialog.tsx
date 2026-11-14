import { useEffect, useState } from "react";
import type { SiteDiffEntry } from "../../bindings/SiteDiffEntry";
import type { SiteSnapshotInfo } from "../../bindings/SiteSnapshotInfo";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/States";
import { formatDateTime } from "../../lib/format";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

const TONE = { added: "ok", removed: "danger", changed: "warn" } as const;

/** Save the site map as a snapshot and compare snapshots (TARGET-005). */
export function SnapshotsDialog({ onClose }: { onClose(): void }) {
  const [list, setList] = useState<SiteSnapshotInfo[] | null>(null);
  const [name, setName] = useState("");
  const [older, setOlder] = useState<number | null>(null);
  const [newer, setNewer] = useState<number | null>(null);
  const [diff, setDiff] = useState<SiteDiffEntry[] | null>(null);
  const notify = (level: "info" | "error", m: string) => useAppStore.getState().notify(level, m);
  const reload = () =>
    void api
      .snapshots()
      .then((l) => {
        setList(l);
        setOlder((o) => o ?? l[0]?.id ?? null);
      })
      .catch((e: unknown) => notify("error", toIpcError(e).message));
  useEffect(reload, []);

  async function save() {
    try {
      const s = await api.snapshotSave(name);
      notify("info", `Saved “${s.name}” (${s.endpoints} endpoints)`);
      setName("");
      reload();
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }

  async function compare() {
    if (older === null) return;
    try {
      setDiff(await api.snapshotCompare(older, newer));
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog engage-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Site map snapshots"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Site map snapshots</strong>
          <Button size="sm" variant="ghost" onClick={onClose}>
            Close
          </Button>
        </header>
        <form
          className="dialog-row"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <input
            aria-label="Snapshot name"
            placeholder="Name, e.g. before the release"
            maxLength={200}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <Button type="submit" variant="primary" icon="plus" disabled={name.trim() === ""}>
            Save current site map
          </Button>
        </form>
        {list === null ? null : list.length === 0 ? (
          <EmptyState title="No snapshots yet">Save one, then compare it later.</EmptyState>
        ) : (
          <div className="dialog-row">
            <label>
              Compare{" "}
              <select
                aria-label="Older snapshot"
                value={older ?? ""}
                onChange={(e) => setOlder(Number(e.target.value))}
              >
                {list.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({formatDateTime(s.createdAt)})
                  </option>
                ))}
              </select>
            </label>
            <label>
              with{" "}
              <select
                aria-label="Newer snapshot"
                value={newer ?? ""}
                onChange={(e) => setNewer(e.target.value === "" ? null : Number(e.target.value))}
              >
                <option value="">the current site map</option>
                {list.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
            <Button icon="compare" onClick={() => void compare()}>
              Compare
            </Button>
            <span className="spacer" />
            {older !== null && (
              <Button
                size="sm"
                variant="ghost"
                icon="trash"
                onClick={() =>
                  void api
                    .snapshotDelete(older)
                    .then(() => {
                      setOlder(null);
                      setDiff(null);
                      reload();
                    })
                    .catch((e: unknown) => notify("error", toIpcError(e).message))
                }
              >
                Delete selected
              </Button>
            )}
          </div>
        )}
        {diff !== null &&
          (diff.length === 0 ? (
            <p className="muted">No differences.</p>
          ) : (
            <div className="endpoint-table-wrap analyzer-table">
              <table className="endpoint-table organizer-table" aria-label="Differences">
                <thead>
                  <tr>
                    <th>Change</th>
                    <th>URL</th>
                    <th>Before</th>
                    <th>After</th>
                  </tr>
                </thead>
                <tbody>
                  {diff.map((d) => (
                    <tr key={d.url}>
                      <td>
                        <Badge tone={TONE[d.change]}>{d.change}</Badge>
                      </td>
                      <td className="mono selectable">{d.url}</td>
                      <td className="mono">
                        {d.beforeMethods.join(" ")} {d.beforeStatuses.join(" ")}
                      </td>
                      <td className="mono">
                        {d.afterMethods.join(" ")} {d.afterStatuses.join(" ")}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))}
      </div>
    </div>
  );
}
