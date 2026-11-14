import { ConditionBuilder } from "../proxy/ConditionBuilder";
import { useEffect, useMemo, useState } from "react";
import type { SiteMapEntry } from "../../bindings/SiteMapEntry";
import type { PointKind } from "../../bindings/PointKind";
import type { SiteFilter } from "../../bindings/SiteFilter";
import type { SiteMark } from "../../bindings/SiteMark";
import type { SiteNodeDetails } from "../../bindings/SiteNodeDetails";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { Segmented } from "../../components/Tabs";
import { api, toIpcError } from "../../services/ipc";
import { exchangeMenu, sendToRepeater } from "../../state/tools";
import { siteLabel } from "../../lib/filters";
import { formatDateTime, statusTone } from "../../lib/format";
import { type SiteNode, entriesUnder, findNode } from "../../lib/siteTree";
import { useScopeStore } from "../../state/scope";
import { useSiteMapStore } from "../../state/siteMap";
import { isMenuKey, openContextMenu, openMenuBelow } from "../../state/contextMenu";
import { nodeMenu, showInLogger, siteOf } from "./siteMapMenu";
import { SnapshotsDialog } from "./SnapshotsDialog";

const MARKS: { value: SiteMark; label: string }[] = [
  { value: "interesting", label: "Interesting" },
  { value: "reviewed", label: "Reviewed" },
  { value: "ignore", label: "Ignore" },
  { value: "favorite", label: "Favorite" },
];
/** Endpoints listed at once in the detail table. */
const MAX_LISTED = 500;
const SEARCH_DEBOUNCE_MS = 250;

export function SiteMapView() {
  const load = useSiteMapStore((s) => s.load);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <div className="history-view">
      <SiteMapFilters />
      <div className="history-split">
        <SplitPane
          direction="horizontal"
          initial={0.38}
          first={<SiteTree />}
          second={<NodeDetail />}
        />
      </div>
    </div>
  );
}

