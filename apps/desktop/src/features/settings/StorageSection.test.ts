import { describe, expect, it } from "vitest";
import { cleanupSummary } from "./StorageSection";

describe("Storage section", () => {
  it("summarises a cleanup", () => {
    expect(cleanupSummary({ removed: 0, bytes: 0, failed: [] })).toBe("Nothing to remove.");
    const s = cleanupSummary({ removed: 2, bytes: 2048, failed: ["x: kept"] });
    expect(s).toContain("Removed 2 item(s)");
    expect(s).toContain("Kept: x: kept");
  });
});
