import { useEffect, useState } from "react";
import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { Switch } from "../../components/Switch";
import {
  type HistoryFilter,
  MIME_FILTERS,
  clearedFilter,
  isFiltered,
  METHODS,
  siteLabel,
  SOURCES,
  STATUS_CLASSES,
  toggle,
} from "../../lib/filters";
import { useHistoryCtx } from "./historyContext";
import { useAppStore } from "../../state/app";
import { openMenuBelow, SEPARATOR } from "../../state/contextMenu";
import { usableScripts, useScriptsStore } from "../../state/scripts";
import { describeConditions } from "../../lib/conditions";
import { ConditionBuilder } from "./ConditionBuilder";

const SEARCH_DEBOUNCE_MS = 250;

export function HistoryFilters() {
  const useStore = useHistoryCtx();
  const filter = useStore((s) => s.filter);
  const setFilter = useStore((s) => s.setFilter);
  const total = useStore((s) => s.total);
  const fixedSources = useStore((s) => s.fixedSources);
  const follow = useStore((s) => s.follow);
  const setFollow = useStore((s) => s.setFollow);
  const [search, setSearch] = useState(filter.search);
  // Follow external changes (e.g. "Clear filters" elsewhere) so the debounce
  // below does not re-apply a stale search.
  const [appliedSearch, setAppliedSearch] = useState(filter.search);
  if (filter.search !== appliedSearch) {
    setAppliedSearch(filter.search);
    setSearch(filter.search);
  }

  useEffect(() => {
    if (search === filter.search) return;
    const t = window.setTimeout(() => setFilter({ search }), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [search, filter.search, setFilter]);

  return (
    <div className="filters" role="search">
      <label className="filter-search">
        <Icon name="search" size={14} />
        <input
          type="search"
          placeholder={filter.regex ? "URL regex…" : "Filter by URL…"}
          aria-label="Filter by URL"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </label>
      <Switch label="Regex" checked={filter.regex} onChange={(regex) => setFilter({ regex })} />
      {filter.site && (
        <span className="chip chip-active" title="Showing one site map node">
          {siteLabel(filter.site)}
          <button
            type="button"
            className="chip-clear"
            aria-label="Show every host"
            onClick={() => setFilter({ site: null })}
          >
            ×
          </button>
        </span>
      )}
      <div className="chip-group" role="group" aria-label="Methods">
        {METHODS.map((m) => (
          <button
            key={m}
            type="button"
            className="chip"
            aria-pressed={filter.methods.includes(m)}
            onClick={() => setFilter({ methods: toggle(filter.methods, m) })}
          >
            {m}
          </button>
        ))}
      </div>
      <div className="chip-group" role="group" aria-label="MIME type">
        {MIME_FILTERS.map((m) => (
          <button
            key={m}
            type="button"
            className="chip"
            aria-pressed={filter.mimes.includes(m)}
            onClick={() => setFilter({ mimes: toggle(filter.mimes, m) })}
          >
            {m}
          </button>
        ))}
      </div>
      <div className="chip-group" role="group" aria-label="Status">
        {STATUS_CLASSES.map((c) => (
          <button
            key={c.value}
            type="button"
            className={`chip chip-status-${c.value}`}
            aria-pressed={filter.statusClasses.includes(c.value)}
            onClick={() => setFilter({ statusClasses: toggle(filter.statusClasses, c.value) })}
          >
            {c.label}
          </button>
        ))}
      </div>
      <Switch
        label="Hide static assets"
        checked={filter.hideStatic}
        onChange={(hideStatic) => setFilter({ hideStatic })}
      />
      <Switch
        label="In scope only"
        checked={filter.inScopeOnly}
        onChange={(inScopeOnly) => setFilter({ inScopeOnly })}
      />
      <Switch
        label="Annotated"
        checked={filter.annotatedOnly}
        onChange={(annotatedOnly) => setFilter({ annotatedOnly })}
      />
      {fixedSources === null && (
        <select
          aria-label="Source tool"
          value={filter.sources[0] ?? ""}
          onChange={(e) =>
            setFilter({
              sources: e.target.value ? [e.target.value as (typeof SOURCES)[number]] : [],
            })
          }
        >
          <option value="">All tools</option>
          {SOURCES.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
      )}
      <SavedFilters />
      <ConditionsChip />
      <ScriptMenu />
      <span className="spacer" />
      <Switch label="Follow" checked={follow} onChange={setFollow} />
      <span className="muted">{total.toLocaleString()} shown</span>
      <Button
        size="sm"
        variant="ghost"
        icon={filter.newestFirst ? "arrowDown" : "arrowUp"}
        onClick={() =>
          setFilter(
            filter.sort === "id"
              ? { newestFirst: !filter.newestFirst }
              : { sort: "id", newestFirst: true },
          )
        }
        title={filter.sort === "id" ? "Sort order" : "Back to capture order"}
      >
        {filter.sort !== "id"
          ? `Sorted by ${filter.sort}`
          : filter.newestFirst
            ? "Newest first"
            : "Oldest first"}
      </Button>
      {isFiltered(filter) && (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            setSearch("");
            setFilter(clearedFilter(filter));
          }}
        >
          Clear filters
        </Button>
      )}
    </div>
  );
}

/** Named filters (kept in the settings, shared by HTTP history and the Logger). */
/** Opens the condition builder below the filters. */
function ConditionsChip() {
  const useStore = useHistoryCtx();
  const conditions = useStore((s) => s.conditions);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className={`chip${conditions ? " chip-active" : ""}`}
        aria-expanded={open}
        title={
          conditions ? describeConditions(conditions) : "Conditions on headers, bodies, status…"
        }
        onClick={() => setOpen((v) => !v)}
      >
        Conditions{conditions ? ` (${conditions.items.length})` : ""}
      </button>
      {open && (
        <div className="condition-popover">
          <ConditionBuilder onClose={() => setOpen(false)} />
        </div>
      )}
    </>
  );
}

