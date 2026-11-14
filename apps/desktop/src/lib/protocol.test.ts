import { describe, expect, it } from "vitest";
import { requestProtocol, withProtocol } from "./protocol";

describe("request protocol", () => {
  it("reads the version of the request line", () => {
    expect(requestProtocol("GET / HTTP/2\nHost: a\n\n")).toBe("HTTP/2");
    expect(requestProtocol("GET / HTTP/2\r\nHost: a\r\n\r\n")).toBe("HTTP/2");
    expect(requestProtocol("GET / HTTP/1.1\nHost: a\n\n")).toBe("HTTP/1.1");
    expect(requestProtocol("GET /HTTP/2-docs HTTP/1.1\n")).toBe("HTTP/1.1");
  });

  it("switches the version and keeps the rest", () => {
    expect(withProtocol("GET /a HTTP/1.1\nHost: a\n\nbody", "HTTP/2")).toBe(
      "GET /a HTTP/2\nHost: a\n\nbody",
    );
    expect(withProtocol("GET /a HTTP/2\r\nHost: a\r\n\r\n", "HTTP/1.1")).toBe(
      "GET /a HTTP/1.1\r\nHost: a\r\n\r\n",
    );
    expect(withProtocol("GET /a", "HTTP/2")).toBe("GET /a HTTP/2");
    expect(withProtocol("", "HTTP/2")).toBe("");
  });
});
