// Diffs for the Comparer: Myers' O((N+M)·D) algorithm over lines, words or
// bytes, plus a structured comparison of two HTTP messages.

import { inspectRequest, type Param } from "./inspector";

export type DiffOp = { kind: "equal" | "delete" | "insert"; tokens: string[] };

export type DiffMode = "line" | "word" | "byte";

/** Inputs larger than this many tokens are not diffed token by token. */
export const MAX_TOKENS = 200_000;
/** Edit distance at which the comparison gives up on details. */
export const MAX_EDITS = 20_000;

export interface DiffResult {
  ops: DiffOp[];
  /** True when the inputs were too large or too different for a detailed diff. */
  coarse: boolean;
  added: number;
  removed: number;
}

export function tokenize(text: string, mode: DiffMode): string[] {
  if (mode === "line") return text === "" ? [] : text.split(/(?<=\n)/);
  if (mode === "word") return text.match(/\s+|[^\s]+/g) ?? [];
  return Array.from(new TextEncoder().encode(text), (b) => b.toString(16).padStart(2, "0"));
}

/** Typed-array read; indices are always in range here. */
const at = (arr: Int32Array, i: number): number => arr[i] ?? 0;
const tok = (arr: string[], i: number): string => arr[i] ?? "";

function push(ops: DiffOp[], kind: DiffOp["kind"], token: string) {
  const last = ops[ops.length - 1];
  if (last && last.kind === kind) last.tokens.push(token);
  else ops.push({ kind, tokens: [token] });
}

/** Myers diff of two token lists. */
export function diffTokens(a: string[], b: string[]): DiffResult {
  const n = a.length;
  const m = b.length;
  const coarse = (): DiffResult => {
    const ops: DiffOp[] = [];
    if (n) ops.push({ kind: "delete", tokens: [...a] });
    if (m) ops.push({ kind: "insert", tokens: [...b] });
    return { ops, coarse: true, added: m, removed: n };
  };
  if (n + m > MAX_TOKENS) return coarse();

  // Trim the common prefix and suffix: cheap and very common.
  let start = 0;
  while (start < n && start < m && a[start] === b[start]) start++;
  let endA = n;
  let endB = m;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const A = a.slice(start, endA);
  const B = b.slice(start, endB);
  const N = A.length;
  const M = B.length;
  const max = Math.min(N + M, MAX_EDITS);
  const offset = max + 1;
  const v = new Int32Array(2 * max + 3);
  const trace: Int32Array[] = [];
  let found = N === 0 && M === 0;
  for (let d = 0; d <= max && !found; d++) {
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && at(v, offset + k - 1) < at(v, offset + k + 1))
          ? at(v, offset + k + 1)
          : at(v, offset + k - 1) + 1;
      let y = x - k;
      while (x < N && y < M && A[x] === B[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= N && y >= M) {
        found = true;
        break;
      }
    }
  }
  if (!found) return coarse();
  trace.push(v.slice());

  // Backtrack to build the edit script (in reverse).
  const reversed: { kind: DiffOp["kind"]; token: string }[] = [];
  let x = N;
  let y = M;
  for (let d = trace.length - 2; d >= 0 && (x > 0 || y > 0); d--) {
    const vd = trace[d] ?? v;
    const k = x - y;
    const prevK =
      k === -d || (k !== d && at(vd, offset + k - 1) < at(vd, offset + k + 1)) ? k + 1 : k - 1;
    const prevX = d === 0 ? 0 : at(vd, offset + prevK);
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      reversed.push({ kind: "equal", token: tok(A, x - 1) });
      x--;
      y--;
    }
    if (d > 0) {
      if (x === prevX) reversed.push({ kind: "insert", token: tok(B, y - 1) });
      else reversed.push({ kind: "delete", token: tok(A, x - 1) });
    }
    x = prevX;
    y = prevY;
  }
  while (x > 0 && y > 0) {
    reversed.push({ kind: "equal", token: tok(A, x - 1) });
    x--;
    y--;
  }

  const ops: DiffOp[] = [];
  for (const t of a.slice(0, start)) push(ops, "equal", t);
  let added = 0;
  let removed = 0;
  for (const e of reversed.reverse()) {
    if (e.kind === "insert") added++;
    if (e.kind === "delete") removed++;
    push(ops, e.kind, e.token);
  }
  for (const t of a.slice(endA)) push(ops, "equal", t);
  return { ops, coarse: false, added, removed };
}

