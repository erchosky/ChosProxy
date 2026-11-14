import { reformatBody } from "../../lib/httpText";
import { useAiStore } from "../../state/ai";
import { PROTOCOLS, requestProtocol, withProtocol } from "../../lib/protocol";
import { REDIRECT_POLICIES, asRedirectPolicy, requestUrl } from "../../lib/redirects";
import { copyAsEntries } from "../../lib/copyAs";
import { type KeyboardEvent, useEffect, useState } from "react";
import type { GroupSendMode } from "../../bindings/GroupSendMode";
import type { RepeaterGroup } from "../../bindings/RepeaterGroup";
import type { RepeaterSendRow } from "../../bindings/RepeaterSendRow";
import type { RepeaterTab } from "../../bindings/RepeaterTab";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { CodeView } from "../../components/message/CodeView";
import { InspectorPanel } from "../../components/message/InspectorPanel";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState, Spinner } from "../../components/States";
import { HIGHLIGHTS } from "../../lib/filters";
import { formatBytes, formatClock, formatDuration, statusTone } from "../../lib/format";
import { parseTarget } from "../../lib/repeaterTarget";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useComparerStore } from "../../state/comparer";
import { useFuzzerStore } from "../../state/fuzzer";
import {
  isMenuKey,
  type MenuEntry,
  openContextMenu,
  openMenuBelow,
  SEPARATOR,
} from "../../state/contextMenu";
import { type TabUi, useRepeaterStore } from "../../state/repeater";
import { usableScripts, useScriptsStore } from "../../state/scripts";
import { sendToDecoder } from "../../state/tools";
import { base64ToBytes, bytesToText } from "../../lib/bytes";
import { CookieJarDialog } from "./CookieJarDialog";

const MODE_LABEL: Record<GroupSendMode, string> = {
  sequence: "In sequence (separate connections)",
  single: "In sequence (single connection)",
  parallel: "In parallel (last-byte sync)",
};

const MODE_SHORT: Record<GroupSendMode, string> = {
  sequence: "sequence",
  single: "single connection",
  parallel: "parallel",
};

/** Drag data of a Repeater tab (its id). */
const TAB_DRAG_TYPE = "application/x-chosproxy-repeater-tab";

/** Context menu of a group: send mode, colour, rename, ungroup. */
function groupMenu(group: RepeaterGroup, onRename: () => void): MenuEntry[] {
  const { updateGroup, deleteGroup, sendGroup } = useRepeaterStore.getState();
  return [
    { heading: `Group “${group.name}”` },
    { label: "Send group", icon: "play", onSelect: () => void sendGroup(group.id) },
    SEPARATOR,
    { heading: "Send mode" },
    ...(Object.keys(MODE_LABEL) as GroupSendMode[]).map((mode) => ({
      label: MODE_LABEL[mode],
      checked: group.sendMode === mode,
      onSelect: () => void updateGroup(group.id, { sendMode: mode }),
    })),
    SEPARATOR,
    {
      label: group.collapsed ? "Expand group" : "Collapse group",
      onSelect: () => void updateGroup(group.id, { collapsed: !group.collapsed }),
    },
    { label: "Rename group…", onSelect: onRename },
    {
      label: "Next colour",
      onSelect: () => {
        const i = HIGHLIGHTS.indexOf(group.color);
        const color = HIGHLIGHTS[(i + 1) % HIGHLIGHTS.length] ?? group.color;
        void updateGroup(group.id, { color });
      },
    },
    {
      label: "Ungroup (keep the tabs)",
      icon: "close",
      onSelect: () => void deleteGroup(group.id),
    },
  ];
}

