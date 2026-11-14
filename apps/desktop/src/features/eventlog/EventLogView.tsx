import { Fragment, useEffect, useMemo, useState } from "react";
import type { LogCategory } from "../../bindings/LogCategory";
import type { NoticeLevel } from "../../bindings/NoticeLevel";
import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { formatDateTime } from "../../lib/format";
import { toggle } from "../../lib/filters";
import { useEventLogStore } from "../../state/eventLog";

const LEVELS: NoticeLevel[] = ["error", "warning", "info"];
const CATEGORIES: LogCategory[] = ["listener", "project", "scope", "tls", "notice", "system"];

export function EventLogView() {
  const entries = useEventLogStore((s) => s.entries);
  const loaded = useEventLogStore((s) => s.loaded);
  const error = useEventLogStore((s) => s.error);
  const { setActive, load, clear } = useEventLogStore.getState();
  const [levels, setLevels] = useState<NoticeLevel[]>([]);
  const [categories, setCategories] = useState<LogCategory[]>([]);
  const [text, setText] = useState("");

  useEffect(() => {
    setActive(true);
    return () => setActive(false);
  }, [setActive]);

  const shown = useMemo(() => {
    const needle = text.trim().toLowerCase();
    return entries
      .filter(
        (e) =>
          (levels.length === 0 || levels.includes(e.level)) &&
          (categories.length === 0 || categories.includes(e.category)) &&
          (needle === "" || e.message.toLowerCase().includes(needle)),
      )
      .reverse();
  }, [entries, levels, categories, text]);

  return (
    <div className="history-view">
      <div className="filters" role="search">
        <label className="filter-search">
          <Icon name="search" size={14} />
          <input
            type="search"
            placeholder="Filter messages…"
            aria-label="Filter messages"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
        <div className="chip-group" role="group" aria-label="Levels">
          {LEVELS.map((l) => (
            <button
              key={l}
              type="button"
              className={`chip chip-level-${l}`}
              aria-pressed={levels.includes(l)}
              onClick={() => setLevels(toggle(levels, l))}
            >
              {l}
            </button>
          ))}
        </div>
        <div className="chip-group" role="group" aria-label="Categories">
          {CATEGORIES.map((c) => (
            <button
              key={c}
              type="button"
              className="chip"
              aria-pressed={categories.includes(c)}
              onClick={() => setCategories(toggle(categories, c))}
            >
              {c}
            </button>
          ))}
        </div>
        <span className="spacer" />
        <span className="muted">
          {shown.length} of {entries.length}
        </span>
        <Button size="sm" variant="ghost" icon="restart" onClick={() => void load()}>
          Refresh
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="trash"
          disabled={entries.length === 0}
          onClick={() => void clear()}
        >
          Clear
        </Button>
      </div>
      {error ? (
        <ErrorState title="Could not read the event log" message={error} onRetry={load} />
      ) : !loaded ? (
        <LoadingState />
      ) : shown.length === 0 ? (
        <EmptyState title={entries.length === 0 ? "No events yet" : "No events match"}>
          Listener changes, projects, scope changes, certificate events, warnings and errors are
          recorded here and kept across restarts (the last 2,000).
        </EmptyState>
      ) : (
        <ol className="event-log" aria-label="Event log">
          {shown.map((e, i) => (
            <Fragment key={e.id}>
              {!e.earlier && shown[i - 1]?.earlier === true && (
                <li className="event-divider" role="separator">
                  This session
                </li>
              )}
              <li
                className={`event-entry event-${e.level}${e.earlier ? " event-earlier" : ""}`}
                title={e.earlier ? "Recorded in an earlier session" : undefined}
              >
                <span className="muted">{formatDateTime(e.at)}</span>
                <span className={`event-level event-level-${e.level}`}>{e.level}</span>
                <span className="muted">{e.category}</span>
                <span className="selectable">{e.message}</span>
              </li>
            </Fragment>
          ))}
        </ol>
      )}
    </div>
  );
}
