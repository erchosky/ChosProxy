import { describe, expect, it } from "vitest";
import { splitMatch } from "./dom";

describe("splitMatch", () => {
  it("splits around the first match", () => {
    expect(splitMatch("<b>chosabc</b>", "chosabc")).toEqual({
      before: "<b>",
      match: "chosabc",
      after: "</b>",
    });
    expect(splitMatch("abc", null)).toBeNull();
    expect(splitMatch("abc", "zz")).toBeNull();
  });
});
