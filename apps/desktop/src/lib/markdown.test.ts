import { describe, expect, it } from "vitest";
import { parseInline, parseMarkdown } from "./markdown";

describe("markdown", () => {
  it("parses blocks", () => {
    const blocks = parseMarkdown(
      "# Title\n\nSome *text* here\ncontinued.\n\n- one\n- **two**\n1. first\n\n> quote\n\n```\n<script>x</script>\n```\n---",
    );
    expect(blocks.map((b) => b.kind)).toEqual([
      "heading",
      "paragraph",
      "list",
      "list",
      "quote",
      "code",
      "rule",
    ]);
    expect(blocks[2]).toMatchObject({ kind: "list", ordered: false });
    expect(blocks[5]).toEqual({ kind: "code", text: "<script>x</script>" });
  });

  it("parses inline marks and keeps HTML as text", () => {
    expect(parseInline("a `b` **c** _d_ [e](https://lab.test/x) <img src=x>")).toEqual([
      { kind: "text", text: "a " },
      { kind: "code", text: "b" },
      { kind: "text", text: " " },
      { kind: "strong", text: "c" },
      { kind: "text", text: " " },
      { kind: "em", text: "d" },
      { kind: "text", text: " " },
      { kind: "link", text: "e", url: "https://lab.test/x" },
      { kind: "text", text: " <img src=x>" },
    ]);
  });
});
