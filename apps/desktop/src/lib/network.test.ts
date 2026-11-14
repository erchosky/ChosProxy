import { describe, expect, it } from "vitest";
import {
  describeMatchRule,
  matchProblem,
  moveItem,
  newMatchRule,
  newUpstreamRule,
  overrideProblem,
  upstreamProblem,
} from "./network";

describe("network helpers", () => {
  it("moves items within bounds", () => {
    expect(moveItem(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(moveItem(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
    expect(moveItem(["a", "b", "c"], 2, -2)).toEqual(["c", "a", "b"]);
  });

  it("checks upstream rules", () => {
    expect(upstreamProblem(newUpstreamRule())).toBeNull();
    expect(upstreamProblem({ ...newUpstreamRule(), host: "*.corp.test" })).toBeNull();
    expect(upstreamProblem({ ...newUpstreamRule(), host: "a b" })).toMatch(/Hosts/);
    expect(upstreamProblem({ ...newUpstreamRule(), proxyPort: 0 })).toMatch(/port/);
    expect(upstreamProblem({ ...newUpstreamRule(), username: "a:b" })).toMatch(/“:”/);
    expect(
      upstreamProblem({ ...newUpstreamRule(), kind: "direct", proxyHost: "", proxyPort: 0 }),
    ).toBeNull();
  });

  it("checks host overrides", () => {
    expect(overrideProblem({ enabled: true, host: "lab.test", address: "10.0.0.2" })).toBeNull();
    expect(overrideProblem({ enabled: true, host: "lab.test", address: "::1" })).toBeNull();
    expect(overrideProblem({ enabled: true, host: "lab.test", address: "nope" })).toMatch(/IPv4/);
    expect(overrideProblem({ enabled: true, host: "*", address: "10.0.0.2" })).toMatch(/Host/);
  });

  it("checks and describes Match & Replace rules", () => {
    const r = newMatchRule();
    expect(matchProblem(r)).toBeNull();
    expect(matchProblem({ ...r, target: "requestBody", match: "" })).toMatch(/text to find/);
    expect(matchProblem({ ...r, replace: "a\nb" })).toMatch(/Line breaks/);
    expect(matchProblem({ ...r, target: "responseBody", replace: "a\nb" })).toBeNull();
    expect(describeMatchRule({ ...r, match: "", replace: "X-A: 1" })).toBe(
      "Request header: add “X-A: 1”",
    );
    expect(describeMatchRule({ ...r, replace: "" })).toMatch(/remove lines matching/);
    expect(describeMatchRule({ ...r, target: "responseBody", match: "a", replace: "b" })).toBe(
      "Response body: a → b",
    );
    expect(matchProblem({ ...r, target: "requestScript", match: "" })).toMatch(/rewrite script/);
    expect(describeMatchRule({ ...r, target: "requestScript", match: "ua" })).toBe(
      "Request script: runs the script “ua”",
    );
  });
});
