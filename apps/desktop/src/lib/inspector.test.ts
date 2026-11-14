import { describe, expect, it } from "vitest";
import {
  applyInspection,
  decodeComponent,
  encodeComponent,
  inspectRequest,
  inspectResponse,
} from "./inspector";

const REQ =
  "POST /api/login?next=%2Fhome&debug HTTP/1.1\r\n" +
  "Host: lab.test\r\n" +
  "Cookie: session=abc123; theme=dark\r\n" +
  "Content-Type: application/x-www-form-urlencoded; charset=utf-8\r\n" +
  "Content-Length: 27\r\n" +
  "\r\n" +
  "user=alice&pass=p%40ss+word";

describe("inspectRequest", () => {
  it("parses query, cookies, headers and form body", () => {
    const i = inspectRequest(REQ);
    expect(i?.method).toBe("POST");
    expect(i?.path).toBe("/api/login");
    expect(i?.query.map((p) => [p.name, p.value])).toEqual([
      ["next", "/home"],
      ["debug", ""],
    ]);
    expect(i?.cookies.map((c) => [c.name, c.value])).toEqual([
      ["session", "abc123"],
      ["theme", "dark"],
    ]);
    expect(i?.form?.map((p) => [p.name, p.value])).toEqual([
      ["user", "alice"],
      ["pass", "p@ss word"],
    ]);
    expect(i?.bodyType).toBe("application/x-www-form-urlencoded");
    expect(i?.headers).toHaveLength(4);
  });

  it("returns null for responses and has no form for other bodies", () => {
    expect(inspectRequest("HTTP/1.1 200 OK\r\n\r\n")).toBeNull();
    const json = inspectRequest("POST /x HTTP/1.1\nContent-Type: application/json\n\n{}");
    expect(json?.form).toBeNull();
    expect(json?.bodyType).toBe("application/json");
  });
});

describe("applyInspection", () => {
  it("is byte-exact when nothing changed", () => {
    const i = inspectRequest(REQ);
    expect(i && applyInspection(REQ, i)).toBe(REQ);
  });

  it("rewrites only the edited query parameter", () => {
    const i = inspectRequest(REQ);
    if (!i) throw new Error("parse");
    const edited = {
      ...i,
      query: i.query.map((p) => (p.name === "debug" ? { ...p, value: "1 2" } : p)),
    };
    const out = applyInspection(REQ, edited);
    expect(out.split("\r\n")[0]).toBe("POST /api/login?next=%2Fhome&debug=1+2 HTTP/1.1");
    expect(out.slice(out.indexOf("\r\n"))).toBe(REQ.slice(REQ.indexOf("\r\n")));
  });

  it("updates the form body and Content-Length", () => {
    const i = inspectRequest(REQ);
    if (!i?.form) throw new Error("parse");
    const edited = { ...i, form: [...i.form, { name: "otp", value: "é" }] };
    const out = applyInspection(REQ, edited);
    expect(out.endsWith("\r\n\r\nuser=alice&pass=p%40ss+word&otp=%C3%A9")).toBe(true);
    expect(out).toContain("Content-Length: 38\r\n");
  });

  it("rewrites and removes cookies", () => {
    const i = inspectRequest(REQ);
    if (!i) throw new Error("parse");
    const one = applyInspection(REQ, { ...i, cookies: [{ name: "session", value: "zzz" }] });
    expect(one).toContain("Cookie: session=zzz\r\n");
    expect(one.indexOf("Cookie:")).toBe(REQ.indexOf("Cookie:"));
    const none = applyInspection(REQ, { ...i, cookies: [] });
    expect(none).not.toContain("Cookie:");
  });

  it("edits headers and keeps LF line endings", () => {
    const lf = "GET / HTTP/1.1\nHost: a\nAccept: */*\n\n";
    const i = inspectRequest(lf);
    if (!i) throw new Error("parse");
    const out = applyInspection(lf, {
      ...i,
      headers: [...i.headers.filter((h) => h.name !== "Accept"), { name: "X-Test", value: "1" }],
    });
    expect(out).toBe("GET / HTTP/1.1\nHost: a\nX-Test: 1\n\n");
  });
});

describe("components", () => {
  it("round-trips encoding and tolerates bad escapes", () => {
    expect(decodeComponent(encodeComponent("a b&c=d/é"))).toBe("a b&c=d/é");
    expect(decodeComponent("%zz+x")).toBe("%zz x");
  });
});

