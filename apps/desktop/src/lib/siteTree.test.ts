import { describe, expect, it } from "vitest";
import type { SiteMapEntry } from "../bindings/SiteMapEntry";
import { buildSiteTree, entriesUnder, findNode } from "./siteTree";

function entry(path: string, extra: Partial<SiteMapEntry> = {}): SiteMapEntry {
  return {
    scheme: "https",
    host: "lab.test",
    port: 443,
    path,
    count: 1,
    methods: ["GET"],
    statuses: [200],
    mimes: ["HTML"],
    sources: ["proxy"],
    hasParams: false,
    firstSeen: 1,
    lastSeen: 2,
    lastId: 1,
    inScope: false,
    ...extra,
  };
}

describe("buildSiteTree", () => {
  it("nests folders and endpoints under their origin", () => {
    const roots = buildSiteTree([
      entry("/"),
      entry("/api/"),
      entry("/api/users", { count: 3, inScope: true }),
      entry("/api/v1/items"),
      entry("/robots.txt"),
      entry("/", { host: "a.test", scheme: "http", port: 8080 }),
    ]);
    expect(roots.map((r) => r.label)).toEqual(["http://a.test:8080", "https://lab.test"]);
    const lab = roots[1];
    expect(lab?.entry?.path).toBe("/");
    expect(lab?.count).toBe(7);
    expect(lab?.endpoints).toBe(5);
    expect(lab?.inScope).toBe(true);
    expect(lab?.children.map((c) => c.label)).toEqual(["api/", "robots.txt"]);
    const api = lab?.children[0];
    expect(api?.kind).toBe("folder");
    expect(api?.entry?.path).toBe("/api/");
    expect(api?.children.map((c) => `${c.kind}:${c.label}`)).toEqual([
      "folder:v1/",
      "endpoint:users",
    ]);
    expect(api?.count).toBe(5);
    expect(roots[0]?.inScope).toBe(false);
  });

  it("attaches marks and finds nodes", () => {
    const roots = buildSiteTree(
      [entry("/api/users"), entry("/login")],
      [
        { scheme: "https", host: "lab.test", port: 443, path: "/api/", mark: "interesting" },
        { scheme: "https", host: "lab.test", port: 443, path: "/login", mark: "reviewed" },
        { scheme: "https", host: "gone.test", port: 443, path: "/", mark: "ignore" },
      ],
    );
    const api = findNode(roots, "https://lab.test/api/");
    expect(api?.mark).toBe("interesting");
    expect(findNode(roots, "https://lab.test/login")?.mark).toBe("reviewed");
    expect(findNode(roots, "https://nope.test/")).toBeNull();
    expect(entriesUnder(roots[0] as never).map((e) => e.path)).toEqual(["/api/users", "/login"]);
  });
});