/** Context menu of a tab. */
function tabMenu(tab: RepeaterTab, groups: RepeaterGroup[], onRename: () => void): MenuEntry[] {
  const { duplicate, close, setGroup, createGroup, setColor } = useRepeaterStore.getState();
  const entries: MenuEntry[] = [
    { heading: `Tab “${tab.name}”` },
    { label: "Rename…", hint: "F2", onSelect: onRename },
    { label: "Duplicate", icon: "copy", onSelect: () => void duplicate(tab.id) },
    SEPARATOR,
    {
      label: "New group with this tab",
      icon: "layers",
      onSelect: () => void createGroup([tab.id]),
    },
  ];
  for (const g of groups) {
    if (g.id === tab.groupId) continue;
    entries.push({
      label: `Add to group “${g.name}”`,
      icon: "layers",
      onSelect: () => void setGroup(tab.id, g.id),
    });
  }
  if (tab.groupId !== null) {
    entries.push({
      label: "Remove from group",
      onSelect: () => void setGroup(tab.id, null),
    });
  }
  entries.push(SEPARATOR, { heading: "Highlight" });
  for (const color of HIGHLIGHTS) {
    entries.push({
      label: color[0]?.toUpperCase() + color.slice(1),
      checked: tab.color === color,
      onSelect: () => void setColor(tab.id, tab.color === color ? null : color),
    });
  }
  if (tab.color !== null) {
    entries.push({ label: "No highlight", onSelect: () => void setColor(tab.id, null) });
  }
  entries.push(SEPARATOR, {
    label: "Close tab",
    icon: "close",
    danger: true,
    onSelect: () => void close(tab.id),
  });
  return entries;
}

/** Repeater: edit a request, send it, inspect the response, repeat. */
export function RepeaterView() {
  const loaded = useRepeaterStore((s) => s.loaded);
  const error = useRepeaterStore((s) => s.error);
  const tabs = useRepeaterStore((s) => s.tabs);
  const activeId = useRepeaterStore((s) => s.activeId);
  const { load, newTab } = useRepeaterStore.getState();

  useEffect(() => {
    void load();
  }, [load]);

  // Opening the view shows the history of the selected tab.
  useEffect(() => {
    if (activeId !== null && useRepeaterStore.getState().ui[activeId]?.history === null) {
      void useRepeaterStore.getState().loadHistory(activeId);
    }
  }, [activeId]);

  if (!loaded) return <LoadingState />;
  if (error) {
    return (
      <ErrorState title="Could not load Repeater" message={error} onRetry={() => void load()} />
    );
  }
  return (
    <div className="repeater">
      <TabStrip />
      {activeId !== null && tabs.some((t) => t.id === activeId) ? (
        <TabPane key={activeId} id={activeId} />
      ) : (
        <EmptyState
          title="No Repeater tabs"
          action={
            <Button variant="primary" icon="plus" onClick={() => void newTab()}>
              New tab
            </Button>
          }
        >
          Send a request here from HTTP history, the Logger, Search, the Organizer or Intercept
          (“Repeater”), or start from an empty request. Edit it, send it with Ctrl/Cmd+Enter and
          compare the responses.
        </EmptyState>
      )}
    </div>
  );
}

