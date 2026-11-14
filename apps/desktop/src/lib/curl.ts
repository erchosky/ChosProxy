/**
 * "Copy as curl": a bash/zsh command (binary bodies use `$'…'` quoting) that sends the same request with
 * curl. Header order, the method and the body are kept; curl computes
 * Content-Length itself and takes the host from the URL.
 */

const SAFE = /^[A-Za-z0-9_\-./:=@%+,]+$/;
// Printable text plus tab and newlines can go between single quotes.
// eslint-disable-next-line no-control-regex
const NEEDS_ESCAPES = /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]|�/;

/** Quotes one argument for sh, bash and zsh. */
export function shellQuote(value: string): string {
  if (value !== "" && SAFE.test(value)) return value;
  if (!NEEDS_ESCAPES.test(value)) return `'${value.replace(/'/g, `'\\''`)}'`;
  return ansiQuote(new TextEncoder().encode(value));
}

/** `$'…'` quoting with `\xHH` for every byte that is not plain ASCII. */
function ansiQuote(bytes: Uint8Array): string {
  let out = "$'";
  for (const b of bytes) {
    if (b === 0x5c) out += "\\\\";
    else if (b === 0x27) out += "\\'";
    else if (b === 0x0a) out += "\\n";
    else if (b === 0x0d) out += "\\r";
    else if (b === 0x09) out += "\\t";
    else if (b >= 0x20 && b < 0x7f) out += String.fromCharCode(b);
    else out += `\\x${b.toString(16).padStart(2, "0")}`;
  }
  return `${out}'`;
}

function bodyArg(body: Uint8Array): string {
  const text = new TextDecoder("utf-8", { fatal: false }).decode(body);
  return NEEDS_ESCAPES.test(text) ? ansiQuote(body) : shellQuote(text);
}

function headEnd(raw: Uint8Array): { head: number; body: number } {
  for (let i = 0; i + 1 < raw.length; i++) {
    if (raw[i] === 0x0a && raw[i + 1] === 0x0a) return { head: i, body: i + 2 };
    if (raw[i] === 0x0d && raw[i + 1] === 0x0a && raw[i + 2] === 0x0d && raw[i + 3] === 0x0a)
      return { head: i, body: i + 4 };
  }
  return { head: raw.length, body: raw.length };
}

/** The curl command for a stored request sent to `url`. */
export function toCurl(url: string, raw: Uint8Array): string {
  const { head, body: bodyStart } = headEnd(raw);
  const lines = new TextDecoder("utf-8", { fatal: false })
    .decode(raw.subarray(0, head))
    .split(/\r?\n/);
  const [method = "GET", , version = "HTTP/1.1"] = (lines[0] ?? "").split(" ");
  const body = raw.subarray(bodyStart);
  let host = "";
  try {
    host = new URL(url).host;
  } catch {
    // Keep every header when the URL cannot be read.
  }

  // One line per option, flag and value together.
  const args = [`curl --path-as-is ${version === "HTTP/1.0" ? "--http1.0" : "--http1.1"}`];
  if (method === "HEAD") args.push("--head");
  else if (method !== "GET" || body.length > 0) args.push(`-X ${shellQuote(method)}`);
  let compressed = false;
  for (const line of lines.slice(1)) {
    const colon = line.indexOf(":");
    if (colon <= 0) continue;
    const name = line.slice(0, colon).trim();
    const value = line.slice(colon + 1).trim();
    const lower = name.toLowerCase();
    if (lower === "content-length" || lower === "transfer-encoding") continue;
    if (lower === "host" && value.toLowerCase() === host.toLowerCase()) continue;
    if (lower === "accept-encoding") compressed = true;
    args.push(`-H ${shellQuote(`${name}: ${value}`)}`);
  }
  if (compressed) args.push("--compressed");
  if (body.length > 0) args.push(`--data-binary ${bodyArg(body)}`);
  args.push(shellQuote(url));
  return args.join(" \\\n  ");
}
