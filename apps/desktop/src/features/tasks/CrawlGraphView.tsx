import { useEffect, useMemo, useState } from "react";
import type { CrawlGraph } from "../../bindings/CrawlGraph";
import type { TaskRow } from "../../bindings/TaskRow";
import { Button } from "../../components/Button";
import { NODE_WIDTH, ROW_HEIGHT, layoutCrawlGraph } from "../../lib/crawlGraph";
import { statusTone } from "../../lib/format";
import { api, toIpcError } from "../../services/ipc";
import { openContextMenu } from "../../state/contextMenu";
import { exchangeMenu } from "../../state/tools";

/** The requests of a crawl as a graph: columns by depth, "found on" lines. */
export function CrawlGraphView({ task }: { task: TaskRow }) {
  const [open, setOpen] = useState(false);
  const [graph, setGraph] = useState<CrawlGraph | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<number | null>(null);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    api.crawlGraph(task.id).then(
      (g) => alive && setGraph(g),
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
    // Read again when the task makes progress.
  }, [open, task.id, task.requests, task.status]);

  const layout = useMemo(() => (graph ? layoutCrawlGraph(graph) : null), [graph]);
  const picked = layout?.nodes.find((n) => n.id === selected) ?? null;

  return (
    <section className="crawl-graph" aria-label="Crawl graph">
      <div className="dialog-row">
        <strong>Crawl graph</strong>
        <span className="muted">where each request was found</span>
        <span className="spacer" />
        <Button size="sm" variant="ghost" onClick={() => setOpen((o) => !o)}>
          {open ? "Hide" : "Show"}
        </Button>
      </div>
      {open && error && <p className="callout callout-danger">{error}</p>}
      {open && layout && layout.nodes.length === 0 && (
        <p className="muted">No requests of this task carry a crawl reason yet.</p>
      )}
      {open && layout && layout.nodes.length > 0 && (
        <>
          <div className="crawl-graph-canvas">
            <svg
              width={layout.width + 8}
              height={layout.height + 8}
              role="img"
              aria-label={`${layout.nodes.length} requests`}
            >
              <g transform="translate(4,4)">
                {layout.edges.map((e) => (
                  <path
                    key={`${e.from}-${e.to}`}
                    className={
                      selected === e.from || selected === e.to
                        ? "crawl-edge crawl-edge-on"
                        : "crawl-edge"
                    }
                    d={`M${e.x1},${e.y1} C${e.x1 + 20},${e.y1} ${e.x2 - 20},${e.y2} ${e.x2},${e.y2}`}
                  />
                ))}
                {layout.nodes.map((n) => (
                  <g
                    key={n.id}
                    transform={`translate(${n.x},${n.y})`}
                    className={selected === n.id ? "crawl-node crawl-node-on" : "crawl-node"}
                    onClick={() => setSelected(n.id)}
                    onContextMenu={(ev) => {
                      setSelected(n.id);
                      openContextMenu(
                        ev,
                        `Request #${n.id}`,
                        exchangeMenu({ id: n.id, url: n.url }),
                      );
                    }}
                  >
                    <title>{`#${n.id} ${n.url} (${n.reason})`}</title>
                    <rect width={NODE_WIDTH} height={ROW_HEIGHT - 6} rx={3} />
                    <circle
                      cx={9}
                      cy={(ROW_HEIGHT - 6) / 2}
                      r={3.5}
                      className={`crawl-dot crawl-dot-${statusTone(n.status)}`}
                    />
                    <text x={18} y={(ROW_HEIGHT - 6) / 2 + 4}>
                      {n.label.length > 30 ? `${n.label.slice(0, 29)}…` : n.label}
                    </text>
                  </g>
                ))}
              </g>
            </svg>
          </div>
          <p className="muted">
            {picked
              ? `#${picked.id} · ${picked.status ?? "no response"} · ${picked.reason} · depth ${picked.depth} · ${picked.url}`
              : "Click a request to see why it was made; right-click for the request menu."}
            {layout.hidden > 0 && ` ${layout.hidden} more requests are not drawn.`}
            {graph?.truncated && " The crawl made more requests than a graph holds."}
          </p>
        </>
      )}
    </section>
  );
}
