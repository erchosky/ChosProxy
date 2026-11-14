import { describe, expect, it } from "vitest";
import type { KeyringStatus } from "../../bindings/KeyringStatus";
import { protectionProblem } from "./KeyringProtectionCard";

const open: KeyringStatus = { protection: "file", locked: false, osKeychainAvailable: true };

describe("keyring protection form", () => {
  it("needs a long enough password typed twice", () => {
    expect(protectionProblem(open, "password", "short", "short")).toMatch(/8 characters/);
    expect(protectionProblem(open, "password", "correct horse", "correct hors")).toMatch(/same/);
    expect(protectionProblem(open, "password", "correct horse", "correct horse")).toBeNull();
  });

  it("does not offer what cannot be done", () => {
    expect(protectionProblem(open, "file", "", "")).toBe("Already in use.");
    expect(protectionProblem(open, "osKeychain", "", "")).toBeNull();
    expect(
      protectionProblem({ ...open, osKeychainAvailable: false }, "osKeychain", "", ""),
    ).toMatch(/not supported/);
    expect(protectionProblem({ ...open, locked: true }, "file", "", "")).toMatch(/Unlock/);
    // A new password for a keyring that already has one is a change.
    const pw: KeyringStatus = { ...open, protection: "password" };
    expect(protectionProblem(pw, "password", "another horse", "another horse")).toBeNull();
  });
});
