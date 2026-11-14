import { describe, expect, it } from "vitest";
import { formatTarget, parseTarget, targetOfUrl } from "./repeaterTarget";

describe("repeater targets", () => {
  it("parses URLs, host:port and bare hosts", () => {
    expect(parseTarget("https://Example.com")).toEqual({
      scheme: "https",
      host: "example.com",
      port: 443,
    });
    expect(parseTarget(" http://127.0.0.1:9090/path?x ")).toEqual({
      scheme: "http",
      host: "127.0.0.1",
      port: 9090,
    });
    expect(parseTarget("lab.test")).toEqual({ scheme: "http", host: "lab.test", port: 80 });
    expect(parseTarget("lab.test:443")).toEqual({ scheme: "https", host: "lab.test", port: 443 });
    expect(parseTarget("http://[::1]:8080")).toEqual({ scheme: "http", host: "::1", port: 8080 });
  });

  it("explains invalid input", () => {
    for (const bad of ["", "ftp://x", "http://", "x:0", "x:99999", "x:abc", "::1", "[::1"]) {
      expect(parseTarget(bad)).toHaveProperty("error");
    }
  });

  it("formats without default ports", () => {
    expect(formatTarget({ scheme: "https", host: "a.test", port: 443 })).toBe("https://a.test");
    expect(formatTarget({ scheme: "http", host: "a.test", port: 8080 })).toBe("http://a.test:8080");
    expect(formatTarget({ scheme: "http", host: "::1", port: 80 })).toBe("http://[::1]");
    expect(targetOfUrl("https://a.test:8443/x?y")?.port).toBe(8443);
    expect(targetOfUrl("nonsense:://")).toBeNull();
  });
});