function TabStrip() {
  const tabs = useRepeaterStore((s) => s.tabs);
  const groups = useRepeaterStore((s) => s.groups);
  const activeId = useRepeaterStore((s) => s.activeId);
  const ui = useRepeaterStore((s) => s.ui);
  const { select, newTab, close, move, edit, flush, updateGroup, dropOnTab, setGroup } =
    useRepeaterStore.getState();
  // The tab or group a dragged tab is over.
  const [dropOn, setDropOn] = useState<string | null>(null);
  const dragged = (e: React.DragEvent): number | null => {
    const id = Number(e.dataTransfer.getData(TAB_DRAG_TYPE));
    return Number.isInteger(id) && id > 0 ? id : null;
  };
  const allowDrop = (key: string) => (e: React.DragEvent) => {
    if (!e.dataTransfer.types.includes(TAB_DRAG_TYPE)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDropOn(key);
  };
  const [renaming, setRenaming] = useState<number | null>(null);
  const [renamingGroup, setRenamingGroup] = useState<number | null>(null);
  const groupOf = (tab: RepeaterTab | undefined) =>
    tab?.groupId == null ? undefined : groups.find((g) => g.id === tab.groupId);

  function onKeyDown(e: KeyboardEvent, index: number) {
    const id = tabs[index]?.id;
    if (id === undefined) return;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      const delta = e.key === "ArrowRight" ? 1 : -1;
      if (e.altKey) {
        void move(id, index + delta);
      } else {
        const next = tabs[(index + delta + tabs.length) % tabs.length];
        if (next) select(next.id);
      }
      e.preventDefault();
    } else if (e.key === "F2") {
      setRenaming(id);
      e.preventDefault();
    } else if (isMenuKey(e)) {
      const tab = tabs[index];
      if (tab)
        openMenuBelow(
          e.currentTarget,
          "Tab actions",
          tabMenu(tab, groups, () => setRenaming(id)),
        );
      e.preventDefault();
    }
  }

  return (
    <div className="repeater-tabs" role="tablist" aria-label="Repeater tabs">
      {tabs.map((tab, index) => {
        const state = ui[tab.id];
        const name = state?.draft.name ?? tab.name;
        const group = groupOf(tab);
        const startsGroup = group !== undefined && groupOf(tabs[index - 1]) !== group;
        // A collapsed group shows its chip and, when it is selected, the selected tab.
        const hidden = group?.collapsed === true && tab.id !== activeId;
        const latest = state?.latest ?? null;
        return (
          <div key={tab.id} className="repeater-tab-slot">
            {startsGroup && (
              <span
                className={dropOn === `g${group.id}` ? "group-drop drop-target" : "group-drop"}
                onDragOver={allowDrop(`g${group.id}`)}
                onDragLeave={() => setDropOn(null)}
                onDrop={(e) => {
                  e.preventDefault();
                  setDropOn(null);
                  const id = dragged(e);
                  if (id !== null) void setGroup(id, group.id);
                }}
              >
                <GroupChip
                  group={group}
                  renaming={renamingGroup === group.id}
                  onRename={() => setRenamingGroup(group.id)}
                  onRenamed={(value) => {
                    setRenamingGroup(null);
                    if (value && value !== group.name) void updateGroup(group.id, { name: value });
                  }}
                />
              </span>
            )}
            {!hidden && (
              <div
                className={`repeater-tab${
                  tab.color
                    ? ` in-group hl-${tab.color}`
                    : group
                      ? ` in-group hl-${group.color}`
                      : ""
                }${dropOn === `t${tab.id}` ? " drop-target" : ""}`}
                draggable={renaming !== tab.id}
                onDragStart={(e) => {
                  e.dataTransfer.setData(TAB_DRAG_TYPE, String(tab.id));
                  e.dataTransfer.effectAllowed = "move";
                }}
                onDragOver={allowDrop(`t${tab.id}`)}
                onDragLeave={() => setDropOn(null)}
                onDrop={(e) => {
                  e.preventDefault();
                  setDropOn(null);
                  const id = dragged(e);
                  if (id !== null) void dropOnTab(id, tab.id);
                }}
                role="tab"
                aria-selected={tab.id === activeId}
                tabIndex={tab.id === activeId ? 0 : -1}
                title={`${name} — double-click or F2 to rename, drag or Alt+←/→ to move`}
                onClick={() => select(tab.id)}
                onDoubleClick={() => setRenaming(tab.id)}
                onKeyDown={(e) => onKeyDown(e, index)}
                onContextMenu={(e) => {
                  select(tab.id);
                  openContextMenu(
                    e,
                    "Tab actions",
                    tabMenu(tab, groups, () => setRenaming(tab.id)),
                  );
                }}
              >
                {renaming === tab.id ? (
                  <input
                    className="repeater-tab-name"
                    aria-label="Tab name"
                    autoFocus
                    maxLength={100}
                    defaultValue={name}
                    onClick={(e) => e.stopPropagation()}
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      if (e.key === "Enter") e.currentTarget.blur();
                      if (e.key === "Escape") setRenaming(null);
                    }}
                    onBlur={(e) => {
                      const value = e.currentTarget.value.trim();
                      setRenaming(null);
                      if (value && value !== name) {
                        edit(tab.id, { name: value });
                        void flush(tab.id);
                      }
                    }}
                  />
                ) : (
                  <span className="repeater-tab-label">{name}</span>
                )}
                {state?.sending && <Spinner label="Sending" />}
                {!state?.sending && latest && (
                  <span
                    className={`repeater-tab-status status status-${statusTone(latest.status)}`}
                    title={latest.error ?? `Last send: ${latest.status ?? "no response"}`}
                  >
                    {latest.status ?? "!"}
                  </span>
                )}
                {state?.dirty && !state.sending && (
                  <span className="repeater-dirty" title="Unsaved edits (saved automatically)" />
                )}
                <button
                  type="button"
                  className="repeater-tab-close"
                  aria-label={`Close tab ${name}`}
                  title="Close tab (its sends stay in the Logger)"
                  onClick={(e) => {
                    e.stopPropagation();
                    void close(tab.id);
                  }}
                >
                  <Icon name="close" size={12} />
                </button>
              </div>
            )}
          </div>
        );
      })}
      <IconButton icon="plus" label="New tab" onClick={() => void newTab()} />
    </div>
  );
}