export function diffText(a: string, b: string, mode: DiffMode): DiffResult {
  return diffTokens(tokenize(a, mode), tokenize(b, mode));
}

// ---------------------------------------------------------------------------
// Structured HTTP comparison

export interface FieldChange {
  name: string;
  /** null = absent on that side. */
  a: string | null;
  b: string | null;
}

export interface HttpDiff {
  firstLine: FieldChange | null;
  headers: FieldChange[];
  query: FieldChange[];
  cookies: FieldChange[];
  form: FieldChange[];
  sizes: { a: number; b: number; bodyA: number; bodyB: number };
  body: DiffResult;
  identical: boolean;
}

function splitHttp(text: string): { first: string; headers: [string, string][]; body: string } {
  const normalized = text.replace(/\r\n/g, "\n");
  const end = normalized.indexOf("\n\n");
  const head = end === -1 ? normalized : normalized.slice(0, end);
  const body = end === -1 ? "" : normalized.slice(end + 2);
  const [first = "", ...lines] = head.split("\n");
  const headers = lines
    .filter((l) => l.includes(":"))
    .map((l): [string, string] => {
      const i = l.indexOf(":");
      return [l.slice(0, i).trim(), l.slice(i + 1).trim()];
    });
  return { first, headers, body };
}

/** Compares name → values lists; repeated names are compared in order. */
function compareFields(
  a: [string, string][],
  b: [string, string][],
  caseless: boolean,
): FieldChange[] {
  const key = (n: string) => (caseless ? n.toLowerCase() : n);
  const group = (list: [string, string][]) => {
    const m = new Map<string, { name: string; values: string[] }>();
    for (const [n, v] of list) {
      const g = m.get(key(n)) ?? { name: n, values: [] };
      g.values.push(v);
      m.set(key(n), g);
    }
    return m;
  };
  const ga = group(a);
  const gb = group(b);
  const out: FieldChange[] = [];
  for (const k of new Set([...ga.keys(), ...gb.keys()])) {
    const x = ga.get(k);
    const y = gb.get(k);
    const count = Math.max(x?.values.length ?? 0, y?.values.length ?? 0);
    for (let i = 0; i < count; i++) {
      const va = x?.values[i] ?? null;
      const vb = y?.values[i] ?? null;
      if (va !== vb) out.push({ name: (x ?? y)?.name ?? k, a: va, b: vb });
    }
  }
  return out;
}

const pairs = (ps: Param[] | null | undefined): [string, string][] =>
  (ps ?? []).map((p) => [p.name, p.value]);

const byteLength = (s: string) => new TextEncoder().encode(s).length;

/** Structured comparison of two HTTP messages (requests or responses). */
export function diffHttp(a: string, b: string): HttpDiff {
  const x = splitHttp(a);
  const y = splitHttp(b);
  const ia = inspectRequest(a);
  const ib = inspectRequest(b);
  const firstLine = x.first === y.first ? null : { name: "first line", a: x.first, b: y.first };
  const body = diffText(x.body, y.body, "line");
  const result: HttpDiff = {
    firstLine,
    headers: compareFields(x.headers, y.headers, true),
    query: compareFields(pairs(ia?.query), pairs(ib?.query), false),
    cookies: compareFields(pairs(ia?.cookies), pairs(ib?.cookies), false),
    form: compareFields(pairs(ia?.form), pairs(ib?.form), false),
    sizes: {
      a: byteLength(a),
      b: byteLength(b),
      bodyA: byteLength(x.body),
      bodyB: byteLength(y.body),
    },
    body,
    identical: a === b,
  };
  return result;
}
