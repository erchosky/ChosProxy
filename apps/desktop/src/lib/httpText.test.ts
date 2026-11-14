import { describe, expect, it } from "vitest";
import { headersOf, reformatBody, waited } from "./httpText";

describe("headersOf", () => {
  it("returns only header lines", () => {
    expect(headersOf("GET / HTTP/1.1\nHost: a\nX: y\n\nbody")).toBe("Host: a\nX: y");
    expect(headersOf("HTTP/1.1 200 OK\r\nA: 1\r\n\r\n")).toBe("A: 1");
    expect(headersOf("GET / HTTP/1.1")).toBe("");
  });
});

describe("waited", () => {
  it("formats seconds and minutes", () => {
    expect(waited(0, 4_200)).toBe("4 s");
    expect(waited(0, 130_000)).toBe("2 min");
    expect(waited(10_000, 0)).toBe("0 s");
  });
});

describe("reformatBody", () => {
  const req =
    'POST /a HTTP/1.1\nHost: x\nContent-Type: application/json\nContent-Length: 13\n\n{"a":[1,2]}';

  it("pretty-prints and compacts JSON, keeping Content-Length right", () => {
    const pretty = reformatBody(req, "pretty");
    expect(pretty).toContain('\n\n{\n  "a": [\n    1,\n    2\n  ]\n}');
    expect(pretty).toContain("Content-Length: 29\n");
    const compact = reformatBody(pretty ?? "", "compact");
    expect(compact).toContain('Content-Length: 11\n\n{"a":[1,2]}');
  });

  it("adds a missing Content-Length and keeps CRLF", () => {
    const crlf = "POST /a HTTP/1.1\r\nHost: x\r\n\r\n<a> <b>1</b> </a>";
    expect(reformatBody(crlf, "compact")).toBe(
      "POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 15\r\n\r\n<a><b>1</b></a>",
    );
  });

  it("leaves other bodies alone", () => {
    expect(reformatBody("GET / HTTP/1.1\nHost: x\n\n", "pretty")).toBeNull();
    expect(reformatBody("POST / HTTP/1.1\nHost: x\n\na=1&b=2", "pretty")).toBeNull();
    expect(
      reformatBody('POST / HTTP/1.1\nContent-Type: application/json\n\n{"a":', "pretty"),
    ).toBeNull();
    expect(reformatBody("POST / HTTP/1.1\nTransfer-Encoding: chunked\n\n{}", "compact")).toBeNull();
  });
});