function GroupChip({
  group,
  renaming,
  onRename,
  onRenamed,
}: {
  group: RepeaterGroup;
  renaming: boolean;
  onRename(): void;
  onRenamed(value: string): void;
}) {
  const sending = useRepeaterStore((s) => s.groupSending[group.id] ?? false);
  const count = useRepeaterStore((s) => s.tabs.filter((t) => t.groupId === group.id).length);
  if (renaming) {
    return (
      <input
        className={`repeater-group-chip hl-${group.color}`}
        aria-label="Group name"
        autoFocus
        maxLength={100}
        defaultValue={group.name}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") onRenamed(group.name);
        }}
        onBlur={(e) => onRenamed(e.currentTarget.value.trim())}
      />
    );
  }
  const open = (el: Element) => openMenuBelow(el, "Group actions", groupMenu(group, onRename));
  const toggle = () =>
    void useRepeaterStore.getState().updateGroup(group.id, { collapsed: !group.collapsed });
  return (
    <>
      <button
        type="button"
        className="repeater-group-toggle"
        aria-expanded={!group.collapsed}
        aria-label={`${group.collapsed ? "Expand" : "Collapse"} group ${group.name}`}
        title={group.collapsed ? "Expand group" : "Collapse group"}
        onClick={toggle}
      >
        <Icon name={group.collapsed ? "chevronRight" : "chevronDown"} size={12} />
      </button>
      <button
        type="button"
        className={`repeater-group-chip hl-${group.color}`}
        title={`Group “${group.name}”: ${count} tab${count === 1 ? "" : "s"}, sent ${MODE_SHORT[group.sendMode]} — click for actions`}
        aria-haspopup="menu"
        onClick={(e) => open(e.currentTarget)}
        onDoubleClick={onRename}
        onContextMenu={(e) => openContextMenu(e, "Group actions", groupMenu(group, onRename))}
      >
        {sending ? <Spinner label="Sending group" /> : <Icon name="layers" size={12} />}
        {group.name}
        {group.collapsed && <span className="repeater-group-count">{count}</span>}
      </button>
    </>
  );
}

function TabPane({ id }: { id: number }) {
  const ui = useRepeaterStore((s) => s.ui[id]);
  const note = useRepeaterStore((s) => s.tabs.find((t) => t.id === id)?.note ?? "");
  const [showHistory, setShowHistory] = useState(false);
  const [showNote, setShowNote] = useState(false);
  if (!ui) return null;
  return (
    <div className="repeater-pane">
      <ControlBar id={id} ui={ui} showHistory={showHistory} onToggleHistory={setShowHistory} />
      <div className="repeater-note-bar">
        <Button
          size="sm"
          variant="ghost"
          icon="file"
          aria-expanded={showNote}
          onClick={() => setShowNote((v) => !v)}
        >
          {showNote ? "Hide note" : note ? "Note ✓" : "Add a note"}
        </Button>
        {!showNote && note && (
          <span className="muted repeater-note-preview">{note.split("\n")[0]}</span>
        )}
      </div>
      {showNote && <TabNote key={id} id={id} saved={note} />}
      <div className="repeater-body">
        <SplitPane
          direction="horizontal"
          first={<RequestEditor id={id} ui={ui} />}
          second={
            showHistory ? (
              <SplitPane
                direction="vertical"
                initial={0.6}
                first={<ResponsePane id={id} ui={ui} />}
                second={<HistoryList id={id} ui={ui} />}
              />
            ) : (
              <ResponsePane id={id} ui={ui} />
            )
          }
        />
      </div>
    </div>
  );
}

