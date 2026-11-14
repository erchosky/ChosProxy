import { prettyBody } from "./pretty";
/** The header block of an HTTP message in text form (without the first
 * line and the empty line). */
export function headersOf(text: string): string {
  const normalized = text.replace(/\r\n/g, "\n");
  const end = normalized.indexOf("\n\n");
  const head = end === -1 ? normalized : normalized.slice(0, end);
  const firstBreak = head.indexOf("\n");
  return firstBreak === -1 ? "" : head.slice(firstBreak + 1);
}

/** Human-readable age, e.g. "4 s", "2 min". */
export function waited(sinceMs: number, nowMs: number): string {
  const s = Math.max(0, Math.round((nowMs - sinceMs) / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  return `${m} min`;
}

/**
 * The request text with its JSON or XML body re-indented ("pretty") or,
 * for JSON, compacted; Content-Length follows the new body. Null when the
 * body is not JSON or XML (or is chunked).
 */
export function reformatBody(text: string, mode: "pretty" | "compact"): string | null {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const split = text.indexOf(eol + eol);
  if (split === -1) return null;
  const headLines = text.slice(0, split).split(eol);
  const body = text.slice(split + eol.length * 2);
  if (body.trim() === "") return null;
  const header = (name: string) =>
    headLines
      .slice(1)
      .find((l) => l.toLowerCase().startsWith(`${name}:`))
      ?.slice(name.length + 1)
      .trim()
      .toLowerCase() ?? "";
  if (header("transfer-encoding").includes("chunked")) return null;
  const type = header("content-type");
  const trimmed = body.trim();
  const isJson = type.includes("json") || /^[[{]/.test(trimmed);
  const isXml = !isJson && (type.includes("xml") || trimmed.startsWith("<"));
  let next: string;
  if (isJson) {
    let doc: unknown;
    try {
      doc = JSON.parse(trimmed);
    } catch {
      return null;
    }
    next = mode === "pretty" ? JSON.stringify(doc, null, 2) : JSON.stringify(doc);
  } else if (isXml) {
    next = mode === "pretty" ? prettyBody(trimmed, "xml") : trimmed.replace(/>\s+</g, "><");
  } else {
    return null;
  }
  if (next === body) return text;
  return replaceBody(text, next);
}

/** The body of an HTTP message in text form ("" when it has none). */
export function bodyOf(text: string): string {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const split = text.indexOf(eol + eol);
  return split === -1 ? "" : text.slice(split + eol.length * 2);
}

/**
 * The message with another body; Content-Length follows (it is added when
 * missing). Null when the message has no head/body separator or is chunked.
 */
export function replaceBody(text: string, body: string): string | null {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const split = text.indexOf(eol + eol);
  if (split === -1) return null;
  const headLines = text.slice(0, split).split(eol);
  if (headLines.some((l, i) => i > 0 && /^transfer-encoding:.*chunked/i.test(l))) return null;
  const length = String(new TextEncoder().encode(body).length);
  let found = false;
  const head = headLines.map((l, i) => {
    if (i > 0 && l.toLowerCase().startsWith("content-length:")) {
      found = true;
      return `${l.slice(0, l.indexOf(":"))}: ${length}`;
    }
    return l;
  });
  if (!found) head.push(`Content-Length: ${length}`);
  return head.join(eol) + eol + eol + body;
}
