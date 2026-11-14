/** The GraphQL operation carried by a request body (a JSON object, or the
 * first of a batch, with a `query` string), or null. */
export function graphqlParts(
  body: string | null,
): { query: string; variables: string; operationName: string | null; batch: number } | null {
  if (!body) return null;
  let value: unknown;
  try {
    value = JSON.parse(body);
  } catch {
    return null;
  }
  const list = Array.isArray(value) ? value : [value];
  const first: unknown = list[0];
  if (typeof first !== "object" || first === null) return null;
  const op = first as Record<string, unknown>;
  if (typeof op.query !== "string") return null;
  const head = op.query.trimStart();
  if (!/^(\{|query\b|mutation\b|subscription\b|fragment\b)/.test(head)) return null;
  return {
    query: formatGraphql(op.query),
    variables:
      op.variables === undefined || op.variables === null
        ? "{}"
        : JSON.stringify(op.variables, null, 2),
    operationName: typeof op.operationName === "string" ? op.operationName : null,
    batch: list.length,
  };
}

/** Indents a GraphQL document by its braces (strings are left alone). */
export function formatGraphql(source: string): string {
  let out = "";
  let depth = 0;
  let inString = false;
  let line = "";
  const flush = () => {
    const trimmed = line.trim();
    if (trimmed) out += `${"  ".repeat(depth)}${trimmed}\n`;
    line = "";
  };
  for (let i = 0; i < source.length; i++) {
    const c = source[i] ?? "";
    if (inString) {
      line += c;
      if (c === "\\") {
        line += source[i + 1] ?? "";
        i++;
      } else if (c === '"') {
        inString = false;
      }
      continue;
    }
    if (c === '"') {
      inString = true;
      line += c;
    } else if (c === "{") {
      line += " {";
      flush();
      depth++;
    } else if (c === "}") {
      flush();
      depth = Math.max(0, depth - 1);
      line = "}";
      flush();
    } else if (c === "\n" || c === ",") {
      if (c === ",") line += ",";
      if (c === "\n" || depth > 0) flush();
    } else {
      line += c;
    }
  }
  flush();
  return out.replace(/\s+\{/g, " {").trimEnd();
}

/**
 * A GraphQL request body with another query and variables (the first
 * operation of a batch; every other field is kept). Null when the body is
 * not a GraphQL operation; an error text when the variables are not a JSON
 * object.
 */
export function withGraphql(
  body: string,
  query: string,
  variables: string,
): { body: string } | { error: string } | null {
  if (graphqlParts(body) === null) return null;
  let vars: unknown;
  try {
    vars = variables.trim() === "" ? {} : JSON.parse(variables);
  } catch {
    return { error: "The variables are not valid JSON." };
  }
  if (typeof vars !== "object" || vars === null || Array.isArray(vars))
    return { error: "The variables must be a JSON object." };
  const value: unknown = JSON.parse(body);
  const patch = (op: unknown) => ({ ...(op as Record<string, unknown>), query, variables: vars });
  const next = Array.isArray(value) ? [patch(value[0]), ...value.slice(1)] : patch(value);
  return { body: JSON.stringify(next) };
}
