import { describe, expect, it } from "vitest";
import { mergeRules, parseScopeList } from "./scopeImport";

describe("parseScopeList", () => {
  it("turns hosts and URLs into rules", () => {
    const { rules, skipped } = parseScopeList(
      [
        "# targets",
        "example.com",
        "*.shop.test",
        "https://api.test:8443/v2/",
        "http://10.0.0.5/",
        "example.com",
        "ftp://files.test",
        "not a host",
        "",
      ].join("\n"),
    );
    expect(skipped).toEqual(["ftp://files.test", "not a host"]);
    expect(rules.map((r) => [r.protocol, r.host, r.includeSubdomains, r.port, r.path])).toEqual([
      ["any", "example.com", false, null, ""],
      ["any", "shop.test", true, null, ""],
      ["https", "api.test", false, 8443, "/v2/"],
      ["http", "10.0.0.5", false, null, ""],
    ]);
    expect(rules.every((r) => r.enabled && !r.regex)).toBe(true);
  });

  it("merges without duplicates", () => {
    const a = parseScopeList("example.com\nb.test").rules;
    const b = parseScopeList("b.test\nc.test").rules;
    expect(mergeRules(a, b).map((r) => r.host)).toEqual(["example.com", "b.test", "c.test"]);
  });
});
