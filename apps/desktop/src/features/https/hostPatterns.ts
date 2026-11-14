/** Splits a textarea into trimmed, non-empty host patterns. */
export function linesToHosts(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l !== "");
}

const HOST = /^[A-Za-z0-9._:-]+$/;

/** Mirrors the engine's validation for quick feedback; the engine re-checks. */
export function validateHostPatterns(text: string): string[] {
  return linesToHosts(text)
    .filter((p) => {
      const host = p.startsWith("*.") ? p.slice(2) : p;
      return host === "" || host.length > 253 || host.includes("*") || !HOST.test(host);
    })
    .map((p) => `Invalid host "${p}": use example.com or *.example.com`);
}

/** Why a regular expression is invalid (JavaScript syntax as an
 * approximation; the engine checks again), or null. */
export function regexProblem(pattern: string): string | null {
  if (pattern.trim() === "") return null;
  try {
    new RegExp(pattern, "i");
    return null;
  } catch (e) {
    return `Invalid regular expression: ${(e as Error).message}`;
  }
}
