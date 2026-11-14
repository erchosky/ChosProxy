// Structured view of an HTTP request in text form (query parameters, form
// body parameters, cookies, headers) and rewriting the text from an edited
// view. Only the sections that changed are rebuilt; untouched parameters keep
// their exact original encoding.

export interface Param {
  name: string;
  value: string;
  /** JSON fields: where the value is in the document. */
  path?: (string | number)[];
  /** JSON fields: the value was a JSON string (edits stay strings). */
  jsonString?: boolean;
  /** Exact text on the wire; dropped when the parameter is edited. */
  raw?: { name: string; value: string; hasEquals: boolean };
  /** Multipart and XML fields: where the value is in the body (UTF-16
   * offsets), so an edit replaces only it. */
  range?: [number, number];
  /** XML fields: the value is written with XML escapes. */
  xml?: boolean;
  /** Shown but not editable here (multipart file contents). */
  fixed?: boolean;
}

export interface Header {
  name: string;
  value: string;
}

export interface Inspection {
  method: string;
  /** Request target without the query string. */
  path: string;
  version: string;
  query: Param[];
  headers: Header[];
  cookies: Param[];
  /** Form body parameters; null when the body is not `x-www-form-urlencoded`. */
  form: Param[] | null;
  /** Content type of the body (lower case, without parameters), if any. */
  bodyType: string | null;
  /** JSON or multipart body fields; JSON values can be edited in place. */
  bodyFields: Param[] | null;
}

/** At most this many JSON or multipart fields are listed. */
export const MAX_FIELDS = 500;

/**
 * JSON leaves as `a.b[0].c` = value (strings unquoted, others as JSON);
 * null if the body is not JSON.
 */
export function jsonFields(body: string): Param[] | null {
  let doc: unknown;
  try {
    doc = JSON.parse(body);
  } catch {
    return null;
  }
  const out: Param[] = [];
  const walk = (v: unknown, path: string, at: (string | number)[]) => {
    if (out.length >= MAX_FIELDS) return;
    if (Array.isArray(v)) {
      if (v.length === 0) out.push({ name: path || "(root)", value: "[]", path: at });
      v.forEach((x, i) => walk(x, `${path}[${i}]`, [...at, i]));
    } else if (v !== null && typeof v === "object") {
      const entries = Object.entries(v);
      if (entries.length === 0) out.push({ name: path || "(root)", value: "{}", path: at });
      for (const [k, x] of entries) walk(x, path ? `${path}.${k}` : k, [...at, k]);
    } else {
      out.push({
        name: path || "(root)",
        value: typeof v === "string" ? v : JSON.stringify(v),
        path: at,
        jsonString: typeof v === "string",
      });
    }
  };
  walk(doc, "", []);
  return out;
}

/** Fields of a `multipart/form-data` body; files show their name and size
 * (and cannot be edited here); other values carry their range. */
export function multipartFields(body: string, contentType: string): Param[] | null {
  const m = /boundary=(?:"([^"]+)"|([^;\s]+))/i.exec(contentType);
  const boundary = m?.[1] ?? m?.[2];
  if (!boundary) return null;
  const delimiter = `--${boundary}`;
  const out: Param[] = [];
  let at = body.indexOf(delimiter);
  while (at !== -1 && out.length < MAX_FIELDS) {
    const partStart = at + delimiter.length;
    if (body.startsWith("--", partStart)) break;
    const next = body.indexOf(delimiter, partStart);
    if (next === -1) break;
    const part = body.slice(partStart, next);
    const sep = part.indexOf("\r\n\r\n") !== -1 ? "\r\n\r\n" : "\n\n";
    const headEnd = part.indexOf(sep);
    if (headEnd !== -1) {
      const head = part.slice(0, headEnd);
      const valueStart = partStart + headEnd + sep.length;
      let valueEnd = next;
      if (body.slice(valueStart, valueEnd).endsWith("\r\n")) valueEnd -= 2;
      else if (body.slice(valueStart, valueEnd).endsWith("\n")) valueEnd -= 1;
      valueEnd = Math.max(valueStart, valueEnd);
      const value = body.slice(valueStart, valueEnd);
      const name = /name="([^"]*)"/i.exec(head)?.[1] ?? "(unnamed)";
      const file = /filename="([^"]*)"/i.exec(head)?.[1];
      out.push(
        file === undefined
          ? { name, value, range: [valueStart, valueEnd] }
          : {
              name,
              value: `file ${file} (${utf8Length(value).toLocaleString()} bytes)`,
              fixed: true,
            },
      );
    }
    at = next;
  }
  return out;
}

