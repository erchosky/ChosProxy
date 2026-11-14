import { describe, expect, it } from "vitest";
import { linesToHosts, validateHostPatterns } from "./hostPatterns";

describe("host patterns", () => {
  it("splits lines and drops blanks", () => {
    expect(linesToHosts(" a.test \n\n*.b.test\n")).toEqual(["a.test", "*.b.test"]);
  });
  it("accepts exact and wildcard patterns", () => {
    expect(validateHostPatterns("example.com\n*.example.com\n127.0.0.1\n::1")).toEqual([]);
  });
  it("rejects malformed patterns", () => {
    expect(validateHostPatterns("*\na*.b\nhas space\n*.")).toHaveLength(4);
  });
});
