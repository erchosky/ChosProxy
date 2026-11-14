import { describe, expect, it, vi } from "vitest";
import type { SiteNode } from "../../lib/siteTree";
import { isItem } from "../../state/contextMenu";
import { nodeMenu } from "./siteMapMenu";

vi.mock("../../services/ipc", () => ({ api: {}, dialogs: {}, toIpcError: (e: unknown) => e }));

const node = (patch: Partial<SiteNode>): SiteNode => ({
  key: "k",
  kind: "folder",
  label: "api/",
  scheme: "https",
  host: "lab.test",
  port: 443,
  path: "/api/",
  entry: null,
  children: [],
  count: 3,
  endpoints: 2,
  inScope: false,
  mark: null,
  ...patch,
});

const labels = (n: SiteNode) =>
  nodeMenu(n)
    .filter(isItem)
    .map((e) => e.label);

describe("site map menu", () => {
  it("offers folder actions", () => {
    expect(labels(node({}))).toEqual([
      "Show 3 requests",
      "Crawl from here…",
      "Analyze target…",
      "Discover content here…",
      "Add folder to scope",
      "Exclude folder from scope",
      "Copy URL",
    ]);
    expect(labels(node({ kind: "origin", path: "/" }))).toContain("Add host to scope");
  });

  it("gives endpoints the full request menu for their latest request", () => {
    const entry = {
      scheme: "https",
      host: "lab.test",
      port: 443,
      path: "/api/users",
      count: 1,
      methods: ["GET"],
      statuses: [200],
      mimes: ["JSON"],
      sources: [],
      hasParams: false,
      firstSeen: 0,
      lastSeen: 0,
      lastId: 42,
      inScope: true,
    };
    const menu = nodeMenu(node({ kind: "endpoint", path: "/api/users", count: 1, entry }));
    expect(menu).toContainEqual({ heading: "Request #42" });
    const items = menu.filter(isItem).map((e) => e.label);
    expect(items).toContain("Send to Repeater");
    expect(items).toContain("Copy as curl command");
    expect(items).not.toContain("Add host to scope");
  });
});
