import { describe, expect, it, vi } from "vitest";
import { api, IpcError } from "../services/ipc";
import {
  encryptionOf,
  passwordProblem,
  usePasswordStore,
  withPassword,
  withSealedFilePassword,
} from "./password";

/** Answers the next password dialogs with these passwords (null cancels). */
function answers(...passwords: (string | null)[]) {
  const queue = [...passwords];
  const unsubscribe = usePasswordStore.subscribe((s, prev) => {
    if (!s.request || s.request === prev.request) return;
    const next = queue.shift();
    if (queue.length === 0) unsubscribe();
    queueMicrotask(() =>
      usePasswordStore
        .getState()
        .answer(next === null || next === undefined ? null : { encrypt: true, password: next }),
    );
  });
}

describe("project passwords", () => {
  it("checks new passwords", () => {
    expect(passwordProblem("short", "short")).toMatch(/8 characters/);
    expect(passwordProblem("x'abcdefgh", "x'abcdefgh")).toMatch(/x'/);
    expect(passwordProblem("long enough", "long enougH")).toMatch(/differ/);
    expect(passwordProblem("long enough", "long enough")).toBeNull();
  });

  it("turns a choice into an encryption", () => {
    expect(encryptionOf({ encrypt: false, password: "" }, true)).toEqual({ kind: "none" });
    expect(encryptionOf({ encrypt: true, password: "" }, true)).toEqual({ kind: "same" });
    expect(encryptionOf({ encrypt: true, password: "pw pw pw" }, false)).toEqual({
      kind: "password",
      password: "pw pw pw",
    });
  });

  it("asks for a password only when the file is encrypted, and again when wrong", async () => {
    const tried: (string | null)[] = [];
    const open = async (pw: string | null) => {
      tried.push(pw);
      if (pw === null) throw new IpcError("encrypted", "the project is encrypted");
      if (pw !== "right") throw new IpcError("wrongPassword", "wrong password");
      return "opened";
    };
    answers("wrong", "right");
    await expect(withPassword("Open", open)).resolves.toBe("opened");
    expect(tried).toEqual([null, "wrong", "right"]);

    // Plain files never ask; other errors pass through; cancelling gives null.
    await expect(withPassword("Open", async () => "plain")).resolves.toBe("plain");
    await expect(
      withPassword("Open", async () => {
        throw new IpcError("io", "gone");
      }),
    ).rejects.toThrow("gone");
    answers(null);
    await expect(
      withPassword("Open", async () => {
        throw new IpcError("encrypted", "locked");
      }),
    ).resolves.toBeNull();
  });

  it("inspects sealed files before running attempt and retries on wrong password", async () => {
    const spy = vi.spyOn(api, "isSealedFile").mockResolvedValue(true);
    const tried: (string | null)[] = [];
    const attempt = async (pw: string | null) => {
      tried.push(pw);
      if (pw !== "secret-key") throw new IpcError("wrongPassword", "wrong password");
      return "imported";
    };
    answers("bad-key", "secret-key");
    await expect(
      withSealedFilePassword("/tmp/export.chosorg", "Unlock export", attempt),
    ).resolves.toBe("imported");
    expect(spy).toHaveBeenCalledWith("/tmp/export.chosorg");
    expect(tried).toEqual(["bad-key", "secret-key"]);

    // Cancellation before attempt runs never invokes attempt.
    const neverCalled = vi.fn();
    answers(null);
    await expect(
      withSealedFilePassword("/tmp/export.chosorg", "Unlock export", neverCalled),
    ).resolves.toBeNull();
    expect(neverCalled).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});
