import { describe, expect, it } from "vitest";
import { scanCounters } from "./ScanDashboard";

describe("scan dashboard", () => {
  it("lists the counters with their explanations", () => {
    const rows = scanCounters({
      config: "x",
      phases: [],
      crawlRequests: 10,
      crawlErrors: 1,
      crawlFound: 20,
      itemsRead: 9,
      audited: 5,
      duplicates: 3,
      noResponse: 1,
      outOfScope: 2,
      scripts: 1,
      activeRequests: 6,
      activeErrors: 1,
      activePoints: 3,
      activeSkipped: 2,
      findingsCreated: 4,
      findingsUpdated: 2,
      issuesBySeverity: {},
    });
    expect(rows.find(([l]) => l === "Skipped")).toEqual([
      "Skipped",
      3,
      "1 no response, 2 out of scope",
    ]);
    expect(rows.find(([l]) => l === "Active probes")).toEqual([
      "Active probes",
      6,
      "1 failed, 2 skipped",
    ]);
    expect(rows.find(([l]) => l === "Points tested")).toEqual([
      "Points tested",
      3,
      "insertion points audited actively",
    ]);
    expect(rows).toHaveLength(10);
  });
});