describe("body fields and responses", () => {
  it("flattens JSON bodies", () => {
    const req =
      'POST /api HTTP/1.1\nHost: a\nContent-Type: application/json; charset=utf-8\n\n{"user":{"name":"bob","tags":["x",2]},"ok":true,"none":null,"e":{}}';
    const fields = inspectRequest(req)?.bodyFields?.map(({ name, value }) => ({ name, value }));
    expect(fields).toEqual([
      { name: "user.name", value: "bob" },
      { name: "user.tags[0]", value: "x" },
      { name: "user.tags[1]", value: "2" },
      { name: "ok", value: "true" },
      { name: "none", value: "null" },
      { name: "e", value: "{}" },
    ]);
    expect(inspectRequest(req.replace("{", "{{"))?.bodyFields).toBeNull();
  });

  it("lists multipart fields and files", () => {
    const req =
      'POST /up HTTP/1.1\r\nContent-Type: multipart/form-data; boundary="b0"\r\n\r\n' +
      '--b0\r\nContent-Disposition: form-data; name="title"\r\n\r\nHello\r\n' +
      '--b0\r\nContent-Disposition: form-data; name="f"; filename="a.txt"\r\n\r\nabcd\r\n--b0--\r\n';
    const fields = inspectRequest(req)?.bodyFields ?? [];
    expect(fields.map((f) => [f.name, f.value, f.fixed ?? false])).toEqual([
      ["title", "Hello", false],
      ["f", "file a.txt (4 bytes)", true],
    ]);
  });

  it("edits a multipart value and leaves files alone", () => {
    const req =
      "POST /up HTTP/1.1\r\nContent-Type: multipart/form-data; boundary=b0\r\nContent-Length: 145\r\n\r\n" +
      '--b0\r\nContent-Disposition: form-data; name="title"\r\n\r\nHello\r\n' +
      '--b0\r\nContent-Disposition: form-data; name="f"; filename="a.txt"\r\n\r\nabcd\r\n--b0--\r\n';
    const inspection = inspectRequest(req);
    if (!inspection?.bodyFields) throw new Error("no fields");
    const edited = inspection.bodyFields.map((f) =>
      f.name === "title" ? { ...f, value: "Hi there" } : { ...f, value: "changed" },
    );
    const out = applyInspection(req, { ...inspection, bodyFields: edited });
    expect(out).toContain('name="title"\r\n\r\nHi there\r\n--b0');
    expect(out).toContain("\r\n\r\nabcd\r\n--b0--");
    const body = out.slice(out.indexOf("\r\n\r\n") + 4);
    expect(out).toContain(`Content-Length: ${new TextEncoder().encode(body).length}\r\n`);
  });

  it("lists and edits XML text and attributes with escapes", () => {
    const req =
      'POST /soap HTTP/1.1\r\nContent-Type: text/xml\r\n\r\n<?xml version="1.0"?>' +
      '<Get xmlns="urn:x" id="7"><user>bob &amp; co</user><empty/><n> 5 </n></Get>';
    const inspection = inspectRequest(req);
    const fields = inspection?.bodyFields ?? [];
    expect(fields.map((f) => [f.name, f.value])).toEqual([
      ["Get@id", "7"],
      ["user", "bob & co"],
      ["n", "5"],
    ]);
    if (!inspection) throw new Error("no inspection");
    const edited = fields.map((f) => (f.name === "user" ? { ...f, value: "<a&b>" } : f));
    const out = applyInspection(req, { ...inspection, bodyFields: edited });
    expect(out).toContain("<user>&lt;a&amp;b&gt;</user><empty/><n> 5 </n>");
    expect(out).toContain('xmlns="urn:x" id="7"');
  });

  it("shows the cookies a response sets", () => {
    const res = inspectResponse(
      "HTTP/1.1 200 OK\r\nSet-Cookie: sid=abc; Path=/; HttpOnly\r\nSet-Cookie: t\r\nServer: x\r\n\r\nbody",
    );
    expect(res?.cookies).toEqual([
      { name: "sid", value: "abc", attributes: "Path=/; HttpOnly" },
      { name: "t", value: "", attributes: "" },
    ]);
    expect(res?.headers).toHaveLength(3);
    expect(inspectResponse("GET / HTTP/1.1\r\n\r\n")).toBeNull();
  });
});

describe("JSON field editing", () => {
  it("changes values in place, keeping types, and updates Content-Length", () => {
    const req =
      'POST /api HTTP/1.1\nHost: a\nContent-Type: application/json\nContent-Length: 44\n\n{"user":{"name":"bob","age":30},"tags":["x"]}';
    const ins = inspectRequest(req);
    if (!ins?.bodyFields) throw new Error("no fields");
    const fields = ins.bodyFields.map((f) =>
      f.name === "user.name"
        ? { ...f, value: "alice" }
        : f.name === "user.age"
          ? { ...f, value: "31" }
          : f.name === "tags[0]"
            ? { ...f, value: "42" }
            : f,
    );
    const out = applyInspection(req, { ...ins, bodyFields: fields });
    const body = out.split("\n\n")[1] ?? "";
    expect(JSON.parse(body)).toEqual({ user: { name: "alice", age: 31 }, tags: ["42"] });
    expect(out).toContain(`Content-Length: ${new TextEncoder().encode(body).length}`);
    // Unchanged fields leave the body exactly as it was.
    expect(applyInspection(req, ins)).toBe(req);
  });
});
