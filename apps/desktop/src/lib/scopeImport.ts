// Turns a pasted list of targets (one per line) into scope rules.
import type { ScopeRule } from "../bindings/ScopeRule";

export interface ScopeImport {
  rules: ScopeRule[];
  /** Lines that are not a host or URL. */
  skipped: string[];
}

const HOST = /^(\*\.)?([a-z0-9-]+(\.[a-z0-9-]+)*|\[[0-9a-f:]+\]|\d+\.\d+\.\d+\.\d+)$/i;

/**
 * Accepts `example.com`, `*.example.com`, `https://example.com:8443/app`
 * and the like; comments (`#`) and blank lines are ignored.
 */
export function parseScopeList(text: string): ScopeImport {
  const rules: ScopeRule[] = [];
  const skipped: string[] = [];
  const seen = new Set<string>();
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(line) ? line : `any://${line}`;
    let protocol: ScopeRule["protocol"] = "any";
    let host = "";
    let port: number | null = null;
    let path = "";
    const m = /^([a-z][a-z0-9+.-]*):\/\/([^/?#]+)([^?#]*)/i.exec(withScheme);
    if (!m) {
      skipped.push(line);
      continue;
    }
    const scheme = (m[1] ?? "").toLowerCase();
    if (scheme === "http" || scheme === "https") protocol = scheme;
    else if (scheme !== "any") {
      skipped.push(line);
      continue;
    }
    let authority = m[2] ?? "";
    const portMatch = /:(\d{1,5})$/.exec(authority);
    if (portMatch && !authority.endsWith("]")) {
      port = Number(portMatch[1]);
      authority = authority.slice(0, -portMatch[0].length);
    }
    host = authority.toLowerCase();
    if (!HOST.test(host) || (port !== null && (port < 1 || port > 65535))) {
      skipped.push(line);
      continue;
    }
    const includeSubdomains = host.startsWith("*.");
    if (includeSubdomains) host = host.slice(2);
    const p = m[3] ?? "";
    path = p === "/" ? "" : p;
    const key = `${protocol}|${host}|${includeSubdomains}|${port}|${path}`;
    if (seen.has(key)) continue;
    seen.add(key);
    rules.push({
      enabled: true,
      protocol,
      host,
      includeSubdomains,
      port,
      path,
      regex: false,
      comment: "imported",
    });
  }
  return { rules, skipped };
}

/** `existing` plus the imported rules that are not already there. */
export function mergeRules(existing: ScopeRule[], imported: ScopeRule[]): ScopeRule[] {
  const key = (r: ScopeRule) =>
    `${r.protocol}|${r.host}|${r.includeSubdomains}|${r.port}|${r.path}|${r.regex}`;
  const have = new Set(existing.map(key));
  return [...existing, ...imported.filter((r) => !have.has(key(r)))];
}
