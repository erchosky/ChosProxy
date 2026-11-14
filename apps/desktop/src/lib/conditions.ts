/** History conditions (the condition builder), turned into a filter script
 * that the engine runs in the ChosScripts sandbox. */

export type ConditionField =
  | "url"
  | "method"
  | "status"
  | "mime"
  | "requestHeader"
  | "responseHeader"
  | "requestBody"
  | "responseBody"
  | "request"
  | "response"
  | "responseLength";

export type ConditionOp =
  | "contains"
  | "notContains"
  | "equals"
  | "notEquals"
  | "matches"
  | "exists"
  | "missing"
  | "greater"
  | "less";

export interface Condition {
  field: ConditionField;
  /** Header name, for header fields. */
  name: string;
  op: ConditionOp;
  value: string;
}

export interface Conditions {
  /** All conditions must hold, or any one. */
  match: "all" | "any";
  items: Condition[];
}

export const FIELDS: { id: ConditionField; label: string; numeric?: boolean; header?: boolean }[] =
  [
    { id: "url", label: "URL" },
    { id: "method", label: "Method" },
    { id: "status", label: "Status", numeric: true },
    { id: "mime", label: "MIME type" },
    { id: "requestHeader", label: "Request header", header: true },
    { id: "responseHeader", label: "Response header", header: true },
    { id: "requestBody", label: "Request body" },
    { id: "responseBody", label: "Response body" },
    { id: "request", label: "Whole request" },
    { id: "response", label: "Whole response" },
    { id: "responseLength", label: "Response length", numeric: true },
  ];

const TEXT_OPS: { id: ConditionOp; label: string }[] = [
  { id: "contains", label: "contains" },
  { id: "notContains", label: "does not contain" },
  { id: "equals", label: "is" },
  { id: "notEquals", label: "is not" },
  { id: "matches", label: "matches regex" },
];
const HEADER_OPS: { id: ConditionOp; label: string }[] = [
  ...TEXT_OPS,
  { id: "exists", label: "is present" },
  { id: "missing", label: "is missing" },
];
const NUMBER_OPS: { id: ConditionOp; label: string }[] = [
  { id: "equals", label: "=" },
  { id: "notEquals", label: "≠" },
  { id: "greater", label: ">" },
  { id: "less", label: "<" },
];

/** The operators a field offers. */
export function opsFor(field: ConditionField): { id: ConditionOp; label: string }[] {
  const f = FIELDS.find((x) => x.id === field);
  if (f?.numeric) return NUMBER_OPS;
  if (f?.header) return HEADER_OPS;
  return TEXT_OPS;
}

export function newCondition(): Condition {
  return { field: "responseBody", name: "", op: "contains", value: "" };
}

/** A Rhai string literal. */
function lit(s: string): string {
  return JSON.stringify(s);
}

function subject(c: Condition): string {
  switch (c.field) {
    case "url":
      return "msg.url";
    case "method":
      return "msg.method";
    case "status":
      return "msg.status";
    case "mime":
      return "msg.mime";
    case "requestHeader":
      return `msg.request_header(${lit(c.name)})`;
    case "responseHeader":
      return `msg.response_header(${lit(c.name)})`;
    case "requestBody":
      return "msg.request_body";
    case "responseBody":
      return "msg.response_body";
    case "request":
      return "msg.request";
    case "response":
      return "msg.response";
    case "responseLength":
      return "msg.response.len()";
  }
}

/** Why a condition is incomplete, or null. */
export function conditionProblem(c: Condition): string | null {
  const f = FIELDS.find((x) => x.id === c.field);
  if (f?.header && c.name.trim() === "") return "Enter the header name";
  if (c.op === "exists" || c.op === "missing") return null;
  if (f?.numeric && !/^-?\d+$/.test(c.value.trim())) return "Enter a whole number";
  if (c.op === "matches" && c.value === "") return "Enter a regular expression";
  return null;
}

function expression(c: Condition): string {
  const s = subject(c);
  const f = FIELDS.find((x) => x.id === c.field);
  if (f?.numeric) {
    const n = String(parseInt(c.value.trim(), 10));
    const op = { equals: "==", notEquals: "!=", greater: ">", less: "<" }[c.op as string] ?? "==";
    return `${s} ${op} ${n}`;
  }
  const v = lit(c.value);
  switch (c.op) {
    case "contains":
      return `contains_ci(${s}, ${v})`;
    case "notContains":
      return `!contains_ci(${s}, ${v})`;
    case "equals":
      return `${s}.to_lower() == ${v}.to_lower()`;
    case "notEquals":
      return `${s}.to_lower() != ${v}.to_lower()`;
    case "matches":
      return `regex_match(${s}, ${v})`;
    case "exists":
      return `${s} != ""`;
    case "missing":
      return `${s} == ""`;
    default:
      return "true";
  }
}

/** The filter script for `conditions` (true for every row when empty). */
export function conditionsScript(conditions: Conditions): string {
  const parts = conditions.items.map((c) => `(${expression(c)})`);
  const body =
    parts.length === 0 ? "true" : parts.join(conditions.match === "all" ? " && " : " || ");
  return `// Made by the condition builder.\nfn filter(msg) {\n    ${body}\n}\n`;
}

/** A one-line summary of the conditions. */
export function describeConditions(conditions: Conditions): string {
  const one = (c: Condition) => {
    const f = FIELDS.find((x) => x.id === c.field);
    const op = opsFor(c.field).find((o) => o.id === c.op)?.label ?? c.op;
    const what = f?.header ? `${c.name}` : (f?.label ?? c.field);
    return c.op === "exists" || c.op === "missing" ? `${what} ${op}` : `${what} ${op} ${c.value}`;
  };
  return conditions.items.map(one).join(conditions.match === "all" ? " and " : " or ");
}
