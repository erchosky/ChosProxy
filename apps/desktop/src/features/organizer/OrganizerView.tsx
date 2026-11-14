import { useEffect, useMemo, useState } from "react";
import type { OrganizerDetail } from "../../bindings/OrganizerDetail";
import type { OrganizerStatus } from "../../bindings/OrganizerStatus";
import type { Severity } from "../../bindings/Severity";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { collectionTree, inCollection } from "../../lib/collections";
import { toggle } from "../../lib/filters";
import { formatDateTime, statusTone } from "../../lib/format";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useOrganizerStore } from "../../state/organizer";
import { openContextMenu } from "../../state/contextMenu";
import { askPassword, withSealedFilePassword } from "../../state/password";
import { exchangeMenu, sendToRepeater } from "../../state/tools";

export const STATUSES: { value: OrganizerStatus; label: string }[] = [
  { value: "todo", label: "To do" },
  { value: "investigating", label: "Investigating" },
  { value: "interesting", label: "Interesting" },
  { value: "confirmed", label: "Confirmed" },
  { value: "falsePositive", label: "False positive" },
  { value: "done", label: "Done" },
];
export const SEVERITIES: Severity[] = ["none", "info", "low", "medium", "high", "critical"];
/** Drag data of an Organizer item (its id). */
const DRAG_TYPE = "application/x-chosproxy-organizer-item";
const statusLabel = (s: OrganizerStatus) => STATUSES.find((x) => x.value === s)?.label ?? s;

export function OrganizerView() {
  const load = useOrganizerStore((s) => s.load);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <div className="history-split organizer">
      <SplitPane direction="vertical" initial={0.4} first={<ItemTable />} second={<ItemDetail />} />
    </div>
  );
}