function ControlBar({
  id,
  ui,
  showHistory,
  onToggleHistory,
}: {
  id: number;
  ui: TabUi;
  showHistory: boolean;
  onToggleHistory(show: boolean): void;
}) {
  const { send, cancel, edit, flush, duplicate, show, sendGroup, cancelGroup } =
    useRepeaterStore.getState();
  const actionScripts = usableScripts(
    useScriptsStore((s) => s.library),
    "action",
  );
  const shownExchange = ui.shown.status === "ready" ? ui.shown.send.exchangeId : null;
  const redirectPolicy = useRepeaterStore((s) =>
    asRedirectPolicy(s.tabs.find((t) => t.id === id)?.redirectPolicy),
  );
  const group = useRepeaterStore((s) => {
    const groupId = s.tabs.find((t) => t.id === id)?.groupId;
    return groupId == null ? undefined : s.groups.find((g) => g.id === groupId);
  });
  const groupSending = useRepeaterStore((s) =>
    group ? (s.groupSending[group.id] ?? false) : false,
  );
  const [jarOpen, setJarOpen] = useState(false);
  const target = parseTarget(ui.draft.targetText);
  const targetError = "error" in target ? target.error : null;
  const history = ui.history ?? [];
  const shownId = ui.shown.status === "ready" ? ui.shown.send.id : null;
  const index = shownId === null ? -1 : history.findIndex((h) => h.id === shownId);
  const older = index >= 0 ? history[index + 1] : undefined;
  const newer = index > 0 ? history[index - 1] : undefined;

  return (
    <div className="repeater-bar">
      {groupSending ? null : ui.sending ? (
        <Button variant="danger" icon="stop" onClick={() => void cancel(id)}>
          Cancel
        </Button>
      ) : (
        <Button
          variant="primary"
          icon="play"
          title="Send (Ctrl/Cmd+Enter in the editor)"
          disabled={targetError !== null}
          onClick={() => void send(id)}
        >
          Send
        </Button>
      )}
      {group &&
        (groupSending ? (
          <Button variant="danger" icon="stop" onClick={() => void cancelGroup(group.id)}>
            Cancel group
          </Button>
        ) : (
          <div className={`repeater-group-send hl-${group.color}`} role="group">
            <Button
              icon="layers"
              title={`Send every tab of “${group.name}” ${MODE_LABEL[group.sendMode].toLowerCase()}`}
              disabled={ui.sending}
              onClick={() => void sendGroup(group.id)}
            >
              Send group
            </Button>
            <IconButton
              icon="chevronDown"
              label={`Group options (now: ${MODE_SHORT[group.sendMode]})`}
              aria-haspopup="menu"
              onClick={(e) =>
                openMenuBelow(
                  e.currentTarget,
                  "Group actions",
                  groupMenu(group, () => {}).filter(
                    (entry) => !("label" in entry && entry.label === "Rename group…"),
                  ),
                )
              }
            />
          </div>
        ))}
      <div className="repeater-nav" role="group" aria-label="Previous sends">
        <IconButton
          icon="back"
          label="Show the previous send"
          disabled={!older}
          onClick={() => older && void show(id, older)}
        />
        <IconButton
          icon="forward"
          label="Show the next send"
          disabled={!newer}
          onClick={() => newer && void show(id, newer)}
        />
      </div>
      <label className="repeater-target">
        <span>Target</span>
        <input
          className="mono"
          value={ui.draft.targetText}
          spellCheck={false}
          aria-invalid={targetError !== null}
          aria-describedby={targetError ? `target-error-${id}` : undefined}
          placeholder="https://example.com"
          onChange={(e) => edit(id, { targetText: e.target.value })}
          onBlur={() => void flush(id)}
        />
      </label>
      {targetError && (
        <span id={`target-error-${id}`} className="field-error">
          {targetError}
        </span>
      )}
      {ui.problem && (
        <Badge tone="danger" title={ui.problem}>
          {ui.problem}
        </Badge>
      )}
      <span className="spacer" />
      {ui.draft.reuseConnection && (
        <Badge tone="neutral" title="The connection is kept open between sends">
          keep-alive
        </Badge>
      )}
      {ui.draft.useCookieJar && (
        <Badge tone="accent" title="Matching cookies from the cookie jar are added when sending">
          cookie jar
        </Badge>
      )}
      <Button
        size="sm"
        variant="ghost"
        icon="settings"
        aria-haspopup="menu"
        title="How this tab sends its request"
        onClick={(e) =>
          openMenuBelow(e.currentTarget, "Send options", [
            { heading: "Send options" },
            {
              label: "Update Content-Length",
              checked: ui.draft.updateContentLength,
              onSelect: () => edit(id, { updateContentLength: !ui.draft.updateContentLength }),
            },
            {
              label: "Reuse the connection (keep-alive)",
              checked: ui.draft.reuseConnection,
              onSelect: () => edit(id, { reuseConnection: !ui.draft.reuseConnection }),
            },
            {
              label: "Use the cookie jar",
              checked: ui.draft.useCookieJar,
              onSelect: () => edit(id, { useCookieJar: !ui.draft.useCookieJar }),
            },
            SEPARATOR,
            { heading: "Protocol" },
            ...PROTOCOLS.map((p) => ({
              label: p.label,
              checked: requestProtocol(ui.draft.request) === p.id,
              onSelect: () => edit(id, { request: withProtocol(ui.draft.request, p.id) }),
            })),
            SEPARATOR,
            { heading: "Follow redirects automatically" },
            ...REDIRECT_POLICIES.map((p) => ({
              label: p.label,
              checked: redirectPolicy === p.id,
              onSelect: () => void useRepeaterStore.getState().setRedirectPolicy(id, p.id),
            })),
            SEPARATOR,
            { label: "Open the cookie jar…", icon: "cookie", onSelect: () => setJarOpen(true) },
          ])
        }
      >
        Options
      </Button>
      <IconButton icon="cookie" label="Cookie jar" onClick={() => setJarOpen(true)} />
      {actionScripts.length > 0 && (
        <Button
          size="sm"
          variant="ghost"
          icon="code"
          disabled={shownExchange === null}
          title={
            shownExchange === null
              ? "Send the request first: action scripts run on a stored request"
              : "Run an action script on the request and response shown"
          }
          onClick={(e) =>
            shownExchange !== null &&
            openMenuBelow(e.currentTarget, "Run script", [
              { heading: "Run script" },
              ...actionScripts.map((a) => ({
                label: a.name,
                icon: "code" as const,
                onSelect: () =>
                  void useScriptsStore.getState().runAction(a.id, a.name, shownExchange),
              })),
            ])
          }
        >
          Run script
        </Button>
      )}
      <Button
        size="sm"
        variant={showHistory ? "secondary" : "ghost"}
        aria-pressed={showHistory}
        onClick={() => onToggleHistory(!showHistory)}
        title="Every send of this tab"
      >
        History{history.length > 0 ? ` (${history.length})` : ""}
      </Button>
      <IconButton icon="copy" label="Duplicate tab" onClick={() => void duplicate(id)} />
      {jarOpen && <CookieJarDialog onClose={() => setJarOpen(false)} />}
    </div>
  );
}

