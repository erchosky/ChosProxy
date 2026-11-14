import { describe, expect, it } from "vitest";
import type { PassiveCheck } from "../../bindings/PassiveCheck";
import { matchingDefinitions } from "./IssueDefinitionsDialog";

const check = (id: string, kind: string, description: string): PassiveCheck => ({
  id,
  kind,
  summary: `${kind} summary`,
  severity: "low",
  description,
  remediation: "Set the header.",
});

describe("issue definitions", () => {
  const checks = [
    check("b", "Missing header", "A header is missing."),
    check("a", "Cookie without Secure", "The cookie travels over plain HTTP."),
  ];

  it("lists definitions by type and searches every text", () => {
    expect(matchingDefinitions(checks, "").map((c) => c.id)).toEqual(["a", "b"]);
    expect(matchingDefinitions(checks, "PLAIN http").map((c) => c.id)).toEqual(["a"]);
    expect(matchingDefinitions(checks, "set the header")).toHaveLength(2);
    expect(matchingDefinitions(checks, "nothing like this")).toEqual([]);
  });
});
