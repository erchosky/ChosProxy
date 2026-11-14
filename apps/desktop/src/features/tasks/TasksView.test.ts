import { describe, expect, it } from "vitest";
import { poolChoiceFor } from "./TasksView";

describe("task pool choice", () => {
  it("is offered only to running tasks that send through pools", () => {
    expect(poolChoiceFor({ kind: "crawl", status: "running" })).toBe(true);
    expect(poolChoiceFor({ kind: "fuzz", status: "paused" })).toBe(true);
    expect(poolChoiceFor({ kind: "scan", status: "completed" })).toBe(false);
    // Live tasks are passive and the browser crawl goes through the proxy.
    expect(poolChoiceFor({ kind: "live", status: "running" })).toBe(false);
    expect(poolChoiceFor({ kind: "browserCrawl", status: "running" })).toBe(false);
  });
});
