import { describe, expect, it } from "vitest";
import {
  DEFAULT_FILTER,
  STATIC_EXTENSIONS,
  clearedFilter,
  isFiltered,
  siteLabel,
  toQuery,
  toggle,
} from "./filters";

describe("toQuery", () => {
  it("maps the default filter to an unfiltered newest-first page", () => {
    expect(toQuery(DEFAULT_FILTER, 0, 200)).toEqual({
      offset: 0,
      limit: 200,
      order: "desc",
      sort: "id",
      search: null,
      searchRegex: false,
      inScopeOnly: false,
      mimes: [],
      sources: [],
      highlights: [],
      annotatedOnly: false,
      site: null,
      methods: [],
      statusClasses: [],
      hideExtensions: [],
    });
  });

  it("maps every option", () => {
    const site = { scheme: "http", host: "lab.test", port: 80, path: "/api/" };
    const q = toQuery(
      {
        search: "  /api ",
        regex: true,
        methods: ["POST"],
        statusClasses: [4, 0],
        hideStatic: true,
        inScopeOnly: true,
        annotatedOnly: true,
        sources: ["repeater"],
        site,
        mimes: ["JSON"],
        sort: "status",
        newestFirst: false,
      },
      400,
      200,
    );
    expect(q.mimes).toEqual(["JSON"]);
    expect(q.sort).toBe("status");
    expect(q.search).toBe("/api");
    expect(q.searchRegex).toBe(true);
    expect(q.order).toBe("asc");
    expect(q.methods).toEqual(["POST"]);
    expect(q.statusClasses).toEqual([4, 0]);
    expect(q.hideExtensions).toBe(STATIC_EXTENSIONS);
    expect(q.inScopeOnly).toBe(true);
    expect(q.annotatedOnly).toBe(true);
    expect(q.sources).toEqual(["repeater"]);
    expect(q.site).toBe(site);
    expect(q.offset).toBe(400);
  });
});

describe("helpers", () => {
  it("detects active filters", () => {
    expect(isFiltered(DEFAULT_FILTER)).toBe(false);
    expect(isFiltered({ ...DEFAULT_FILTER, newestFirst: false })).toBe(false);
    expect(isFiltered({ ...DEFAULT_FILTER, search: "x" })).toBe(true);
    expect(isFiltered({ ...DEFAULT_FILTER, inScopeOnly: true })).toBe(true);
    expect(
      isFiltered({ ...DEFAULT_FILTER, site: { scheme: "http", host: "a", port: 80, path: "/" } }),
    ).toBe(true);
  });
  it("clears filters but keeps the order", () => {
    const f = { ...DEFAULT_FILTER, search: "x", inScopeOnly: true, newestFirst: false };
    expect(clearedFilter(f)).toEqual({ ...DEFAULT_FILTER, newestFirst: false });
  });
  it("labels sites without default ports", () => {
    expect(siteLabel({ scheme: "https", host: "lab.test", port: 443, path: "/a/" })).toBe(
      "https://lab.test/a/",
    );
    expect(siteLabel({ scheme: "http", host: "lab.test", port: 8080, path: "/" })).toBe(
      "http://lab.test:8080/",
    );
  });
  it("toggles list membership", () => {
    expect(toggle(["a"], "b")).toEqual(["a", "b"]);
    expect(toggle(["a", "b"], "a")).toEqual(["b"]);
  });
});
