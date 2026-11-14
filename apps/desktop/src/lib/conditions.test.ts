import { describe, expect, it } from "vitest";
import {
  conditionProblem,
  conditionsScript,
  describeConditions,
  newCondition,
  opsFor,
} from "./conditions";

describe("condition builder", () => {
  it("writes a filter script", () => {
    const script = conditionsScript({
      match: "all",
      items: [
        { field: "responseHeader", name: "Server", op: "contains", value: 'ng"inx' },
        { field: "status", name: "", op: "greater", value: "399" },
        { field: "requestHeader", name: "Cookie", op: "missing", value: "" },
        { field: "url", name: "", op: "matches", value: "/api/v[0-9]+/" },
      ],
    });
    expect(script).toContain("fn filter(msg)");
    expect(script).toContain('(contains_ci(msg.response_header("Server"), "ng\\"inx"))');
    expect(script).toContain("(msg.status > 399)");
    expect(script).toContain('(msg.request_header("Cookie") == "")');
    expect(script).toContain('(regex_match(msg.url, "/api/v[0-9]+/"))');
    expect(script.match(/&&/g)?.length).toBe(3);
    expect(conditionsScript({ match: "any", items: [] })).toContain("    true\n");
  });

  it("checks and describes conditions", () => {
    expect(conditionProblem(newCondition())).toBeNull();
    expect(
      conditionProblem({ ...newCondition(), field: "status", op: "equals", value: "x" }),
    ).toMatch(/number/);
    expect(conditionProblem({ ...newCondition(), field: "responseHeader", name: " " })).toMatch(
      /header name/,
    );
    expect(opsFor("status").map((o) => o.id)).toEqual(["equals", "notEquals", "greater", "less"]);
    expect(
      describeConditions({
        match: "any",
        items: [
          { field: "status", name: "", op: "equals", value: "500" },
          { field: "responseHeader", name: "X-Debug", op: "exists", value: "" },
        ],
      }),
    ).toBe("Status = 500 or X-Debug is present");
  });
});
