import { describe, expect, it } from "vitest";
import { languageOf, prettyBody } from "./pretty";

describe("languageOf", () => {
  it("uses Content-Type first", () => {
    expect(languageOf("application/json; charset=utf-8", "")).toBe("json");
    expect(languageOf("application/problem+json", "")).toBe("json");
    expect(languageOf("text/html", "")).toBe("html");
    expect(languageOf("application/xml", "")).toBe("xml");
    expect(languageOf("text/javascript", "")).toBe("javascript");
    expect(languageOf("text/css", "")).toBe("css");
  });
  it("sniffs when Content-Type is missing or generic", () => {
    expect(languageOf(null, '  {"a": 1}')).toBe("json");
    expect(languageOf(null, "{not json")).toBe("text");
    expect(languageOf("text/plain", "<!DOCTYPE html><p>")).toBe("html");
    expect(languageOf(undefined, "<?xml version='1.0'?><a/>")).toBe("xml");
  });
});

describe("prettyBody", () => {
  it("reformats JSON only", () => {
    expect(prettyBody('{"a":[1,2]}', "json")).toBe('{\n  "a": [\n    1,\n    2\n  ]\n}');
    expect(prettyBody("<a><b/></a>", "xml")).toBe("<a><b/></a>");
    expect(prettyBody("{broken", "json")).toBe("{broken");
  });
});
