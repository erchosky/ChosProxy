import { describe, expect, it } from "vitest";
import { formatBytes, formatDuration, statusTone } from "./format";

describe("formatBytes", () => {
  it("formats across units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(20 * 1024 * 1024)).toBe("20 MB");
  });
  it("handles missing values", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(-1)).toBe("—");
  });
});

describe("formatDuration", () => {
  it("switches to seconds", () => {
    expect(formatDuration(0)).toBe("<1 ms");
    expect(formatDuration(38.4)).toBe("38 ms");
    expect(formatDuration(1240)).toBe("1.24 s");
    expect(formatDuration(null)).toBe("—");
  });
});

describe("statusTone", () => {
  it("maps classes", () => {
    expect(statusTone(101)).toBe("info");
    expect(statusTone(204)).toBe("ok");
    expect(statusTone(302)).toBe("redirect");
    expect(statusTone(404)).toBe("client");
    expect(statusTone(503)).toBe("server");
    expect(statusTone(null)).toBe("none");
  });
});
