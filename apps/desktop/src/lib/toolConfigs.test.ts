import { describe, expect, it } from "vitest";
import { loadableSettings, settingsOf } from "./toolConfigs";

const crawl = {
  startUrls: ["http://lab.test/"],
  maxDepth: 3,
  requestsPerSecond: 10,
  useCookieJar: true,
  avoid: ["logout"],
};

describe("tool settings library", () => {
  it("saves settings, not the target", () => {
    expect(settingsOf("crawler", crawl)).toEqual({
      maxDepth: 3,
      requestsPerSecond: 10,
      useCookieJar: true,
      avoid: ["logout"],
    });
    const fuzz = { target: { host: "a" }, template: "GET /", headCrlf: true, concurrency: 2 };
    expect(settingsOf("fuzzer", fuzz)).toEqual({ concurrency: 2 });
  });

  it("loads only settings the form has, of the same kind", () => {
    const json = JSON.stringify({
      startUrls: ["http://other.test/"],
      maxDepth: 1,
      requestsPerSecond: "fast",
      avoid: ["delete", "logout"],
      unknown: 1,
    });
    expect(loadableSettings("crawler", crawl, json)).toEqual({
      maxDepth: 1,
      avoid: ["delete", "logout"],
    });
    expect(loadableSettings("crawler", crawl, "[1]")).toBeNull();
    expect(loadableSettings("crawler", crawl, "nope")).toBeNull();
  });
});
