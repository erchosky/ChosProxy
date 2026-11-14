import { describe, expect, it } from "vitest";
import { diffHttp, diffText, diffTokens, tokenize } from "./diff";

function render(a: string, b: string, mode: "line" | "word" | "byte") {
  return diffText(a, b, mode)
    .ops.map((o) => {
      const t = o.tokens.join(mode === "byte" ? " " : "");
      return o.kind === "equal" ? t : o.kind === "insert" ? `[+${t}]` : `[-${t}]`;
    })
    .join("");
}

describe("diffTokens", () => {
  it("finds a minimal edit script", () => {
    const r = diffTokens([..."ABCABBA"], [..."CBABAC"]);
    expect(r.added + r.removed).toBe(5); // the classic Myers example
    const a = r.ops
      .filter((o) => o.kind !== "insert")
      .flatMap((o) => o.tokens)
      .join("");
    const b = r.ops
      .filter((o) => o.kind !== "delete")
      .flatMap((o) => o.tokens)
      .join("");
    expect([a, b]).toEqual(["ABCABBA", "CBABAC"]);
  });

  it("handles empty and identical inputs", () => {
    expect(diffTokens([], []).ops).toEqual([]);
    expect(diffTokens(["a"], ["a"])).toMatchObject({ added: 0, removed: 0, coarse: false });
    expect(diffTokens([], ["a"]).added).toBe(1);
  });
});

describe("diffText", () => {
  it("diffs words and lines", () => {
    expect(render("the quick fox", "the slow fox", "word")).toBe("the [-quick][+slow] fox");
    expect(render("a\nb\nc\n", "a\nc\nd\n", "line")).toBe("a\n[-b\n]c\n[+d\n]");
  });

  it("diffs bytes as hex", () => {
    expect(tokenize("é", "byte")).toEqual(["c3", "a9"]);
    expect(render("ab", "ac", "byte")).toBe("61[-62][+63]");
  });
});

describe("diffHttp", () => {
  it("reports status, header, parameter, cookie and body changes", () => {
    const a =
      "POST /login?next=/home HTTP/1.1\r\nHost: lab\r\nCookie: s=1; t=dark\r\n" +
      "Content-Type: application/x-www-form-urlencoded\r\n\r\nuser=alice&pass=x";
    const b =
      "POST /login?next=/admin HTTP/1.1\r\nHost: lab\r\nCookie: s=2\r\nX-New: 1\r\n" +
      "Content-Type: application/x-www-form-urlencoded\r\n\r\nuser=alice&pass=y";
    const d = diffHttp(a, b);
    expect(d.identical).toBe(false);
    expect(d.firstLine?.b).toBe("POST /login?next=/admin HTTP/1.1");
    expect(d.headers.map((h) => [h.name, h.a, h.b])).toEqual([
      ["Cookie", "s=1; t=dark", "s=2"],
      ["X-New", null, "1"],
    ]);
    expect(d.query).toEqual([{ name: "next", a: "/home", b: "/admin" }]);
    expect(d.cookies).toEqual([
      { name: "s", a: "1", b: "2" },
      { name: "t", a: "dark", b: null },
    ]);
    expect(d.form).toEqual([{ name: "pass", a: "x", b: "y" }]);
    expect(d.sizes.bodyA).toBe(17);
  });

  it("compares responses", () => {
    const d = diffHttp(
      "HTTP/1.1 200 OK\nContent-Length: 2\n\nok",
      "HTTP/1.1 403 Forbidden\nContent-Length: 2\n\nno",
    );
    expect(d.firstLine).toEqual({
      name: "first line",
      a: "HTTP/1.1 200 OK",
      b: "HTTP/1.1 403 Forbidden",
    });
    expect(d.headers).toEqual([]);
    expect(d.body.added).toBe(1);
  });
});