function SiteMapFilters() {
  const filter = useSiteMapStore((s) => s.filter);
  const setFilter = useSiteMapStore((s) => s.setFilter);
  const map = useSiteMapStore((s) => s.map);
  const loading = useSiteMapStore((s) => s.loading);
  const load = useSiteMapStore((s) => s.load);
  const [search, setSearch] = useState(filter.search);
  const [snapshots, setSnapshots] = useState(false);
  const conditions = useSiteMapStore((s) => s.conditions);
  const setConditions = useSiteMapStore((s) => s.setConditions);
  const scriptInfo = useSiteMapStore((s) => s.scriptInfo);
  const [building, setBuilding] = useState(false);
  useEffect(() => {
    if (search === filter.search) return;
    const t = window.setTimeout(() => setFilter({ search }), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [search, filter.search, setFilter]);
  return (
    <>
      <div className="filters" role="search">
        <label className="filter-search">
          <Icon name="search" size={14} />
          <input
            type="search"
            placeholder="Filter by URL…"
            aria-label="Filter site map by URL"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <Switch
          label="In scope only"
          checked={filter.inScopeOnly}
          onChange={(inScopeOnly) => setFilter({ inScopeOnly })}
        />
        <Switch
          label="Hide static assets"
          checked={filter.hideStatic}
          onChange={(hideStatic) => setFilter({ hideStatic })}
        />
        <Button
          size="sm"
          variant={conditions ? "primary" : "ghost"}
          title="Show only the endpoints of requests that meet conditions on headers, bodies, status…"
          onClick={() => setBuilding((b) => !b)}
        >
          {conditions ? `Conditions (${conditions.items.length})` : "Conditions"}
        </Button>
        <span className="spacer" />
        {map && (
          <span className="muted">
            {map.entries.length.toLocaleString()} endpoints{map.truncated ? " (limit reached)" : ""}
            {scriptInfo &&
              ` · ${scriptInfo.scanned.toLocaleString()} requests looked at${
                scriptInfo.capped ? " (the newest only)" : ""
              }${scriptInfo.errors ? ` · ${scriptInfo.errors} failed: ${scriptInfo.firstError ?? ""}` : ""}`}
          </span>
        )}
        <Button size="sm" variant="ghost" icon="layers" onClick={() => setSnapshots(true)}>
          Snapshots
        </Button>
        {snapshots && <SnapshotsDialog onClose={() => setSnapshots(false)} />}
        <Button
          size="sm"
          variant="ghost"
          icon="restart"
          disabled={loading}
          onClick={() => void load()}
        >
          Refresh
        </Button>
      </div>
      {building && (
        <ConditionBuilder
          onClose={() => setBuilding(false)}
          binding={{ applied: conditions, set: setConditions }}
        />
      )}
    </>
  );
}

function SiteTree() {
  const tree = useSiteMapStore((s) => s.tree);
  const map = useSiteMapStore((s) => s.map);
  const loading = useSiteMapStore((s) => s.loading);
  const error = useSiteMapStore((s) => s.error);
  const load = useSiteMapStore((s) => s.load);
  const filter = useSiteMapStore((s) => s.filter);
  const conditions = useSiteMapStore((s) => s.conditions);
  if (error)
    return <ErrorState title="Could not build the site map" message={error} onRetry={load} />;
  if (!map) return <LoadingState />;
  if (tree.length === 0) {
    return loading ? (
      <LoadingState />
    ) : filter.inScopeOnly || filter.search || filter.hideStatic || conditions ? (
      <EmptyState title="Nothing matches the filters" />
    ) : (
      <EmptyState title="No traffic yet">
        The site map is built automatically from everything the proxy captures.
      </EmptyState>
    );
  }
  return (
    <div className="site-tree" role="tree" aria-label="Site map">
      {tree.map((n) => (
        <TreeItem key={n.key} node={n} depth={0} />
      ))}
    </div>
  );
}

function TreeItem({ node, depth }: { node: SiteNode; depth: number }) {
  const expanded = useSiteMapStore((s) => s.expanded[node.key] ?? false);
  const selected = useSiteMapStore((s) => s.selectedKey === node.key);
  const { toggle, select } = useSiteMapStore.getState();
  const hasChildren = node.children.length > 0;
  return (
    <>
      <div
        role="treeitem"
        aria-expanded={hasChildren ? expanded : undefined}
        aria-selected={selected}
        tabIndex={selected ? 0 : -1}
        className={`tree-row${selected ? " selected" : ""}${node.mark === "ignore" ? " muted" : ""}`}
        style={{ paddingLeft: 8 + depth * 14 }}
        onClick={() => select(node.key)}
        onDoubleClick={() => hasChildren && toggle(node.key)}
        onContextMenu={(e) => {
          select(node.key);
          openContextMenu(e, "Site map actions", nodeMenu(node));
        }}
        onKeyDown={(e) => {
          if (isMenuKey(e)) {
            e.preventDefault();
            openMenuBelow(e.currentTarget, "Site map actions", nodeMenu(node));
            return;
          }
          if (e.key === "ArrowRight" && hasChildren && !expanded) toggle(node.key);
          if (e.key === "ArrowLeft" && expanded) toggle(node.key);
          if (e.key === "Enter") select(node.key);
        }}
      >
        <button
          type="button"
          className="tree-twisty"
          aria-label={expanded ? "Collapse" : "Expand"}
          tabIndex={-1}
          disabled={!hasChildren}
          onClick={(e) => {
            e.stopPropagation();
            toggle(node.key);
          }}
        >
          {hasChildren ? (expanded ? "▾" : "▸") : ""}
        </button>
        <span
          className={`scope-dot${node.inScope ? " in" : ""}`}
          title={node.inScope ? "In scope" : "Out of scope"}
        />
        <span className={node.kind === "origin" ? "tree-origin" : "mono"}>{node.label}</span>
        {node.mark && <span className={`mark mark-${node.mark}`}>{node.mark}</span>}
        <span className="spacer" />
        <span className="muted num">{node.count}</span>
      </div>
      {expanded && node.children.map((c) => <TreeItem key={c.key} node={c} depth={depth + 1} />)}
    </>
  );
}

function NodeDetail() {
  const tree = useSiteMapStore((s) => s.tree);
  const selectedKey = useSiteMapStore((s) => s.selectedKey);
  const setMark = useSiteMapStore((s) => s.setMark);
  const addUrl = useScopeStore((s) => s.addUrl);
  const node = useMemo(
    () => (selectedKey ? findNode(tree, selectedKey) : null),
    [tree, selectedKey],
  );
  const entries = useMemo(() => (node ? entriesUnder(node) : []), [node]);
  if (!node) {
    return (
      <EmptyState title="Select a node">
        Hosts, folders and endpoints seen in the traffic. Double-click or use the arrows to expand.
      </EmptyState>
    );
  }
  const url = siteLabel(siteOf(node));
  return (
    <div className="page node-detail">
      <div className="exchange-summary">
        <span className="mono selectable" title={url}>
          {url}
        </span>
        <span className="spacer" />
        <Badge tone={node.inScope ? "ok" : "neutral"}>
          {node.inScope ? "in scope" : "out of scope"}
        </Badge>
      </div>
      <div className="node-actions">
        <Button size="sm" variant="primary" onClick={() => showInLogger(node)}>
          Show {node.count} request{node.count === 1 ? "" : "s"}
        </Button>
        <Button size="sm" onClick={() => void addUrl(url, false, node.kind !== "origin")}>
          Add to scope
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void addUrl(url, true, node.kind !== "origin")}
        >
          Exclude from scope
        </Button>
        <span className="spacer" />
        <label className="field-label" htmlFor="node-mark">
          Mark
        </label>
        <select
          id="node-mark"
          value={node.mark ?? ""}
          onChange={(e) => void setMark(node, (e.target.value || null) as SiteMark | null)}
        >
          <option value="">None</option>
          {MARKS.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </div>
      <EndpointTable entries={entries} />
      <NodeExtras key={url} site={siteOf(node)} />
    </div>
  );
}

const KIND_LABELS: Record<PointKind, string> = {
  pathSegment: "path",
  query: "query",
  form: "form",
  cookie: "cookie",
  header: "header",
  json: "JSON",
  multipart: "multipart",
  xml: "XML",
};

/** Parameters seen under a node and its latest requests (M27). */
function NodeExtras({ site }: { site: SiteFilter }) {
  const [details, setDetails] = useState<SiteNodeDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"params" | "requests">("params");
  useEffect(() => {
    api
      .siteNodeDetails(site)
      .then(setDetails)
      .catch((e: unknown) => setError(toIpcError(e).message));
    // The key on this component reloads it for another node.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (error) return <p className="callout callout-danger">{error}</p>;
  if (!details) return null;
  return (
    <section className="node-extras">
      <div className="dialog-row">
        <Segmented<"params" | "requests">
          items={[
            { id: "params", label: `Parameters (${details.params.length})` },
            { id: "requests", label: `Requests (${details.total})` },
          ]}
          value={tab}
          onChange={setTab}
          label="Node details"
        />
        {tab === "params" && details.sampled < details.total && (
          <span className="muted">from the latest {details.sampled} requests</span>
        )}
      </div>
      {tab === "params" ? (
        details.params.length === 0 ? (
          <p className="muted">No parameters in these requests.</p>
        ) : (
          <table className="endpoint-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Where</th>
                <th className="num">Requests</th>
                <th>Example</th>
              </tr>
            </thead>
            <tbody>
              {details.params.map((p) => (
                <tr key={p.name}>
                  <td className="mono">{p.name}</td>
                  <td>{p.kinds.map((k) => KIND_LABELS[k]).join(", ")}</td>
                  <td className="num">{p.count}</td>
                  <td className="mono muted" title={p.example}>
                    {p.example}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )
      ) : (
        <table className="endpoint-table">
          <thead>
            <tr>
              <th className="num">#</th>
              <th>Method</th>
              <th>Path</th>
              <th>Status</th>
              <th>Time</th>
            </tr>
          </thead>
          <tbody>
            {details.requests.map((r) => (
              <tr
                key={r.id}
                className="clickable"
                title="Open in Repeater (right-click for more)"
                onClick={() => sendToRepeater(r.id)}
                onContextMenu={(e) => openContextMenu(e, `Request #${r.id}`, exchangeMenu(r))}
              >
                <td className="num">{r.id}</td>
                <td>
                  <span className={`method method-${r.method.toLowerCase()}`}>{r.method}</span>
                </td>
                <td className="mono" title={r.url}>
                  {r.path}
                </td>
                <td>
                  {r.status === null ? (
                    "—"
                  ) : (
                    <span className={`status status-${statusTone(r.status)}`}>{r.status}</span>
                  )}
                </td>
                <td>{formatDateTime(r.startedAt)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function EndpointTable({ entries }: { entries: SiteMapEntry[] }) {
  const shown = entries.slice(0, MAX_LISTED);
  return (
    <div className="endpoint-table-wrap">
      <table className="endpoint-table">
        <thead>
          <tr>
            <th>Path</th>
            <th>Methods</th>
            <th>Status</th>
            <th>Params</th>
            <th>MIME</th>
            <th className="num">Requests</th>
            <th>Source</th>
            <th>First seen</th>
            <th>Last seen</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((e) => (
            <tr key={`${e.scheme}://${e.host}:${e.port}${e.path}`}>
              <td className="mono" title={e.path}>
                <span className={`scope-dot${e.inScope ? " in" : ""}`} /> {e.path}
              </td>
              <td>
                {e.methods.map((m) => (
                  <span key={m} className={`method method-${m.toLowerCase()}`}>
                    {m}{" "}
                  </span>
                ))}
              </td>
              <td>
                {e.statuses.length === 0
                  ? "—"
                  : e.statuses.map((s) => (
                      <span key={s} className={`status status-${statusTone(s)}`}>
                        {s}{" "}
                      </span>
                    ))}
              </td>
              <td>{e.hasParams ? "?" : ""}</td>
              <td>{e.mimes.join(", ")}</td>
              <td className="num">{e.count}</td>
              <td className="muted">{e.sources.join(", ")}</td>
              <td className="muted">{formatDateTime(e.firstSeen)}</td>
              <td className="muted">{formatDateTime(e.lastSeen)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {entries.length > MAX_LISTED && (
        <p className="muted">
          Showing {MAX_LISTED} of {entries.length} endpoints; select a folder to narrow down.
        </p>
      )}
    </div>
  );
}
