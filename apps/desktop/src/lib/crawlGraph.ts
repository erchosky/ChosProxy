import type { CrawlGraph } from "../bindings/CrawlGraph";

/** A node placed on the canvas. */
export interface PlacedNode {
  id: number;
  x: number;
  y: number;
  /** Path and query (the host only when it differs from the first node's). */
  label: string;
  url: string;
  status: number | null;
  reason: string;
  depth: number;
}

export interface PlacedEdge {
  from: number;
  to: number;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface CrawlLayout {
  nodes: PlacedNode[];
  edges: PlacedEdge[];
  width: number;
  height: number;
  /** Nodes left out because the graph is larger than `maxNodes`. */
  hidden: number;
}

export const COLUMN_WIDTH = 260;
export const ROW_HEIGHT = 26;
export const NODE_WIDTH = 220;

function labelOf(url: string, firstHost: string | null): string {
  try {
    const u = new URL(url);
    const path = `${u.pathname}${u.search}` || "/";
    return u.host === firstHost ? path : `${u.host}${path}`;
  } catch {
    return url;
  }
}

/**
 * Places a crawl graph in columns by depth (start URLs on the left), in the
 * order the requests were made. Edges go from the right side of the page a
 * URL was found on to the left side of the request it caused.
 */
export function layoutCrawlGraph(graph: CrawlGraph, maxNodes = 400): CrawlLayout {
  const shown = graph.nodes.slice(0, maxNodes);
  let firstHost: string | null = null;
  try {
    if (shown[0]) firstHost = new URL(shown[0].url).host;
  } catch {
    firstHost = null;
  }
  const rows = new Map<number, number>();
  const nodes: PlacedNode[] = shown.map((n) => {
    const row = rows.get(n.depth) ?? 0;
    rows.set(n.depth, row + 1);
    return {
      id: n.id,
      x: n.depth * COLUMN_WIDTH,
      y: row * ROW_HEIGHT,
      label: labelOf(n.url, firstHost),
      url: n.url,
      status: n.status,
      reason: n.reason,
      depth: n.depth,
    };
  });
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const edges: PlacedEdge[] = [];
  for (const e of graph.edges) {
    const a = byId.get(e.from);
    const b = byId.get(e.to);
    if (!a || !b) continue;
    edges.push({
      from: e.from,
      to: e.to,
      x1: a.x + NODE_WIDTH,
      y1: a.y + ROW_HEIGHT / 2,
      x2: b.x,
      y2: b.y + ROW_HEIGHT / 2,
    });
  }
  const columns = Math.max(0, ...nodes.map((n) => n.depth)) + 1;
  const tallest = Math.max(0, ...rows.values());
  return {
    nodes,
    edges,
    width: nodes.length ? (columns - 1) * COLUMN_WIDTH + NODE_WIDTH : 0,
    height: tallest * ROW_HEIGHT,
    hidden: graph.nodes.length - shown.length,
  };
}
