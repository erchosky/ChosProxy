/** The protocol a request's text says it uses (its request line). */
export type Protocol = "HTTP/1.1" | "HTTP/2";

export const PROTOCOLS: { id: Protocol; label: string }[] = [
  { id: "HTTP/1.1", label: "HTTP/1.1" },
  { id: "HTTP/2", label: "HTTP/2 (own connection; parallel groups as a single packet)" },
];

function firstLine(text: string): [string, string] {
  const i = text.indexOf("\n");
  return i < 0 ? [text, ""] : [text.slice(0, i), text.slice(i)];
}

/** "HTTP/2" when the request line ends with HTTP/2, otherwise "HTTP/1.1". */
export function requestProtocol(text: string): Protocol {
  const [line] = firstLine(text);
  return /\sHTTP\/2\r?$/.test(line) ? "HTTP/2" : "HTTP/1.1";
}

/** The request with its request line's version set to `protocol`; a line
 * without a version gets one. Everything else is kept as typed. */
export function withProtocol(text: string, protocol: Protocol): string {
  const [line, rest] = firstLine(text);
  const cr = line.endsWith("\r") ? "\r" : "";
  const bare = cr ? line.slice(0, -1) : line;
  const m = /^(.*\S)\s+HTTP\/[0-9.]+$/.exec(bare);
  const start = m ? m[1] : bare.trimEnd();
  if (!start) return text;
  return `${start} ${protocol}${cr}${rest}`;
}