function ItemTable() {
  const items = useOrganizerStore((s) => s.items);
  const loaded = useOrganizerStore((s) => s.loaded);
  const error = useOrganizerStore((s) => s.error);
  const selectedId = useOrganizerStore((s) => s.selectedId);
  const { select, load } = useOrganizerStore.getState();
  const [statuses, setStatuses] = useState<OrganizerStatus[]>([]);
  const [collection, setCollection] = useState("");
  const tree = useMemo(() => collectionTree(items.map((i) => i.collection)), [items]);
  const collections = useMemo(() => tree.map((n) => n.path), [tree]);
  // The collection an item is being dragged over ("" = no collection).
  const [dropOn, setDropOn] = useState<string | null>(null);
  const drop = (target: string) => (e: React.DragEvent) => {
    e.preventDefault();
    setDropOn(null);
    const id = Number(e.dataTransfer.getData(DRAG_TYPE));
    if (!Number.isInteger(id) || id <= 0) return;
    void useOrganizerStore
      .getState()
      .moveTo(id, target)
      .then((err) => err && useAppStore.getState().notify("error", err));
  };
  const over = (target: string) => (e: React.DragEvent) => {
    if (!e.dataTransfer.types.includes(DRAG_TYPE)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDropOn(target);
  };
  const [text, setText] = useState("");
  const shown = useMemo(() => {
    const needle = text.trim().toLowerCase();
    return items.filter(
      (i) =>
        (statuses.length === 0 || statuses.includes(i.status)) &&
        inCollection(i.collection, collection) &&
        (needle === "" ||
          [i.name, i.url, i.notes, ...i.tags].some((f) => f.toLowerCase().includes(needle))),
    );
  }, [items, statuses, text, collection]);

  async function doExport() {
    const slug = collection
      ? collection.replace(/[^a-zA-Z0-9_-]+/g, "-").toLowerCase()
      : "organizer";
    const defaultName = `chosproxy-${slug}.chosorg`;
    const path = dialogs.chooseOrganizerDestination
      ? await dialogs.chooseOrganizerDestination(defaultName)
      : await dialogs.chooseJsonDestination(defaultName);
    if (!path) return;
    const answer = await askPassword({
      mode: "choose",
      title: collection ? `Export collection “${collection}”` : "Export Organizer items",
      message:
        "Exported items contain full HTTP requests and responses. Encrypting the export with a password is recommended before sharing.",
      encrypt: true,
    });
    if (!answer) return;
    try {
      const res = await api.organizerExport(
        path,
        collection || null,
        answer.encrypt ? answer.password : null,
      );
      useAppStore
        .getState()
        .notify(
          "info",
          `Exported ${res.items} item(s)${res.encrypted ? " (encrypted)" : ""} to ${path}`,
        );
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    }
  }

  async function doImport() {
    const path = dialogs.chooseOrganizerFile
      ? await dialogs.chooseOrganizerFile()
      : await dialogs.chooseJsonFile();
    if (!path) return;
    try {
      const res = await withSealedFilePassword(path, "Unlock Organizer export", (pw) =>
        api.organizerImport(path, pw),
      );
      if (!res) return;
      await load();
      useAppStore
        .getState()
        .notify(
          "info",
          `Imported ${res.items} item(s)${res.encrypted ? " (encrypted)" : ""} from ${path}`,
        );
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    }
  }

  if (error)
    return <ErrorState title="Could not load the Organizer" message={error} onRetry={load} />;
  if (!loaded) return <LoadingState />;
  return (
    <div className="organizer-list">
      <div className="filters" role="search">
        <label className="filter-search">
          <Icon name="search" size={14} />
          <input
            type="search"
            placeholder="Filter by name, URL, notes, tag…"
            aria-label="Filter Organizer items"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
        <div className="chip-group" role="group" aria-label="Status">
          {STATUSES.map((s) => (
            <button
              key={s.value}
              type="button"
              className="chip"
              aria-pressed={statuses.includes(s.value)}
              onClick={() => setStatuses(toggle(statuses, s.value))}
            >
              {s.label}
            </button>
          ))}
        </div>
        <datalist id="organizer-collections">
          {collections.map((c) => (
            <option key={c} value={c} />
          ))}
        </datalist>
        <span className="spacer" />
        <Button size="sm" disabled={items.length === 0} onClick={() => void doExport()}>
          {collection ? `Export “${collection}”…` : "Export…"}
        </Button>
        <Button size="sm" onClick={() => void doImport()}>
          Import…
        </Button>
        <span className="muted">
          {shown.length} of {items.length}
        </span>
      </div>
      {items.length === 0 ? (
        <EmptyState title="Nothing organized yet">
          Use “Organizer” on a request in HTTP history, the Logger or Search to keep it here with
          your notes, tags, status and severity.
        </EmptyState>
      ) : (
        <div className="organizer-body">
          {tree.length > 0 && (
            <ul className="collection-tree" aria-label="Collections">
              <li>
                <button
                  type="button"
                  className={collection === "" ? "collection selected" : "collection"}
                  onClick={() => setCollection("")}
                >
                  All items <span className="muted">{items.length}</span>
                </button>
              </li>
              <li>
                <button
                  type="button"
                  className={dropOn === "" ? "collection drop-target" : "collection"}
                  title="Drop an item here to take it out of its collection"
                  onDragOver={over("")}
                  onDragLeave={() => setDropOn(null)}
                  onDrop={drop("")}
                  onClick={() => setCollection("")}
                >
                  No collection{" "}
                  <span className="muted">{items.filter((i) => !i.collection).length}</span>
                </button>
              </li>
              {tree.map((n) => (
                <li key={n.path}>
                  <button
                    type="button"
                    className={[
                      "collection",
                      collection === n.path ? "selected" : "",
                      dropOn === n.path ? "drop-target" : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    style={{ paddingLeft: 8 + n.depth * 14 }}
                    title={`${n.path}: drop an item here to move it`}
                    onDragOver={over(n.path)}
                    onDragLeave={() => setDropOn(null)}
                    onDrop={drop(n.path)}
                    onClick={() => setCollection(n.path)}
                  >
                    {n.label} <span className="muted">{n.count}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="endpoint-table-wrap">
            <table className="endpoint-table organizer-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Status</th>
                  <th>Severity</th>
                  <th>Method</th>
                  <th>URL</th>
                  <th>Code</th>
                  <th>Collection</th>
                  <th>Tags</th>
                  <th>Updated</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((i) => (
                  <tr
                    key={i.id}
                    className={selectedId === i.id ? "selected" : undefined}
                    aria-selected={selectedId === i.id}
                    draggable
                    onDragStart={(e) => {
                      e.dataTransfer.setData(DRAG_TYPE, String(i.id));
                      e.dataTransfer.effectAllowed = "move";
                    }}
                    onClick={() => select(i.id)}
                    onContextMenu={(e) => {
                      const exchangeId = i.exchangeId;
                      if (exchangeId === null) return;
                      select(i.id);
                      openContextMenu(
                        e,
                        "Request actions",
                        exchangeMenu({ id: exchangeId, url: i.url }),
                      );
                    }}
                  >
                    <td>{i.name}</td>
                    <td>
                      <span className={`org-status org-${i.status}`}>{statusLabel(i.status)}</span>
                    </td>
                    <td>
                      <span className={`sev sev-${i.severity}`}>{i.severity}</span>
                    </td>
                    <td>
                      <span className={`method method-${i.method.toLowerCase()}`}>{i.method}</span>
                    </td>
                    <td className="mono" title={i.url}>
                      {i.url}
                    </td>
                    <td>
                      <span className={`status status-${statusTone(i.statusCode)}`}>
                        {i.statusCode ?? "—"}
                      </span>
                    </td>
                    <td>{i.collection}</td>
                    <td>{i.tags.join(", ")}</td>
                    <td className="muted">{formatDateTime(i.updatedAt)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

function ItemDetail() {
  const detail = useOrganizerStore((s) => s.detail);
  const select = useOrganizerStore((s) => s.select);
  if (detail.status === "idle") return <EmptyState title="Select an item" />;
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error") {
    return (
      <ErrorState
        title="Could not load the item"
        message={detail.message}
        onRetry={() => select(detail.id)}
      />
    );
  }
  return <ItemEditor key={`${detail.id}:${detail.detail.item.updatedAt}`} detail={detail.detail} />;
}

function ItemEditor({ detail }: { detail: OrganizerDetail }) {
  const { update, remove } = useOrganizerStore.getState();
  const { item } = detail;
  const [name, setName] = useState(item.name);
  const [status, setStatus] = useState(item.status);
  const [severity, setSeverity] = useState(item.severity);
  const [tags, setTags] = useState(item.tags.join(", "));
  const [collection, setCollection] = useState(item.collection);
  const [notes, setNotes] = useState(item.notes);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const tagList = tags
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
  const dirty =
    name !== item.name ||
    status !== item.status ||
    severity !== item.severity ||
    notes !== item.notes ||
    collection.trim() !== item.collection ||
    tagList.join(",") !== item.tags.join(",");

  async function save() {
    setSaving(true);
    setError(await update(item.id, { name, status, severity, tags: tagList, notes, collection }));
    setSaving(false);
  }

  async function del() {
    const ok = await dialogs.confirm(
      `Delete “${item.name}” from the Organizer? The request stays in the history.`,
      "Delete Organizer item?",
      "Delete",
    );
    if (ok) setError(await remove(item.id));
  }

  return (
    <div className="organizer-detail">
      <form
        className="organizer-form"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <input
          aria-label="Name"
          className="grow"
          value={name}
          maxLength={200}
          onChange={(e) => setName(e.target.value)}
        />
        <select
          aria-label="Status"
          value={status}
          onChange={(e) => setStatus(e.target.value as OrganizerStatus)}
        >
          {STATUSES.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </select>
        <select
          aria-label="Severity"
          value={severity}
          onChange={(e) => setSeverity(e.target.value as Severity)}
        >
          {SEVERITIES.map((s) => (
            <option key={s} value={s}>
              Severity: {s}
            </option>
          ))}
        </select>
        <input
          aria-label="Tags"
          placeholder="tags, comma separated"
          value={tags}
          onChange={(e) => setTags(e.target.value)}
        />
        <input
          aria-label="Collection"
          placeholder="collection (Parent/Child nests)"
          list="organizer-collections"
          maxLength={60}
          value={collection}
          onChange={(e) => setCollection(e.target.value)}
        />
        <Button type="submit" size="sm" variant="primary" disabled={!dirty || saving}>
          Save
        </Button>
        <Button size="sm" variant="ghost" icon="trash" onClick={() => void del()}>
          Delete
        </Button>
        {item.exchangeId !== null && (
          <Button
            size="sm"
            variant="ghost"
            icon="repeater"
            title="Open the request in a new Repeater tab"
            onClick={() => item.exchangeId !== null && sendToRepeater(item.exchangeId)}
          >
            Repeater
          </Button>
        )}
        <textarea
          aria-label="Notes"
          placeholder="Notes: what you saw, how to reproduce, next steps…"
          rows={3}
          maxLength={20000}
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
        />
        {error && <p className="field-error">{error}</p>}
        <span className="muted organizer-meta">
          Added {formatDateTime(item.createdAt)}
          {item.exchangeId !== null && ` from request #${item.exchangeId}`}
          {dirty && <Badge tone="accent">unsaved</Badge>}
        </span>
      </form>
      <div className="exchange-split">
        <SplitPane
          direction="horizontal"
          first={<MessageViewer title="Request" message={detail.request} emptyText="No request" />}
          second={
            <MessageViewer title="Response" message={detail.response} emptyText="No response" />
          }
        />
      </div>
    </div>
  );
}
