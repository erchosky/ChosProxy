// "Copy as code": a stored request as Python (requests) or JavaScript
// (fetch). Header order, the method and the exact body bytes are kept;
// Content-Length and Transfer-Encoding are left to the library.

export interface ParsedRequest {
  method: string;
  headers: [string, string][];
  body: Uint8Array;
}

function headEnd(raw: Uint8Array): { head: number; body: number } {
  for (let i = 0; i + 1 < raw.length; i++) {
    if (raw[i] === 0x0a && raw[i + 1] === 0x0a) return { head: i, body: i + 2 };
    if (raw[i] === 0x0d && raw[i + 1] === 0x0a && raw[i + 2] === 0x0d && raw[i + 3] === 0x0a)
      return { head: i, body: i + 4 };
  }
  return { head: raw.length, body: raw.length };
}

/** Method, headers to send (without framing and a redundant Host) and body. */
export function parseRequest(url: string, raw: Uint8Array): ParsedRequest {
  const { head, body } = headEnd(raw);
  const lines = new TextDecoder("utf-8", { fatal: false })
    .decode(raw.subarray(0, head))
    .split(/\r?\n/);
  const method = (lines[0] ?? "").split(" ")[0] || "GET";
  let host = "";
  try {
    host = new URL(url).host.toLowerCase();
  } catch {
    host = "";
  }
  const headers: [string, string][] = [];
  for (const line of lines.slice(1)) {
    const colon = line.indexOf(":");
    if (colon <= 0) continue;
    const name = line.slice(0, colon).trim();
    const value = line.slice(colon + 1).trim();
    const lower = name.toLowerCase();
    if (lower === "content-length" || lower === "transfer-encoding") continue;
    if (lower === "host" && value.toLowerCase() === host) continue;
    headers.push([name, value]);
  }
  return { method, headers, body: raw.subarray(body) };
}

const utf8 = (b: Uint8Array) => {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(b);
  } catch {
    return null;
  }
};

/** A Python string literal (double quotes). */
function pyString(s: string): string {
  // JSON string syntax is valid Python string syntax.
  return JSON.stringify(s);
}

/** A Python bytes literal. */
function pyBytes(b: Uint8Array): string {
  let out = 'b"';
  for (const c of b) {
    if (c === 0x5c) out += "\\\\";
    else if (c === 0x22) out += '\\"';
    else if (c === 0x0a) out += "\\n";
    else if (c === 0x0d) out += "\\r";
    else if (c === 0x09) out += "\\t";
    else if (c >= 0x20 && c < 0x7f) out += String.fromCharCode(c);
    else out += `\\x${c.toString(16).padStart(2, "0")}`;
  }
  return `${out}"`;
}

/** Python using `requests`. */
export function toPython(url: string, raw: Uint8Array): string {
  const r = parseRequest(url, raw);
  const lines = [
    "import requests",
    "",
    "response = requests.request(",
    `    ${pyString(r.method)},`,
    `    ${pyString(url)},`,
  ];
  if (r.headers.length > 0) {
    lines.push("    headers={");
    for (const [n, v] of r.headers) lines.push(`        ${pyString(n)}: ${pyString(v)},`);
    lines.push("    },");
  }
  if (r.body.length > 0) lines.push(`    data=${pyBytes(r.body)},`);
  lines.push(
    "    allow_redirects=False,",
    ")",
    "print(response.status_code)",
    "print(response.text)",
  );
  return `${lines.join("\n")}\n`;
}

/** JavaScript using `fetch` (Node.js 18+ or Deno; browsers drop some headers). */
export function toFetch(url: string, raw: Uint8Array): string {
  const r = parseRequest(url, raw);
  const lines = [
    "// Browsers refuse headers such as Cookie or Host in fetch; run it in Node.js 18+ or Deno.",
    `const response = await fetch(${JSON.stringify(url)}, {`,
    `  method: ${JSON.stringify(r.method)},`,
  ];
  if (r.headers.length > 0) {
    lines.push("  headers: [");
    for (const [n, v] of r.headers) lines.push(`    [${JSON.stringify(n)}, ${JSON.stringify(v)}],`);
    lines.push("  ],");
  }
  if (r.body.length > 0) {
    const text = utf8(r.body);
    lines.push(
      text !== null
        ? `  body: ${JSON.stringify(text)},`
        : `  body: new Uint8Array([${Array.from(r.body).join(", ")}]),`,
    );
  }
  lines.push(
    '  redirect: "manual",',
    "});",
    "console.log(response.status, await response.text());",
  );
  return `${lines.join("\n")}\n`;
}
