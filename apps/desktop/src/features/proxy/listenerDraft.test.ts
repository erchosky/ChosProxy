import { describe, expect, it } from "vitest";
import { fromDraft, newDraft, toDraft, validateDraft, type ListenerDraft } from "./listenerDraft";

const base: ListenerDraft = {
  id: "default",
  bindAddress: "127.0.0.1",
  port: "8080",
  startOnOpen: true,
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

describe("listener drafts", () => {
  it("round-trips config", () => {
    const cfg = {
      id: "a",
      bind_address: "::1",
      port: 9000,
      start_on_open: false,
      allow_remote: false,
      auth: { username: "u", password_hash: "ab", salt: "cd", iterations: 100000 },
      invisible: false,
      invisible_tls_port: 8443,
      http2: true,
      redirect: "10.0.0.5:443",
      certificate: { kind: "shared" as const },
    };
    expect(fromDraft(toDraft(cfg))).toEqual(cfg);
    const chosen = {
      ...cfg,
      certificate: { kind: "file" as const, cert: "/c.pem", key: "/k.pem" },
    };
    expect(fromDraft(toDraft(chosen))).toEqual(chosen);
    const own = { ...cfg, certificate: { kind: "ownCa" as const } };
    expect(fromDraft(toDraft(own))).toEqual(own);
  });

  it("needs both files for a chosen certificate", () => {
    const file = { ...base, certificate: "file" as const, certFile: " ", keyFile: "" };
    const errors = validateDraft(file, [file]);
    expect(errors.certFile).toBeDefined();
    expect(errors.keyFile).toBeDefined();
    const ok = { ...file, certFile: "/c.pem", keyFile: "/k.pem" };
    expect(validateDraft(ok, [ok])).toEqual({});
  });

  it("accepts a valid loopback listener", () => {
    expect(validateDraft(base, [base])).toEqual({});
  });

  it("requires opt-in for non-loopback addresses", () => {
    const exposed = { ...base, bindAddress: "0.0.0.0" };
    expect(validateDraft(exposed, [exposed]).bindAddress).toBeDefined();
    const allowed = { ...exposed, allowRemote: true };
    expect(validateDraft(allowed, [allowed])).toEqual({});
  });

  it("rejects bad ids, ports and duplicates", () => {
    const bad = { ...base, id: "Bad Id", port: "70000" };
    const errors = validateDraft(bad, [bad]);
    expect(errors.id).toBeDefined();
    expect(errors.port).toBeDefined();
    expect(validateDraft(base, [base, base]).id).toBe("Duplicate id");
    expect(validateDraft({ ...base, bindAddress: "localhost" }, [base]).bindAddress).toBeDefined();
  });

  it("creates new drafts on free ids and ports", () => {
    const d = newDraft([base]);
    expect(d.id).toBe("listener-2");
    expect(d.port).toBe("8081");
    expect(d.allowRemote).toBe(false);
  });

  it("checks invisible mode", () => {
    const inv = { ...base, invisible: true, invisibleTlsPort: "0" };
    expect(validateDraft(inv, [inv]).invisibleTlsPort).toBeDefined();
    const withAuth = {
      ...base,
      invisible: true,
      auth: { username: "u", password_hash: "ab", salt: "cd", iterations: 1 },
    };
    expect(validateDraft(withAuth, [withAuth]).invisible).toMatch(/credentials/);
    expect(validateDraft({ ...inv, invisibleTlsPort: "443" }, [inv])).toEqual({});
    expect(validateDraft({ ...base, redirect: "nope" }, [base]).redirect).toBeDefined();
    expect(validateDraft({ ...base, redirect: "[::1]:8443" }, [base])).toEqual({});
  });
});
