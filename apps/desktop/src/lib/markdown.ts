// A small Markdown subset parsed into plain data (rendered as React text
// nodes by the notes view). Nothing is ever interpreted as HTML.

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "em"; text: string }
  | { kind: "link"; text: string; url: string };

export type Block =
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; inline: Inline[] }
  | { kind: "paragraph"; inline: Inline[] }
  | { kind: "list"; ordered: boolean; items: Inline[][] }
  | { kind: "quote"; inline: Inline[] }
  | { kind: "code"; text: string }
  | { kind: "rule" };

const INLINE = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*|_[^_]+_)|(\[[^\]]+\]\([^)\s]+\))/g;

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ kind: "text", text: text.slice(last, at) });
    const token = m[0];
    if (m[1]) out.push({ kind: "code", text: token.slice(1, -1) });
    else if (m[2]) out.push({ kind: "strong", text: token.slice(2, -2) });
    else if (m[3]) out.push({ kind: "em", text: token.slice(1, -1) });
    else {
      const close = token.indexOf("](");
      out.push({ kind: "link", text: token.slice(1, close), url: token.slice(close + 2, -1) });
    }
    last = at + token.length;
  }
  if (last < text.length) out.push({ kind: "text", text: text.slice(last) });
  return out;
}

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length)
      blocks.push({ kind: "paragraph", inline: parseInline(paragraph.join(" ")) });
    paragraph = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    const trimmed = line.trim();
    if (trimmed.startsWith("```")) {
      flush();
      const code: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? "").trim().startsWith("```"))
        code.push(lines[i++] ?? "");
      blocks.push({ kind: "code", text: code.join("\n") });
      continue;
    }
    if (trimmed === "") {
      flush();
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(trimmed);
    if (heading) {
      flush();
      blocks.push({
        kind: "heading",
        level: (heading[1]?.length ?? 1) as 1 | 2 | 3 | 4 | 5 | 6,
        inline: parseInline(heading[2] ?? ""),
      });
      continue;
    }
    if (/^(-{3,}|\*{3,})$/.test(trimmed)) {
      flush();
      blocks.push({ kind: "rule" });
      continue;
    }
    if (trimmed.startsWith(">")) {
      flush();
      blocks.push({ kind: "quote", inline: parseInline(trimmed.replace(/^>\s?/, "")) });
      continue;
    }
    const item = /^([-*+]|\d+[.)])\s+(.*)$/.exec(trimmed);
    if (item) {
      flush();
      const ordered = /\d/.test(item[1] ?? "");
      const prev = blocks[blocks.length - 1];
      const inline = parseInline(item[2] ?? "");
      if (prev?.kind === "list" && prev.ordered === ordered) prev.items.push(inline);
      else blocks.push({ kind: "list", ordered, items: [inline] });
      continue;
    }
    paragraph.push(trimmed);
  }
  flush();
  return blocks;
}
