import { describe, expect, it } from "vitest";
import { SCRIPT_KINDS, TEMPLATES, scriptId } from "./scriptTemplates";

describe("script templates", () => {
  it("has a template defining the entry point of each kind", () => {
    const entry: Record<string, RegExp> = {
      check: /fn check\(msg\)/,
      filter: /fn filter\(msg\)/,
      column: /fn column\(msg\)/,
      action: /fn action\(msg\)/,
      rewrite: /fn request\(text\)/,
      payload: /fn payload\(p\)/,
    };
    for (const k of SCRIPT_KINDS) expect(TEMPLATES[k.id]).toMatch(entry[k.id] as RegExp);
  });

  it("makes unique ids", () => {
    expect(scriptId("Server version!", [])).toBe("server-version");
    expect(scriptId("Server version", ["server-version"])).toBe("server-version-2");
    expect(scriptId("¡¿", [])).toBe("script");
  });
});
