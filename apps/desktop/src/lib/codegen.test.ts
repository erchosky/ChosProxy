import { describe, expect, it } from "vitest";
import { parseRequest, toFetch, toPython } from "./codegen";

const enc = (s: string) => new TextEncoder().encode(s);
const REQ = enc(
  'POST /api/login?next=/ HTTP/1.1\r\nHost: lab.test\r\nCookie: a="b"\r\nContent-Type: application/json\r\nContent-Length: 18\r\n\r\n{"user":"bob\\\\x"}',
);

describe("copy as code", () => {
  it("keeps method, headers and body; drops framing and a redundant Host", () => {
    const r = parseRequest("https://lab.test/api/login?next=/", REQ);
    expect(r.method).toBe("POST");
    expect(r.headers).toEqual([
      ["Cookie", 'a="b"'],
      ["Content-Type", "application/json"],
    ]);
  });

  it("writes Python with exact bytes", () => {
    const py = toPython("https://lab.test/api/login?next=/", REQ);
    expect(py).toContain('"POST",');
    expect(py).toContain('"Cookie": "a=\\"b\\"",');
    expect(py).toContain('data=b"{\\"user\\":\\"bob\\\\\\\\x\\"}",');
    expect(py).toContain("allow_redirects=False");
    const bin = toPython(
      "https://a.test/",
      new Uint8Array([...enc("PUT / HTTP/1.1\r\n\r\n"), 0, 255]),
    );
    expect(bin).toContain('data=b"\\x00\\xff",');
  });

  it("writes fetch, with binary bodies as bytes", () => {
    const js = toFetch("https://lab.test/x", REQ);
    expect(js).toContain('method: "POST"');
    expect(js).toContain('["Cookie", "a=\\"b\\""],');
    expect(js).toContain('redirect: "manual"');
    const bin = toFetch(
      "https://a.test/",
      new Uint8Array([...enc("PUT / HTTP/1.1\r\n\r\n"), 0, 255]),
    );
    expect(bin).toContain("body: new Uint8Array([0, 255]),");
  });
});
