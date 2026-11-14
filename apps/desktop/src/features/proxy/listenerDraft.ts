import type { ListenerConfig } from "../../bindings/ListenerConfig";
import type { ProxyAuth } from "../../bindings/ProxyAuth";

/** Editable form state for a listener (port kept as text while typing). */
export interface ListenerDraft {
  id: string;
  bindAddress: string;
  port: string;
  startOnOpen: boolean;
  allowRemote: boolean;
  /** Stored credentials, kept as they are (changed through their own form). */
  auth: ProxyAuth | null;
  /** Also serve clients that do not know they use a proxy. */
  invisible: boolean;
  /** Destination port of TLS from invisible clients (text while typing). */
  invisibleTlsPort: string;
  /** Offer HTTP/2 to clients of intercepted HTTPS. */
  http2: boolean;
  /** Invisible traffic goes to this host:port (empty = off). */
  redirect: string;
  /** Certificate shown to clients of intercepted HTTPS. */
  certificate: "shared" | "ownCa" | "file";
  /** PEM files of a chosen certificate and its key. */
  certFile: string;
  keyFile: string;
}

export function toDraft(c: ListenerConfig): ListenerDraft {
  return {
    id: c.id,
    bindAddress: c.bind_address,
    port: String(c.port),
    startOnOpen: c.start_on_open,
    allowRemote: c.allow_remote,
    auth: c.auth,
    invisible: c.invisible,
    invisibleTlsPort: String(c.invisible_tls_port),
    http2: c.http2,
    redirect: c.redirect,
    certificate: c.certificate.kind,
    certFile: c.certificate.kind === "file" ? c.certificate.cert : "",
    keyFile: c.certificate.kind === "file" ? c.certificate.key : "",
  };
}

const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;

function isLoopback(address: string): boolean {
  return address.startsWith("127.") || address === "::1";
}

/** Field errors for a draft, keyed by field name. Mirrors server-side
 * validation for fast feedback; the engine re-validates on save. */
export function validateDraft(
  d: ListenerDraft,
  all: ListenerDraft[],
): Partial<Record<keyof ListenerDraft, string>> {
  const errors: Partial<Record<keyof ListenerDraft, string>> = {};
  if (!/^[a-z0-9-]{1,32}$/.test(d.id)) errors.id = "1–32 characters: a-z, 0-9, -";
  else if (all.filter((o) => o.id === d.id).length > 1) errors.id = "Duplicate id";
  const addr = d.bindAddress.trim();
  if (!IPV4.test(addr) && !/^[0-9a-fA-F:]+$/.test(addr)) errors.bindAddress = "Enter an IP address";
  else if (!isLoopback(addr) && !d.allowRemote) {
    errors.bindAddress = "Non-loopback address: enable “Allow remote connections” to confirm";
  }
  const port = Number(d.port);
  if (!/^\d+$/.test(d.port) || port < 1 || port > 65535) errors.port = "1–65535";
  const tls = Number(d.invisibleTlsPort);
  if (!/^\d+$/.test(d.invisibleTlsPort) || tls < 1 || tls > 65535) {
    errors.invisibleTlsPort = "1–65535";
  }
  const redirect = d.redirect.trim();
  if (redirect && !/^(\[[0-9a-fA-F:]+\]|[A-Za-z0-9._-]+):\d{1,5}$/.test(redirect)) {
    errors.redirect = "host:port, e.g. 10.0.0.5:443";
  }
  if (d.certificate === "file") {
    if (d.certFile.trim() === "") errors.certFile = "Choose the certificate file (PEM)";
    if (d.keyFile.trim() === "") errors.keyFile = "Choose its private key file (PEM)";
  }
  if (d.invisible && d.auth) {
    errors.invisible = "Invisible clients cannot send proxy credentials: remove them first";
  }
  return errors;
}

export function fromDraft(d: ListenerDraft): ListenerConfig {
  return {
    id: d.id,
    bind_address: d.bindAddress.trim(),
    port: Number(d.port),
    start_on_open: d.startOnOpen,
    allow_remote: d.allowRemote,
    auth: d.auth,
    invisible: d.invisible,
    invisible_tls_port: Number(d.invisibleTlsPort),
    http2: d.http2,
    redirect: d.redirect.trim(),
    certificate:
      d.certificate === "file"
        ? { kind: "file", cert: d.certFile.trim(), key: d.keyFile.trim() }
        : { kind: d.certificate },
  };
}

/** A fresh listener on the next free id/port. */
export function newDraft(existing: ListenerDraft[]): ListenerDraft {
  let n = existing.length + 1;
  while (existing.some((d) => d.id === `listener-${n}`)) n++;
  const ports = existing.map((d) => Number(d.port)).filter(Number.isFinite);
  return {
    id: `listener-${n}`,
    bindAddress: "127.0.0.1",
    port: String(Math.max(8080, ...ports) + 1),
    startOnOpen: false,
    allowRemote: false,
    auth: null,
    invisible: false,
    invisibleTlsPort: "443",
    http2: false,
    redirect: "",
    certificate: "shared",
    certFile: "",
    keyFile: "",
  };
}
