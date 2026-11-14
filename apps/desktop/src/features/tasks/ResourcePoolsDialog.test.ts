import { describe, expect, it } from "vitest";
import { newPool, removePool, renamePool } from "./ResourcePoolsDialog";

describe("resource pool helpers", () => {
  const config = {
    pools: [newPool([]), { ...newPool(["Pool 1"]), concurrency: 2 }],
    assignments: { crawler: "Pool 1", fuzzer: "pool 2" },
  };

  it("names new pools without clashes", () => {
    expect(config.pools.map((p) => p.name)).toEqual(["Pool 1", "Pool 2"]);
    expect(newPool(["Pool 2"]).name).toBe("Pool 3");
  });

  it("moves assignments when a pool is renamed", () => {
    const renamed = renamePool(config, 1, "Slow");
    expect(renamed.pools[1]?.name).toBe("Slow");
    expect(renamed.assignments).toEqual({ crawler: "Pool 1", fuzzer: "Slow" });
  });

  it("drops the assignments of a removed pool", () => {
    const removed = removePool(config, 0);
    expect(removed.pools).toHaveLength(1);
    expect(removed.assignments).toEqual({ fuzzer: "pool 2" });
  });
});
