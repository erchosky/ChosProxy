import { describe, expect, it } from "vitest";
import { shellQuote, toCurl } from "./curl";

const enc = (s: string) => new TextEncoder().encode(s);

describe("shellQuote", () => {
  it("leaves safe words alone and quotes the rest", () => {
    expect(shellQuote("https://a.test/x?y=1")).toBe("'https://a.test/x?y=1'");
    expect(shellQuote("--http1.1")).toBe("--http1.1");
    expect(shellQuote("it's")).toBe(`'it'\\''s'`);
    expect(shellQuote("a\u0001b")).toBe("$'a\\x01b'");
    expect(shellQuote("")).toBe("''");
  });
});

describe("toCurl", () => {
  it("keeps method, headers and body, drops what curl computes", () => {
    const raw = enc(
      "POST /login?next=/ HTTP/1.1\r\nHost: lab.test:8080\r\nCookie: a=b; c='d'\r\n" +
        "Accept-Encoding: gzip\r\nContent-Length: 9\r\n\r\nuser=bob'",
    );
    const cmd = toCurl("http://lab.test:8080/login?next=/", raw);
    expect(cmd).toContain("-X POST");
    expect(cmd).not.toContain("Host:");
    expect(cmd).not.toContain("Content-Length");
    expect(cmd).toContain(`-H 'Cookie: a=b; c='\\''d'\\'''`);
    expect(cmd).toContain("--compressed");
    expect(cmd).toContain(`--data-binary 'user=bob'\\'''`);
    expect(cmd.endsWith("'http://lab.test:8080/login?next=/'")).toBe(true);
  });

  it("keeps an unusual Host, plain GETs have no -X, binary bodies are escaped", () => {
    const get = toCurl("https://a.test/", enc("GET / HTTP/1.1\nHost: other.test\n\n"));
    expect(get).not.toContain("-X");
    expect(get).toContain("-H 'Host: other.test'");
    const bin = toCurl(
      "https://a.test/u",
      new Uint8Array([...enc("PUT /u HTTP/1.1\r\nHost: a.test\r\n\r\n"), 0, 255, 10]),
    );
    expect(bin).toContain("--data-binary $'\\x00\\xff\\n'");
    expect(toCurl("https://a.test/", enc("HEAD / HTTP/1.0\r\n\r\n"))).toContain("--http1.0");
  });
});