function RequestEditor({ id, ui }: { id: number; ui: TabUi }) {
  const { edit, send } = useRepeaterStore.getState();
  const [inspector, setInspector] = useState(false);
  const [selection, setSelection] = useState("");
  const request = ui.draft.request;
  const editor = (
    <CodeView
      text={request}
      language="http"
      wrap
      editable
      label="Request to send"
      onChange={(text) => edit(id, { request: text })}
      onSubmit={() => void send(id)}
      onSelection={setSelection}
      menu={() =>
        copyAsEntries(requestUrl(ui.draft.targetText.trim(), request), () =>
          new TextEncoder().encode(request.replace(/\r?\n/g, "\r\n")),
        )
      }
    />
  );
  return (
    <section className="message-viewer" aria-label="Request">
      <header className="message-toolbar">
        <strong>Request</strong>
        <span className="message-meta">
          {formatBytes(new TextEncoder().encode(request).length)}
        </span>
        {ui.dirty && <Badge tone="accent">edited</Badge>}
        <span className="spacer" />
        <Button
          size="sm"
          variant={inspector ? "secondary" : "ghost"}
          aria-pressed={inspector}
          onClick={() => setInspector((v) => !v)}
          title="Edit parameters, cookies and headers as a table"
        >
          Inspector
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title="Ask ChosAI about this request (preview first; secrets are redacted)"
          onClick={() =>
            useAiStore.getState().open({
              title: `Repeater ${ui.draft.name || "request"}`,
              attachments: [
                {
                  kind: "text",
                  label: `Repeater request to ${ui.draft.targetText.trim()}`,
                  text: request,
                },
              ],
              task: "explainRequest",
            })
          }
        >
          ChosAI
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title="Indent the JSON or XML body to edit it comfortably (Content-Length follows)"
          onClick={() => {
            const next = reformatBody(request, "pretty");
            if (next === null) {
              useAppStore.getState().notify("info", "The body is not JSON or XML");
            } else {
              edit(id, { request: next });
            }
          }}
        >
          Format body
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title="Remove the indentation of a JSON or XML body (Content-Length follows)"
          onClick={() => {
            const next = reformatBody(request, "compact");
            if (next !== null) edit(id, { request: next });
          }}
        >
          Compact
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title={
            selection ? "Open the selected text in the Decoder" : "Open the request in the Decoder"
          }
          onClick={() => sendToDecoder(selection || request)}
        >
          Decode{selection ? " selection" : ""}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="bolt"
          title="Open this request in the Fuzzer to send it with payloads"
          onClick={() => {
            const target = parseTarget(ui.draft.targetText);
            if ("error" in target) {
              useAppStore.getState().notify("error", `Fix the target first: ${target.error}`);
              return;
            }
            void useFuzzerStore.getState().openRequest(target, request);
          }}
        >
          Fuzz
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            useComparerStore.getState().add(`Repeater ${ui.draft.name} request`, request);
            useAppStore.getState().notify("info", "Sent to Comparer");
          }}
        >
          Compare
        </Button>
      </header>
      <div className="message-body">
        {inspector ? (
          <SplitPane
            direction="horizontal"
            initial={0.6}
            first={editor}
            second={
              <InspectorPanel text={request} onChange={(text) => edit(id, { request: text })} />
            }
          />
        ) : (
          editor
        )}
      </div>
    </section>
  );
}

