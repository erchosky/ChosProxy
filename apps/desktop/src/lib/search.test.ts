import { describe, expect, it } from "vitest";
import { fuzzyScore } from "./search";

describe("fuzzyScore", () => {
  it("matches subsequences and rejects others", () => {
    expect(fuzzyScore("hist", "Go to HTTP history")).not.toBeNull();
    expect(fuzzyScore("gth", "Go to HTTP history")).not.toBeNull();
    expect(fuzzyScore("xyz", "Go to HTTP history")).toBeNull();
    expect(fuzzyScore("", "anything")).toBe(0);
  });
  it("ranks contiguous word-start matches higher", () => {
    const a = fuzzyScore("set", "Open Settings") ?? -Infinity;
    const b = fuzzyScore("set", "Close project listener") ?? -Infinity;
    expect(a).toBeGreaterThan(b);
  });
});