const XML_UNESCAPES: Record<string, string> = {
  "&lt;": "<",
  "&gt;": ">",
  "&amp;": "&",
  "&quot;": '"',
  "&apos;": "'",
};

function xmlUnescape(s: string): string {
  return s.replace(/&(lt|gt|amp|quot|apos);/g, (e) => XML_UNESCAPES[e] ?? e);
}

/** XML escapes for a value written into an element or attribute. */
export function xmlEscape(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

/** Text of leaf elements (`<a>text</a>` as `a`) and attribute values (as
 * `element@attr`) of an XML body, with their ranges; null if none. */
export function xmlFields(body: string): Param[] | null {
  const out: Param[] = [];
  const tag = /<([A-Za-z_][\w:.-]*)((?:\s+[^<>]*?)?)(\/?)>/g;
  let m: RegExpExecArray | null;
  while ((m = tag.exec(body)) !== null && out.length < MAX_FIELDS) {
    const [whole, name = "", attrs = "", selfClosing] = m;
    const attrStart = m.index + 1 + name.length;
    const attr = /([\w:.-]+)\s*=\s*(["'])(.*?)\2/g;
    let a: RegExpExecArray | null;
    while ((a = attr.exec(attrs)) !== null && out.length < MAX_FIELDS) {
      const raw = a[3] ?? "";
      const valueStart = attrStart + a.index + a[0].length - raw.length - 1;
      if (a[1]?.startsWith("xmlns")) continue;
      out.push({
        name: `${name}@${a[1] ?? ""}`,
        value: xmlUnescape(raw),
        range: [valueStart, valueStart + raw.length],
        xml: true,
      });
    }
    if (selfClosing) continue;
    const textStart = m.index + whole.length;
    const close = `</${name}>`;
    const textEnd = body.indexOf("<", textStart);
    if (textEnd !== -1 && body.startsWith(close, textEnd)) {
      const raw = body.slice(textStart, textEnd);
      const lead = raw.length - raw.trimStart().length;
      const trimmed = raw.trim();
      if (trimmed !== "") {
        out.push({
          name,
          value: xmlUnescape(trimmed),
          range: [textStart + lead, textStart + lead + trimmed.length],
          xml: true,
        });
      }
    }
  }
  return out.length > 0 ? out : null;
}

/** Writes changed values of ranged fields (multipart, XML) into `body`;
 * everything else stays byte for byte. */
export function applyRangedFields(body: string, before: Param[], after: Param[]): string {
  const changes: { start: number; end: number; text: string }[] = [];
  after.forEach((p, i) => {
    const old = before[i];
    if (!old?.range || old.fixed || old.value === p.value) return;
    changes.push({
      start: old.range[0],
      end: old.range[1],
      text: old.xml ? xmlEscape(p.value) : p.value,
    });
  });
  changes.sort((a, b) => b.start - a.start);
  let out = body;
  for (const c of changes) out = out.slice(0, c.start) + c.text + out.slice(c.end);
  return out;
}

/** A cookie set by a response, with its attributes as written. */
export interface SetCookieRow {
  name: string;
  value: string;
  attributes: string;
}

export interface ResponseInspection {
  status: string;
  headers: Header[];
  cookies: SetCookieRow[];
}

/** Headers and Set-Cookie cookies of a response in text form. */
export function inspectResponse(text: string): ResponseInspection | null {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const end = text.indexOf(eol + eol);
  const lines = (end === -1 ? text : text.slice(0, end)).split(eol);
  const status = lines[0] ?? "";
  if (!/^HTTP\/\d/.test(status)) return null;
  const headers = lines
    .slice(1)
    .filter((l) => l !== "")
    .map(parseHeader);
  const cookies = headers
    .filter((h) => h.name.toLowerCase() === "set-cookie")
    .map((h) => {
      const semi = h.value.indexOf(";");
      const pair = semi === -1 ? h.value : h.value.slice(0, semi);
      const eq = pair.indexOf("=");
      return {
        name: (eq === -1 ? pair : pair.slice(0, eq)).trim(),
        value: eq === -1 ? "" : pair.slice(eq + 1).trim(),
        attributes: semi === -1 ? "" : h.value.slice(semi + 1).trim(),
      };
    });
  return { status, headers, cookies };
}

const utf8Length = (s: string) => new TextEncoder().encode(s).length;

interface Parts {
  eol: string;
  requestLine: string;
  headerLines: string[];
  body: string;
}

function split(text: string): Parts | null {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const sep = eol + eol;
  const end = text.indexOf(sep);
  const head = end === -1 ? text.replace(new RegExp(`${eol}$`), "") : text.slice(0, end);
  const body = end === -1 ? "" : text.slice(end + sep.length);
  const lines = head.split(eol);
  const requestLine = lines[0] ?? "";
  if (!/^[A-Za-z]+ \S+ HTTP\/\d/.test(requestLine)) return null;
  return { eol, requestLine, headerLines: lines.slice(1), body };
}

/** Decodes `application/x-www-form-urlencoded` / query text; invalid escapes stay as they are. */
export function decodeComponent(s: string): string {
  const plus = s.replace(/\+/g, " ");
  try {
    return decodeURIComponent(plus);
  } catch {
    return plus;
  }
}

/** Encodes a query or form component (spaces as `+`). */
export function encodeComponent(s: string): string {
  return encodeURIComponent(s).replace(/%20/g, "+");
}

function parsePairs(text: string, separator: RegExp): Param[] {
  if (text === "") return [];
  return text.split(separator).map((pair) => {
    const eq = pair.indexOf("=");
    const rawName = eq === -1 ? pair : pair.slice(0, eq);
    const rawValue = eq === -1 ? "" : pair.slice(eq + 1);
    return {
      name: decodeComponent(rawName),
      value: decodeComponent(rawValue),
      raw: { name: rawName, value: rawValue, hasEquals: eq !== -1 },
    };
  });
}

function unchanged(p: Param): boolean {
  return (
    p.raw !== undefined &&
    decodeComponent(p.raw.name) === p.name &&
    decodeComponent(p.raw.value) === p.value
  );
}

function serializePairs(params: Param[], joiner: string): string {
  return params
    .map((p) => {
      if (unchanged(p) && p.raw) {
        return p.raw.hasEquals ? `${p.raw.name}=${p.raw.value}` : p.raw.name;
      }
      return `${encodeComponent(p.name)}=${encodeComponent(p.value)}`;
    })
    .join(joiner);
}

function parseHeader(line: string): Header {
  const colon = line.indexOf(":");
  if (colon === -1) return { name: line, value: "" };
  return { name: line.slice(0, colon).trim(), value: line.slice(colon + 1).trim() };
}

function headerValue(headers: Header[], name: string): string | null {
  const h = headers.find((x) => x.name.toLowerCase() === name);
  return h ? h.value : null;
}

function parseCookies(headers: Header[]): Param[] {
  return headers
    .filter((h) => h.name.toLowerCase() === "cookie")
    .flatMap((h) =>
      h.value
        .split(/;\s*/)
        .filter((c) => c !== "")
        .map((c) => {
          const eq = c.indexOf("=");
          // Cookie values are opaque: shown and written as they are.
          return eq === -1
            ? { name: c, value: "" }
            : { name: c.slice(0, eq).trim(), value: c.slice(eq + 1).trim() };
        }),
    );
}

/** Parses a request in text form; null if it is not a request. */
export function inspectRequest(text: string): Inspection | null {
  const parts = split(text);
  if (!parts) return null;
  const [method = "", target = "", version = ""] = parts.requestLine.split(" ");
  const q = target.indexOf("?");
  const headers = parts.headerLines.filter((l) => l !== "").map(parseHeader);
  const contentType = headerValue(headers, "content-type");
  const bodyType = contentType ? (contentType.split(";")[0] ?? "").trim().toLowerCase() : null;
  const isForm = bodyType === "application/x-www-form-urlencoded";
  return {
    method,
    path: q === -1 ? target : target.slice(0, q),
    version,
    query: q === -1 ? [] : parsePairs(target.slice(q + 1), /&/),
    headers,
    cookies: parseCookies(headers),
    form: isForm ? parsePairs(parts.body, /&/) : null,
    bodyType,
    bodyFields:
      bodyType === "multipart/form-data" && contentType
        ? multipartFields(parts.body, contentType)
        : bodyType !== null && (bodyType.endsWith("/json") || bodyType.endsWith("+json"))
          ? jsonFields(parts.body)
          : bodyType !== null && (bodyType.endsWith("/xml") || bodyType.endsWith("+xml"))
            ? xmlFields(parts.body)
            : null,
  };
}

/**
 * Rewrites `text` so it matches `edited` (from {@link inspectRequest} of the
 * same text, then modified). Sections that did not change keep their exact
 * original text; Content-Length follows a changed form body.
 */
export function applyInspection(text: string, edited: Inspection): string {
  const parts = split(text);
  const original = inspectRequest(text);
  if (!parts || !original) return text;
  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

  let requestLine = parts.requestLine;
  if (!same(original.query, edited.query) || original.path !== edited.path) {
    const query = serializePairs(edited.query, "&");
    const target = query === "" ? edited.path : `${edited.path}?${query}`;
    requestLine = `${original.method} ${target} ${original.version}`;
  }

  let headers = edited.headers;
  if (!same(original.cookies, edited.cookies)) {
    const cookie = edited.cookies
      .map((c) => (c.value === "" && !c.name.includes("=") ? c.name : `${c.name}=${c.value}`))
      .join("; ");
    const withoutCookies = headers.filter((h) => h.name.toLowerCase() !== "cookie");
    const at = headers.findIndex((h) => h.name.toLowerCase() === "cookie");
    if (cookie !== "") {
      const insertAt = at === -1 ? withoutCookies.length : at;
      withoutCookies.splice(insertAt, 0, { name: "Cookie", value: cookie });
    }
    headers = withoutCookies;
  }

  let body = parts.body;
  const isJson = (t: string | null) => t !== null && (t.endsWith("/json") || t.endsWith("+json"));
  let bodyChanged = false;
  if (edited.form !== null && original.form !== null && !same(original.form, edited.form)) {
    body = serializePairs(edited.form, "&");
    bodyChanged = true;
  } else if (
    isJson(original.bodyType) &&
    edited.bodyFields !== null &&
    original.bodyFields !== null &&
    !same(original.bodyFields, edited.bodyFields)
  ) {
    const json = applyJsonFields(parts.body, original.bodyFields, edited.bodyFields);
    if (json !== null) {
      body = json;
      bodyChanged = true;
    }
  } else if (
    edited.bodyFields !== null &&
    original.bodyFields !== null &&
    original.bodyFields.some((f) => f.range) &&
    !same(original.bodyFields, edited.bodyFields)
  ) {
    const next = applyRangedFields(parts.body, original.bodyFields, edited.bodyFields);
    if (next !== parts.body) {
      body = next;
      bodyChanged = true;
    }
  }
  if (bodyChanged) {
    const chunked = (headerValue(headers, "transfer-encoding") ?? "")
      .toLowerCase()
      .includes("chunked");
    if (!chunked) {
      const length = String(utf8Length(body));
      const has = headers.some((h) => h.name.toLowerCase() === "content-length");
      headers = has
        ? headers.map((h) =>
            h.name.toLowerCase() === "content-length" ? { ...h, value: length } : h,
          )
        : [...headers, { name: "Content-Length", value: length }];
    }
  }

  const headerLines = same(headers, original.headers)
    ? parts.headerLines.filter((l) => l !== "")
    : headers.map((h) => `${h.name}: ${h.value}`);
  return [requestLine, ...headerLines].join(parts.eol) + parts.eol + parts.eol + body;
}

/**
 * Applies edited JSON field values to `body`: each changed value goes to its
 * path, as a string when it was one, else as JSON if it parses (numbers,
 * booleans, null, objects) or a string. Returns the re-serialized document
 * (compact), or null if the body no longer parses.
 */
export function applyJsonFields(body: string, before: Param[], after: Param[]): string | null {
  let doc: unknown;
  try {
    doc = JSON.parse(body);
  } catch {
    return null;
  }
  after.forEach((p, i) => {
    const old = before[i];
    if (!old || !p.path || old.value === p.value) return;
    let value: unknown = p.value;
    if (!p.jsonString) {
      try {
        value = JSON.parse(p.value);
      } catch {
        value = p.value;
      }
    }
    if (p.path.length === 0) {
      doc = value;
      return;
    }
    let target: unknown = doc;
    for (const key of p.path.slice(0, -1)) {
      if (target === null || typeof target !== "object") return;
      target = (target as Record<string | number, unknown>)[key];
    }
    const last = p.path[p.path.length - 1];
    if (target !== null && typeof target === "object" && last !== undefined) {
      (target as Record<string | number, unknown>)[last] = value;
    }
  });
  return JSON.stringify(doc);
}