/** Filter and column scripts (ChosScripts) for this list. */
function ScriptMenu() {
  const useStore = useHistoryCtx();
  const scriptFilter = useStore((s) => s.scriptFilter);
  const conditionsOn = useStore((s) => s.conditions !== null);
  const scriptColumn = useStore((s) => s.scriptColumn);
  const info = useStore((s) => s.scriptInfo);
  const { setScriptFilter, setScriptColumn } = useStore.getState();
  const library = useScriptsStore((s) => s.library);
  const load = useScriptsStore((s) => s.load);
  const goTo = useAppStore((s) => s.setView);
  useEffect(() => {
    if (!library) void load();
  }, [library, load]);
  const filters = usableScripts(library, "filter");
  const columns = usableScripts(library, "column");
  const name = (id: string | null) => library?.config.scripts.find((s) => s.id === id)?.name ?? id;
  const active = scriptFilter !== null || scriptColumn !== null;
  return (
    <>
      <button
        type="button"
        className={`chip${active ? " chip-active" : ""}`}
        aria-haspopup="menu"
        title="Filter and column scripts"
        onClick={(e) =>
          openMenuBelow(e.currentTarget, "Scripts", [
            { heading: "Show only rows the script keeps" },
            {
              label: "No filter script",
              checked: scriptFilter === null,
              onSelect: () => setScriptFilter(null),
            },
            ...filters.map((s) => ({
              label: s.name,
              checked: scriptFilter === s.id,
              onSelect: () => setScriptFilter(s.id),
            })),
            SEPARATOR,
            { heading: "Column computed by a script" },
            {
              label: "No script column",
              checked: scriptColumn === null,
              onSelect: () => setScriptColumn(null),
            },
            ...columns.map((s) => ({
              label: s.name,
              checked: scriptColumn === s.id,
              onSelect: () => setScriptColumn(s.id),
            })),
            SEPARATOR,
            { label: "Edit scripts…", icon: "code", onSelect: () => goTo("scripts") },
          ])
        }
      >
        <Icon name="code" size={13} /> {scriptFilter ? name(scriptFilter) : "Script"}
      </button>
      {(scriptFilter || conditionsOn) && info && (
        <span className="muted script-info" title={info.firstError ?? undefined}>
          {info.capped ? `newest ${info.scanned} rows checked` : `${info.scanned} rows checked`}
          {info.errors > 0 ? ` · script failed on ${info.errors}` : ""}
        </span>
      )}
    </>
  );
}

function SavedFilters() {
  const useStore = useHistoryCtx();
  const filter = useStore((s) => s.filter);
  const setFilter = useStore((s) => s.setFilter);
  const settings = useAppStore((s) => s.settings);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  if (!settings) return null;
  const saved = settings.ui.savedFilters;
  const store = (savedFilters: typeof saved) =>
    void useAppStore.getState().saveSettings({ ...settings, ui: { ...settings.ui, savedFilters } });

  if (naming)
    return (
      <form
        className="dialog-row"
        onSubmit={(e) => {
          e.preventDefault();
          const n = name.trim();
          if (!n) return;
          store([
            ...saved.filter((f) => f.name !== n),
            { name: n, filter: JSON.stringify({ ...filter, site: null }) },
          ]);
          setNaming(false);
          setName("");
        }}
      >
        <input
          autoFocus
          aria-label="Filter name"
          placeholder="Name"
          maxLength={100}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setNaming(false)}
        />
        <Button size="sm" type="submit" disabled={name.trim() === ""}>
          Save
        </Button>
      </form>
    );
  return (
    <Button
      size="sm"
      variant="ghost"
      icon="bookmark"
      aria-haspopup="menu"
      onClick={(e) =>
        openMenuBelow(e.currentTarget, "Saved filters", [
          { heading: saved.length > 0 ? "Apply" : "No saved filters yet" },
          ...saved.map((f) => ({
            label: f.name,
            onSelect: () => {
              try {
                setFilter({ ...(JSON.parse(f.filter) as Partial<HistoryFilter>), site: null });
              } catch {
                useAppStore.getState().notify("error", `The saved filter “${f.name}” is damaged`);
              }
            },
          })),
          SEPARATOR,
          { label: "Save the current filter…", icon: "plus", onSelect: () => setNaming(true) },
          ...(saved.length > 0 ? [{ heading: "Delete" } as const] : []),
          ...saved.map((f) => ({
            label: `Delete “${f.name}”`,
            icon: "trash" as const,
            onSelect: () => store(saved.filter((x) => x.name !== f.name)),
          })),
        ])
      }
    >
      Saved
    </Button>
  );
}
