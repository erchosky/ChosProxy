import { type KeyboardEvent, useEffect, useMemo, useState } from "react";
import { Icon } from "../../components/Icon";
import { fuzzyScore } from "../../lib/search";
import { useAppStore } from "../../state/app";
import { useInterceptStore } from "../../state/intercept";
import { useOastStore } from "../../state/oast";
import { useTasksStore } from "../../state/tasks";
import { buildCommands, type Command } from "./commands";
import type { HistoryRow } from "../../bindings/HistoryRow";
import { DEFAULT_FILTER, toFilterQuery } from "../../lib/filters";
import { api } from "../../services/ipc";
import { useHistoryStore, useLoggerStore } from "../../state/history";

export function CommandPalette() {
  const open = useAppStore((s) => s.paletteOpen);
  return open ? <PaletteDialog /> : null;
}

function PaletteDialog() {
  const setOpen = useAppStore((s) => s.setPaletteOpen);
  const project = useAppStore((s) => s.project);
  const listeners = useAppStore((s) => s.listeners);
  const settings = useAppStore((s) => s.settings);
  const browser = useAppStore((s) => s.browser);
  const ca = useAppStore((s) => s.ca);
  const interceptEnabled = useInterceptStore((s) => s.state.enabled);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  const commands = useMemo(() => {
    const s = useAppStore.getState();
    return buildCommands({
      project,
      listeners,
      theme: settings?.ui.theme ?? "dark",
      setView: s.setView,
      newTemporaryProject: () => void s.newTemporaryProject(),
      newEncryptedTemporaryProject: () => void s.newTemporaryProject(true),
      createProject: () => void s.createProject(),
      openProject: () => void s.openProject(),
      closeProject: () => void s.closeProject(),
      saveProjectAs: () => void s.saveProjectAs(),
      newCrawl: () => void useTasksStore.getState().openCrawl(),
      newOastPayload: () => void useOastStore.getState().newPayload("", "url"),
      startListener: (id) => void s.startListener(id),
      stopListener: (id) => void s.stopListener(id),
      setTheme: (theme) => {
        if (settings) void s.saveSettings({ ...settings, ui: { ...settings.ui, theme } });
      },
      browserRunning: browser?.running ?? false,
      openBrowser: () => void s.launchBrowser(null, null),
      closeBrowser: () => void s.closeBrowser(),
      caAvailable: ca?.available ?? false,
      exportCa: () => void s.exportCa("pem"),
      interceptEnabled,
      setIntercept: (enabled) => void useInterceptStore.getState().setEnabled(enabled),
      showShortcuts: () => s.setShortcutsOpen(true),
    });
  }, [project, listeners, settings, browser, ca, interceptEnabled]);

  const historyHits = useHistoryHits(project !== null ? query : "");
  const results = useMemo(
    () => [
      ...commands
        .map((c) => ({ c, score: fuzzyScore(query, `${c.group} ${c.title}`) }))
        .filter((r): r is { c: (typeof commands)[number]; score: number } => r.score !== null)
        .sort((a, b) => b.score - a.score)
        .map((r) => r.c),
      ...historyHits,
    ],
    [commands, query, historyHits],
  );

  function runAt(i: number) {
    const cmd = results[i];
    if (!cmd) return;
    setOpen(false);
    cmd.run();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") setOpen(false);
    else if (e.key === "ArrowDown") setActive((a) => Math.min(results.length - 1, a + 1));
    else if (e.key === "ArrowUp") setActive((a) => Math.max(0, a - 1));
    else if (e.key === "Enter") runAt(active);
    else return;
    e.preventDefault();
  }

  return (
    <div className="palette-backdrop" onMouseDown={() => setOpen(false)}>
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <label className="palette-input">
          <Icon name="search" />
          <input
            autoFocus
            type="text"
            placeholder="Type a command, or part of a URL to find a request…"
            aria-label="Command"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
          />
        </label>
        <ul className="palette-list" role="listbox">
          {results.length === 0 && <li className="palette-empty">No matching commands</li>}
          {results.map((c, i) => (
            <li
              key={c.id}
              role="option"
              aria-selected={i === active}
              className="palette-item"
              onMouseEnter={() => setActive(i)}
              onClick={() => runAt(i)}
            >
              <span>{c.title}</span>
              <span className="muted">{c.group}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** Opens a stored request: proxy traffic in HTTP history, other tools in the Logger. */
export function openStoredRequest(row: Pick<HistoryRow, "id" | "source">) {
  const app = useAppStore.getState();
  if (row.source === "proxy") {
    useHistoryStore.getState().select(row.id);
    app.setView("history");
  } else {
    useLoggerStore.getState().select(row.id);
    app.setView("logger");
  }
}

/** Stored requests whose URL contains `query` (3+ characters), newest first. */
function useHistoryHits(query: string): Command[] {
  const [hits, setHits] = useState<Command[]>([]);
  const text = query.trim();
  useEffect(() => {
    if (text.length < 3) return;
    let live = true;
    const t = window.setTimeout(() => {
      api
        .historyPage({ ...toFilterQuery(DEFAULT_FILTER), search: text, offset: 0, limit: 8 })
        .then((page) => {
          if (!live) return;
          setHits(
            page.rows.map((r) => ({
              id: `history-${r.id}`,
              title: `#${r.id} ${r.method} ${r.url}`,
              group: r.source === "proxy" ? "HTTP history" : `Logger (${r.source})`,
              run: () => openStoredRequest(r),
            })),
          );
        })
        .catch(() => live && setHits([]));
    }, 150);
    return () => {
      live = false;
      window.clearTimeout(t);
    };
  }, [text]);
  return text.length < 3 ? [] : hits;
}
