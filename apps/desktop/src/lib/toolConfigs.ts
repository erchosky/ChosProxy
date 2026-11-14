/** Tools with a settings library (M31b). */
export type ConfigTool = "crawler" | "discovery" | "fuzzer";

/** What is about one run, not a setting: never saved, never loaded. */
const NOT_SETTINGS: Record<ConfigTool, string[]> = {
  crawler: ["startUrls"],
  discovery: ["baseUrl"],
  fuzzer: ["target", "template", "headCrlf", "bodyCrlf"],
};

/** The settings of a tool's form, without its target or request. */
export function settingsOf(tool: ConfigTool, config: object): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(config)) {
    if (!NOT_SETTINGS[tool].includes(key)) out[key] = value;
  }
  return out;
}

function sameShape(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) return Array.isArray(a) && Array.isArray(b);
  if (a === null || b === null) return a === b;
  return typeof a === typeof b;
}

/**
 * The part of a saved configuration that can go into the form: only
 * settings the form has, of the same kind of value (a file written by hand
 * or by another version cannot put anything else there). Null when the
 * text is not a JSON object.
 */
export function loadableSettings(
  tool: ConfigTool,
  current: object,
  json: string,
): Record<string, unknown> | null {
  let saved: unknown;
  try {
    saved = JSON.parse(json);
  } catch {
    return null;
  }
  if (typeof saved !== "object" || saved === null || Array.isArray(saved)) return null;
  const now = current as Record<string, unknown>;
  const patch: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(saved)) {
    if (NOT_SETTINGS[tool].includes(key) || !(key in now)) continue;
    if (sameShape(now[key], value)) patch[key] = value;
  }
  return patch;
}