function ResponsePane({ id, ui }: { id: number; ui: TabUi }) {
  const { followRedirect, restore } = useRepeaterStore.getState();
  const shown = ui.shown;
  if (ui.sending && shown.status !== "ready") {
    return <LoadingState label="Sending…" />;
  }
  if (shown.status === "idle") {
    return (
      <EmptyState title="No response yet">
        Press Send (or Ctrl/Cmd+Enter in the request) to send the request to the target.
      </EmptyState>
    );
  }
  if (shown.status === "loading") return <LoadingState />;
  if (shown.status === "error") {
    return <ErrorState title="Could not load the send" message={shown.message} />;
  }
  const { send, detail } = shown;
  const latest = ui.history?.[0]?.id === send.id;
  const status = send.status;
  const isRedirect = status !== null && [301, 302, 303, 307, 308].includes(status);
  return (
    <div className="exchange-panel">
      <div className="exchange-summary repeater-summary">
        {status !== null ? (
          <Badge tone={status >= 500 ? "danger" : status >= 400 ? "warn" : "ok"}>
            {status} {detail.reason}
          </Badge>
        ) : (
          <Badge tone="danger">no response</Badge>
        )}
        {send.error && (
          <Badge tone={status === null ? "danger" : "warn"} title={send.error}>
            {send.error}
          </Badge>
        )}
        <span className="muted">{formatBytes(send.responseLen)}</span>
        <TimingInfo send={send} />
        {!latest && (
          <>
            <Badge tone="neutral" title={`Sent ${formatClock(send.sentAt)}`}>
              earlier send · {formatClock(send.sentAt)}
            </Badge>
            <Button
              size="sm"
              variant="ghost"
              title="Put the request of this send back into the editor"
              onClick={() => void restore(id, send.exchangeId)}
            >
              Restore request
            </Button>
          </>
        )}
        <span className="spacer" />
        {isRedirect && (
          <Button
            size="sm"
            variant="secondary"
            title="Send the request a browser would send next"
            disabled={ui.sending}
            onClick={() => void followRedirect(id)}
          >
            Follow redirect
          </Button>
        )}
        <CompareWithPrevious ui={ui} send={send} />
      </div>
      <div className="exchange-split">
        <MessageViewer
          title={`Response #${detail.row.id}`}
          message={detail.response}
          compareLabel={`Repeater ${ui.draft.name} #${detail.row.id} response`}
          emptyText={send.error ? `No response: ${send.error}` : "No response received"}
        />
      </div>
    </div>
  );
}

