import { describe, expect, it } from "vitest";
import type { AiActivity } from "../bindings/AiActivity";
import { choicesFor } from "../features/settings/AgentSection";
import { interrupted, transcript } from "./agent";

const row = (id: number, patch: Partial<AiActivity>): AiActivity => ({
  id,
  taskId: 1,
  at: id * 1000,
  action: "tool",
  tool: "",
  target: "",
  permission: "",
  decision: "",
  result: "",
  detail: "",
  ...patch,
});

describe("ChosAgent helpers", () => {
  it("turns the activity log into a transcript", () => {
    const items = transcript([
      row(1, { action: "goal", detail: "Goal: look" }),
      row(2, { action: "model", result: "2 tool call(s)", detail: "Let me look." }),
      row(3, { action: "model", result: "1 tool call(s)", detail: "" }),
      row(4, {
        tool: "send_request",
        target: "http://lab:80",
        permission: "ask",
        decision: "approved",
        result: "Sent; stored as #9.",
      }),
      row(5, { tool: "run_shell", permission: "", decision: "denied", result: "no tool" }),
      row(6, { action: "message", detail: "Keep it short" }),
      row(7, { action: "model", result: "answer", detail: "Done." }),
      row(8, { action: "answer", detail: "Done." }),
      row(9, { action: "stop", result: "stopped by the tester" }),
    ]);
    expect(items.map((i) => i.kind)).toEqual([
      "goal",
      "thinking",
      "tool",
      "tool",
      "message",
      "answer",
      "note",
    ]);
    expect(items[2]).toMatchObject({
      title: "send_request · http://lab:80",
      permission: "ask",
      decision: "approved by you",
    });
    expect(items[3]?.decision).toBe("refused (not allowed)");
  });

  it("tells an interrupted task from a stopped or finished one", () => {
    const tool = row(2, { tool: "search_history", decision: "ran" });
    expect(interrupted({ status: "cancelled" }, [row(1, { action: "goal" }), tool])).toBe(true);
    expect(
      interrupted({ status: "cancelled" }, [tool, row(3, { action: "stop", result: "stopped" })]),
    ).toBe(false);
    expect(interrupted({ status: "completed" }, [tool])).toBe(false);
    expect(interrupted({ status: "cancelled" }, [])).toBe(false);
    const items = transcript([row(1, { action: "resume", target: "task #4", detail: "Tool x" })]);
    expect(items[0]).toMatchObject({ kind: "note", title: "Continues task #4", body: "Tool x" });
  });

  it("never offers a permission above a tool's highest", () => {
    expect(choicesFor({ highest: "ask" })).toEqual(["deny", "ask"]);
    expect(choicesFor({ highest: "allow" })).toEqual(["deny", "ask", "allow"]);
  });
});
