import { describe, expect, it } from "vitest";
import type { CrawlGraph } from "../bindings/CrawlGraph";
import { COLUMN_WIDTH, NODE_WIDTH, ROW_HEIGHT, layoutCrawlGraph } from "./crawlGraph";

const node = (id: number, url: string, depth: number) => ({
  id,
  url,
  status: 200,
  depth,
  reason: depth === 0 ? "start URL" : "a link",
});

const graph: CrawlGraph = {
  nodes: [
    node(1, "http://lab.test/", 0),
    node(2, "http://lab.test/a?x=1", 1),
    node(3, "http://lab.test/b", 1),
    node(4, "http://cdn.test/app.js", 2),
  ],
  edges: [
    { from: 1, to: 2 },
    { from: 1, to: 3 },
    { from: 3, to: 4 },
    { from: 99, to: 4 },
  ],
  truncated: false,
};

describe("crawl graph layout", () => {
  it("puts requests in columns by depth, in order", () => {
    const l = layoutCrawlGraph(graph);
    expect(l.nodes.map((n) => [n.x, n.y])).toEqual([
      [0, 0],
      [COLUMN_WIDTH, 0],
      [COLUMN_WIDTH, ROW_HEIGHT],
      [2 * COLUMN_WIDTH, 0],
    ]);
    expect(l.nodes.map((n) => n.label)).toEqual(["/", "/a?x=1", "/b", "cdn.test/app.js"]);
    expect(l.width).toBe(2 * COLUMN_WIDTH + NODE_WIDTH);
    expect(l.height).toBe(2 * ROW_HEIGHT);
    // Edges join known nodes only, right side to left side.
    expect(l.edges).toHaveLength(3);
    expect(l.edges[2]).toMatchObject({
      from: 3,
      to: 4,
      x1: COLUMN_WIDTH + NODE_WIDTH,
      x2: 2 * COLUMN_WIDTH,
    });
  });

  it("leaves out what does not fit and says how many", () => {
    const l = layoutCrawlGraph(graph, 2);
    expect(l.nodes).toHaveLength(2);
    expect(l.hidden).toBe(2);
    expect(l.edges).toHaveLength(1);
    expect(layoutCrawlGraph({ nodes: [], edges: [], truncated: false })).toMatchObject({
      width: 0,
      height: 0,
    });
  });
});
