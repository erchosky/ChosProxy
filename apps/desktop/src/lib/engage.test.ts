import { describe, expect, it } from "vitest";
import { csrfPoc, escapeHtml, formPairs, framePoc, framingVerdict, DEFAULT_FRAME } from "./engage";

describe("csrfPoc", () => {
  it("rebuilds a urlencoded POST with escaped values", () => {
    const poc = csrfPoc(
      {
        method: "POST",
        url: "https://shop.test/account/email",
        contentType: "application/x-www-form-urlencoded",
        body: "email=a%40b.test&note=%22%3E%3Cx&x",
      },
      { autoSubmit: true },
    );
    expect(poc.encoding).toBe("urlencoded");
    expect(poc.html).toContain('<form action="https://shop.test/account/email" method="POST">');
    expect(poc.html).toContain('name="email" value="a@b.test"');
    expect(poc.html).toContain('name="note" value="&quot;&gt;&lt;x"');
    expect(poc.html).toContain('name="x" value=""');
    expect(poc.html).toContain("document.forms[0].submit()");
  });

  it("moves GET parameters into the form and explains limits", () => {
    const poc = csrfPoc(
      { method: "GET", url: "https://a.test/t?to=bob&amount=10", contentType: null, body: "" },
      { autoSubmit: false },
    );
    expect(poc.html).toContain('action="https://a.test/t" method="GET"');
    expect(poc.html).toContain('name="amount" value="10"');
    expect(poc.html).not.toContain("<script>");
    const put = csrfPoc(
      { method: "PUT", url: "https://a.test/x", contentType: "application/json", body: '{"a":1}' },
      { autoSubmit: false },
    );
    expect(put.encoding).toBe("text/plain");
    expect(put.html).toContain('enctype="text/plain"');
    expect(put.notes.join(" ")).toMatch(/cannot send PUT/);
  });

  it("keeps multipart fields", () => {
    const body = '--zz\r\nContent-Disposition: form-data; name="title"\r\n\r\nHi\r\n--zz--\r\n';
    const poc = csrfPoc(
      {
        method: "POST",
        url: "https://a.test/up",
        contentType: "multipart/form-data; boundary=zz",
        body,
      },
      { autoSubmit: false },
    );
    expect(poc.html).toContain('enctype="multipart/form-data"');
    expect(poc.html).toContain('name="title" value="Hi"');
  });

  it("parses form pairs and escapes", () => {
    expect(formPairs("a=1+2&b")).toEqual([
      ["a", "1 2"],
      ["b", ""],
    ]);
    expect(escapeHtml(`<a href="x">'&`)).toBe("&lt;a href=&quot;x&quot;&gt;&#39;&amp;");
  });
});

describe("FrameLab", () => {
  it("judges framing headers", () => {
    expect(framingVerdict([]).frameable).toBe(true);
    expect(framingVerdict([["X-Frame-Options", "DENY"]]).frameable).toBe(false);
    expect(framingVerdict([["X-Frame-Options", "ALLOW-FROM https://a"]]).frameable).toBe(true);
    expect(
      framingVerdict([["Content-Security-Policy", "default-src 'self'; frame-ancestors 'none'"]])
        .frameable,
    ).toBe(false);
    expect(
      framingVerdict([
        ["Content-Security-Policy", "frame-ancestors *"],
        ["X-Frame-Options", "DENY"],
      ]).frameable,
    ).toBe(true);
  });

  it("builds an escaped, bounded PoC page", () => {
    const html = framePoc('https://a.test/?q="><x', {
      ...DEFAULT_FRAME,
      decoy: "<b>Win</b>",
      opacity: 5,
      width: -3,
    });
    expect(html).toContain('src="https://a.test/?q=&quot;&gt;&lt;x"');
    expect(html).toContain("&lt;b&gt;Win&lt;/b&gt;");
    expect(html).toContain("opacity: 1;");
    expect(html).toContain("width: 100px;");
  });
});
