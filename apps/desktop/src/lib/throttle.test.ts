import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { throttle } from "./throttle";

describe("throttle", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("runs at once, then merges calls into one trailing run", () => {
    const fn = vi.fn();
    const t = throttle(fn, 100);
    t();
    expect(fn).toHaveBeenCalledTimes(1);
    t();
    t();
    t();
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it("keeps running under a steady stream of calls (unlike a debounce)", () => {
    const fn = vi.fn();
    const t = throttle(fn, 100);
    for (let i = 0; i < 50; i++) {
      t();
      vi.advanceTimersByTime(20);
    }
    // 1 s of calls every 20 ms: about one run per 100 ms.
    expect(fn.mock.calls.length).toBeGreaterThanOrEqual(9);
    expect(fn.mock.calls.length).toBeLessThanOrEqual(11);
  });
});
