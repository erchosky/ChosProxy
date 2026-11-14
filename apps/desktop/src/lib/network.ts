import type { HostOverride } from "../bindings/HostOverride";
import type { MatchReplaceRule } from "../bindings/MatchReplaceRule";
import type { MatchTarget } from "../bindings/MatchTarget";
import type { UpstreamKind } from "../bindings/UpstreamKind";
import type { UpstreamRule } from "../bindings/UpstreamRule";

export const UPSTREAM_KINDS: { id: UpstreamKind; label: string }[] = [
  { id: "http", label: "HTTP proxy" },
  { id: "socks5", label: "SOCKS5 proxy" },
  { id: "direct", label: "Direct (no proxy)" },
];

export const MATCH_TARGETS: { id: MatchTarget; label: string }[] = [
  { id: "requestHeader", label: "Request header" },
  { id: "requestBody", label: "Request body" },
  { id: "requestFirstLine", label: "Request line" },
  { id: "responseHeader", label: "Response header" },
  { id: "responseBody", label: "Response body" },
  { id: "responseFirstLine", label: "Status line" },
  { id: "requestScript", label: "Request script" },
  { id: "responseScript", label: "Response script" },
  { id: "wsToServer", label: "WebSocket message to server" },
  { id: "wsToClient", label: "WebSocket message to client" },
  { id: "wsScript", label: "WebSocket script" },
];

/** True for rules that run a rewrite script (its id is in `match`). */
export function isScriptTarget(t: MatchTarget): boolean {
  return t === "requestScript" || t === "responseScript" || t === "wsScript";
}

/** `list` with item `i` moved `delta` places (clamped). */
export function moveItem<T>(list: readonly T[], i: number, delta: number): T[] {
  const j = Math.min(Math.max(i + delta, 0), list.length - 1);
  if (i === j || i < 0 || i >= list.length) return [...list];
  const out = [...list];
  const [item] = out.splice(i, 1);
  out.splice(j, 0, item as T);
  return out;
}

const HOST_PATTERN = /^(\*\.)?[A-Za-z0-9._:-]+$/;

export function isHostPattern(p: string): boolean {
  return HOST_PATTERN.test(p) && !p.slice(2).includes("*") && p.length <= 253;
}

/** A problem with an upstream rule, for quick feedback (the engine
 * validates again on save). */
export function upstreamProblem(r: UpstreamRule): string | null {
  if (r.host !== "*" && !isHostPattern(r.host)) return "Hosts: *, example.com or *.example.com";
  if (r.kind === "direct") return null;
  if (!/^[A-Za-z0-9._:-]+$/.test(r.proxyHost)) return "Enter the proxy's host name or address";
  if (!Number.isInteger(r.proxyPort) || r.proxyPort < 1 || r.proxyPort > 65535) {
    return "Proxy port: 1–65535";
  }
  if (r.kind === "http" && r.username.includes(":")) return "The user name cannot contain “:”";
  return null;
}

const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;

export function overrideProblem(h: HostOverride): string | null {
  if (!isHostPattern(h.host)) return "Host: example.com or *.example.com";
  if (!IPV4.test(h.address) && !/^[0-9a-fA-F:]+:[0-9a-fA-F:.]*$/.test(h.address)) {
    return "Enter an IPv4 or IPv6 address";
  }
  return null;
}

export function newUpstreamRule(): UpstreamRule {
  return {
    enabled: true,
    host: "*",
    kind: "http",
    proxyHost: "127.0.0.1",
    proxyPort: 8081,
    username: "",
    password: "",
    remoteDns: true,
  };
}

export function newMatchRule(): MatchReplaceRule {
  return {
    enabled: true,
    target: "requestHeader",
    match: "^User-Agent:.*$",
    replace: "User-Agent: ChosProxy",
    regex: true,
    comment: "",
  };
}

/** A problem with a Match & Replace rule the engine would also reject. */
export function matchProblem(r: MatchReplaceRule): string | null {
  if (isScriptTarget(r.target)) return r.match ? null : "Choose a rewrite script";
  const header = r.target === "requestHeader" || r.target === "responseHeader";
  if (!r.match && !header) return "Enter the text to find";
  if (!r.match && !r.replace) return "Enter a header to add, or a text to find";
  const multiline = r.target.endsWith("Body") || r.target.startsWith("ws");
  if (!multiline && /[\r\n]/.test(r.replace)) {
    return "Line breaks are only allowed in body replacements";
  }
  return null;
}

/** One line describing what a rule does. */
export function describeMatchRule(r: MatchReplaceRule): string {
  const where = MATCH_TARGETS.find((t) => t.id === r.target)?.label ?? r.target;
  if (isScriptTarget(r.target)) return `${where}: runs the script “${r.match}”`;
  if (!r.match) return `${where}: add “${r.replace}”`;
  if (!r.replace) {
    return r.target.endsWith("Header")
      ? `${where}: remove lines matching ${r.match}`
      : `${where}: remove ${r.match}`;
  }
  return `${where}: ${r.match} → ${r.replace}`;
}
