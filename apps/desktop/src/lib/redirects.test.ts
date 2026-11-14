import { describe, expect, it } from "vitest";
import { isRedirect, requestUrl, shouldFollow } from "./redirects";

describe("redirect policy", () => {
  const yes = () => Promise.resolve(true);
  const no = () => Promise.resolve(false);
  it("follows according to the policy", async () => {
    expect(await shouldFollow("never", "https://a.test/x", "https://a.test/y", yes)).toBe(false);
    expect(await shouldFollow("always", "https://a.test/", "https://evil.test/", no)).toBe(true);
    expect(await shouldFollow("onSite", "https://a.test/x", "https://a.test/y", no)).toBe(true);
    expect(await shouldFollow("onSite", "https://a.test/x", "http://a.test/y", yes)).toBe(false);
    expect(await shouldFollow("onSite", "https://a.test/", "https://b.a.test/", yes)).toBe(false);
    expect(await shouldFollow("inScope", "https://a.test/", "https://b.test/", yes)).toBe(true);
    expect(await shouldFollow("inScope", "https://a.test/", "https://b.test/", no)).toBe(false);
  });

  it("recognises redirects and builds request URLs", () => {
    expect([301, 302, 303, 307, 308].every(isRedirect)).toBe(true);
    expect(isRedirect(304) || isRedirect(200) || isRedirect(null)).toBe(false);
    expect(requestUrl("https://a.test:8443", "GET /p?q=1 HTTP/1.1\nHost: a")).toBe(
      "https://a.test:8443/p?q=1",
    );
  });
});
