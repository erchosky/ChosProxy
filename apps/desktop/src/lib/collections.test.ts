import { describe, expect, it } from "vitest";
import { collectionTree, inCollection, normalizeCollection } from "./collections";

describe("Organizer collections", () => {
  it("builds a nested tree with counts that include children", () => {
    const tree = collectionTree(["Auth/Login", "Auth/Login", "Auth", "API / v2 ", "", "Zed"]);
    expect(tree.map((n) => [n.path, n.depth, n.count])).toEqual([
      ["API", 0, 1],
      ["API/v2", 1, 1],
      ["Auth", 0, 3],
      ["Auth/Login", 1, 2],
      ["Zed", 0, 1],
    ]);
    expect(tree[1]?.label).toBe("v2");
  });

  it("filters by a collection and what is under it", () => {
    expect(inCollection("Auth/Login", "Auth")).toBe(true);
    expect(inCollection("Auth", "Auth")).toBe(true);
    expect(inCollection("Authz", "Auth")).toBe(false);
    expect(inCollection("", "Auth")).toBe(false);
    expect(inCollection("anything", "")).toBe(true);
    expect(normalizeCollection(" a / b//c ")).toBe("a/b/c");
  });
});
