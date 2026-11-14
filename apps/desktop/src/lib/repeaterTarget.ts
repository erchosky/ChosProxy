// Repeater targets as text: "https://host:port".
import type { RepeaterTarget } from "../bindings/RepeaterTarget";

const DEFAULT_PORT: Record<string, number> = { http: 80, https: 443 };

/** `https://host` or `http://host:8080` (default ports left out). */
export function formatTarget(t: RepeaterTarget): string {
  const host = t.host.includes(":") ? `[${t.host}]` : t.host;
  return t.port === DEFAULT_PORT[t.scheme]
    ? `${t.scheme}://${host}`
    : `${t.scheme}://${host}:${t.port}`;
}

/** Parses what the user typed. A missing scheme means http (https for
 * port 443); a path after the authority is ignored. */
export function parseTarget(text: string): RepeaterTarget | { error: string } {
  let rest = text.trim();
  if (!rest) return { error: "Enter a target such as https://example.com" };
  let scheme = "";
  const m = /^([a-z][a-z0-9+.-]*):\/\//i.exec(rest);
  if (m?.[1]) {
    scheme = m[1].toLowerCase();
    if (scheme !== "http" && scheme !== "https") {
      return { error: "The protocol must be http or https" };
    }
    rest = rest.slice(m[0].length);
  }
  const authority = rest.split(/[/?#]/, 1)[0] ?? "";
  let host: string;
  let portText: string | undefined;
  if (authority.startsWith("[")) {
    const close = authority.indexOf("]");
    if (close < 0) return { error: "Missing ] after the IPv6 address" };
    host = authority.slice(1, close);
    const after = authority.slice(close + 1);
    if (after && !after.startsWith(":")) return { error: "Invalid host" };
    portText = after ? after.slice(1) : undefined;
  } else {
    const parts = authority.split(":");
    if (parts.length > 2) return { error: "Put IPv6 addresses in brackets: [::1]" };
    host = parts[0] ?? "";
    portText = parts[1];
  }
  host = host.toLowerCase();
  if (!host || /[\s@]/.test(host)) return { error: "Invalid host" };
  let port: number;
  if (portText === undefined || portText === "") {
    if (!scheme) scheme = "http";
    port = scheme === "https" ? 443 : 80;
  } else {
    if (!/^\d+$/.test(portText)) return { error: "The port must be a number" };
    port = Number(portText);
    if (port < 1 || port > 65535) return { error: "The port must be 1–65535" };
    if (!scheme) scheme = port === 443 ? "https" : "http";
  }
  return { scheme, host, port };
}

/** Target of a full URL, e.g. the URL of a paused request. */
export function targetOfUrl(url: string): RepeaterTarget | null {
  const parsed = parseTarget(url);
  return "error" in parsed ? null : parsed;
}
