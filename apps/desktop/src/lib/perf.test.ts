import { describe, expect, it } from "vitest";
import type { PerformanceSnapshot } from "../bindings/PerformanceSnapshot";
import { pushReading, sparkline, storedRate } from "./perf";

const at = (ms: number, stored: number): PerformanceSnapshot => ({
  at: ms,
  process: null,
  dbBytes: 0,
  walBytes: 0,
  stored,
  dropped: 0,
  proxy: {
    activeConnections: 0,
    totalConnections: 0,
    exchanges: 0,
    tunnels: 0,
    intercepted: 0,
    tlsClientFailures: 0,
    authFailures: 0,
    errors: 0,
  },
  poolInFlight: 0,
  poolWaiting: 0,
  tasksRunning: 0,
});

describe("performance helpers", () => {
  it("turns two readings into a rate", () => {
    expect(storedRate(at(0, 10), at(2000, 30))).toBe(10);
    expect(storedRate(at(0, 10), at(0, 30))).toBe(0);
    expect(storedRate(at(0, 30), at(2000, 0))).toBe(0); // a new project
  });

  it("keeps the newest readings", () => {
    expect(pushReading([1, 2, 3], 4, 3)).toEqual([2, 3, 4]);
    expect(pushReading([], 1)).toEqual([1]);
  });

  it("draws a sparkline in its box", () => {
    expect(sparkline([], 100, 20)).toBe("");
    expect(sparkline([0, 5, 10], 100, 20)).toBe("0.0,20.0 50.0,10.0 100.0,0.0");
    expect(sparkline([0, 0], 10, 4)).toBe("0.0,4.0 10.0,4.0");
  });
});
