import { describe, expect, it } from "vitest";
import { groupOf, toggleCheck } from "./CheckPicker";

describe("check picker", () => {
  it("groups checks by id", () => {
    expect(groupOf("active-sqli")).toBe("Active audit");
    expect(groupOf("js-secrets")).toBe("JavaScript");
    expect(groupOf("cross-referer-leak")).toBe("Across requests");
    expect(groupOf("cookie-secure")).toBe("Single message");
  });

  it("turns checks on and off", () => {
    expect(toggleCheck([], "a", false)).toEqual(["a"]);
    expect(toggleCheck(["a", "b"], "a", true)).toEqual(["b"]);
    expect(toggleCheck(["a"], "a", false)).toEqual(["a"]);
  });
});