function TimingInfo({ send }: { send: RepeaterSendRow }) {
  const t = send.timing;
  const parts = [
    ["DNS", t.dnsMs],
    ["connect", t.connectMs],
    ["TLS", t.tlsMs],
    ["server", t.waitMs],
    ["download", t.downloadMs],
  ] as const;
  const title = parts
    .filter(([, ms]) => ms > 0)
    .map(([label, ms]) => `${label} ${formatDuration(ms)}`)
    .join(" · ");
  return (
    <>
      <span className="muted" title={title || undefined}>
        {formatDuration(t.totalMs)}
      </span>
      {send.reused && (
        <Badge tone="neutral" title="Sent over a connection kept from an earlier request">
          reused connection
        </Badge>
      )}
      {send.jarCookies.length > 0 && (
        <Badge tone="accent" title={`From the cookie jar: ${send.jarCookies.join(", ")}`}>
          jar: {send.jarCookies.join(", ")}
        </Badge>
      )}
    </>
  );
}

/** Sends this response and the one of the send before it to the Comparer. */
function CompareWithPrevious({ ui, send }: { ui: TabUi; send: RepeaterSendRow }) {
  const history = ui.history ?? [];
  const index = history.findIndex((h) => h.id === send.id);
  const previous = index >= 0 ? history[index + 1] : undefined;
  if (!previous) return null;
  async function compare() {
    if (!previous) return;
    const { notify, setView } = useAppStore.getState();
    try {
      const [before, after] = await Promise.all([
        api.repeaterSnapshot(previous.exchangeId),
        api.repeaterSnapshot(send.exchangeId),
      ]);
      const text = (m: typeof after.detail.response) =>
        m ? bytesToText(base64ToBytes(m.rawBase64)) : "(no response)";
      const comparer = useComparerStore.getState();
      comparer.add(`#${before.detail.row.id} response (earlier)`, text(before.detail.response));
      comparer.add(`#${after.detail.row.id} response`, text(after.detail.response));
      setView("comparer");
    } catch (e) {
      notify("error", `Could not compare: ${toIpcError(e).message}`);
    }
  }
  return (
    <Button
      size="sm"
      variant="ghost"
      icon="compare"
      title="Compare this response with the previous send's response"
      onClick={() => void compare()}
    >
      Diff previous
    </Button>
  );
}

function HistoryList({ id, ui }: { id: number; ui: TabUi }) {
  const { show } = useRepeaterStore.getState();
  if (ui.history === null) return <LoadingState />;
  if (ui.history.length === 0) {
    return <EmptyState title="No sends yet">Each send of this tab is listed here.</EmptyState>;
  }
  const shownId = ui.shown.status === "ready" ? ui.shown.send.id : null;
  const list = ui.history;
  return (
    <div className="repeater-history" role="listbox" aria-label="Sends of this tab">
      {list.map((h, i) => (
        <button
          key={h.id}
          type="button"
          role="option"
          aria-selected={h.id === shownId}
          className="repeater-history-row"
          onClick={() => void show(id, h)}
          title={h.error ?? h.url}
        >
          <span className="muted mono">{list.length - i}</span>
          <span className="muted">{formatClock(h.sentAt)}</span>
          <strong className="mono">{h.method}</strong>
          <span className="mono repeater-history-url">{h.url}</span>
          {h.status !== null ? (
            <Badge tone={h.status >= 500 ? "danger" : h.status >= 400 ? "warn" : "ok"}>
              {h.status}
            </Badge>
          ) : (
            <Badge tone="danger">error</Badge>
          )}
          <span className="muted">{formatBytes(h.responseLen)}</span>
          <span className="muted">{formatDuration(h.timing.totalMs)}</span>
        </button>
      ))}
    </div>
  );
}

/** The tab's note, saved shortly after typing stops. */
function TabNote({ id, saved }: { id: number; saved: string }) {
  const [text, setText] = useState(saved);
  useEffect(() => {
    if (text === saved) return;
    const t = window.setTimeout(() => {
      api
        .repeaterSetNote(id, text)
        .then((tab) =>
          useRepeaterStore.setState((s) => ({ tabs: s.tabs.map((x) => (x.id === id ? tab : x)) })),
        )
        .catch((e: unknown) =>
          useAppStore
            .getState()
            .notify("error", `Could not save the note: ${toIpcError(e).message}`),
        );
    }, 600);
    return () => window.clearTimeout(t);
  }, [id, text, saved]);
  return (
    <textarea
      className="repeater-note"
      aria-label="Note on this tab"
      placeholder="What this request tests, what you found…"
      rows={3}
      maxLength={20000}
      value={text}
      onChange={(e) => setText(e.target.value)}
    />
  );
}
