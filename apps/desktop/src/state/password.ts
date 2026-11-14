import { create } from "zustand";
import type { ProjectEncryption } from "../bindings/ProjectEncryption";
import { api, toIpcError } from "../services/ipc";

/**
 * What the password dialog asks (M31, encrypted projects):
 * - `unlock`: the password of an existing file;
 * - `choose`: whether a new file (new project, Save As, export) is
 *   encrypted, and with which password;
 * - `change`: a new password for the open project.
 */
export type PasswordMode = "unlock" | "choose" | "change";

export interface PasswordRequest {
  mode: PasswordMode;
  title: string;
  message?: string;
  /** For `choose`: encryption ticked at first. */
  encrypt?: boolean;
  /** For `choose`: the project is encrypted and may keep its password. */
  canKeep?: boolean;
}

/** The answer: a password (empty = keep or none), or null when cancelled. */
export interface PasswordAnswer {
  encrypt: boolean;
  password: string;
}

interface PasswordState {
  request: PasswordRequest | null;
  resolve: ((a: PasswordAnswer | null) => void) | null;
  ask(request: PasswordRequest): Promise<PasswordAnswer | null>;
  answer(a: PasswordAnswer | null): void;
}

export const usePasswordStore = create<PasswordState>((set, get) => ({
  request: null,
  resolve: null,
  ask: (request) =>
    new Promise((resolve) => {
      get().resolve?.(null);
      set({ request, resolve });
    }),
  answer: (a) => {
    get().resolve?.(a);
    set({ request: null, resolve: null });
  },
}));

/** Asks for a password; null when cancelled. */
export function askPassword(request: PasswordRequest): Promise<PasswordAnswer | null> {
  return usePasswordStore.getState().ask(request);
}

/**
 * Problems with a password being chosen; null when it is acceptable.
 * Long passphrases are encouraged; the file cannot be opened without it.
 */
export function passwordProblem(password: string, repeat: string): string | null {
  if (password.length < 8) return "Use at least 8 characters (a few words work well).";
  if (/^[xX]'/.test(password)) return "A password cannot start with x'.";
  if (password !== repeat) return "The two passwords differ.";
  return null;
}

/** The encryption a `choose` answer asks for. */
export function encryptionOf(a: PasswordAnswer, canKeep: boolean): ProjectEncryption {
  if (!a.encrypt) return { kind: "none" };
  if (a.password === "" && canKeep) return { kind: "same" };
  return { kind: "password", password: a.password };
}

/**
 * Runs `attempt` without a password first; while the file turns out to be
 * encrypted (or the password is wrong), asks for one and tries again.
 * Null when the tester cancels.
 */
export async function withPassword<T>(
  title: string,
  attempt: (password: string | null) => Promise<T>,
): Promise<T | null> {
  try {
    return await attempt(null);
  } catch (e) {
    let error = toIpcError(e);
    while (error.kind === "encrypted" || error.kind === "wrongPassword") {
      const a = await askPassword({
        mode: "unlock",
        title,
        message:
          error.kind === "wrongPassword"
            ? "That password does not open it. Try again."
            : "This project is encrypted.",
      });
      if (!a) return null;
      try {
        return await attempt(a.password);
      } catch (again) {
        error = toIpcError(again);
      }
    }
    throw error;
  }
}

/**
 * Inspects `path` with `isSealedFile` first; if sealed, prompts for a password
 * before running `attempt`, and retries while the password is wrong.
 * Returns null when the tester cancels.
 */
export async function withSealedFilePassword<T>(
  path: string,
  title: string,
  attempt: (password: string | null) => Promise<T>,
): Promise<T | null> {
  let initialPw: string | null = null;
  if (typeof api.isSealedFile === "function") {
    try {
      const sealed = await api.isSealedFile(path);
      if (sealed) {
        const a = await askPassword({
          mode: "unlock",
          title,
          message: "This export file is encrypted with a password.",
        });
        if (!a) return null;
        initialPw = a.password;
      }
    } catch {
      // Fall back to direct attempt + withPassword retry loop.
    }
  }
  try {
    return await attempt(initialPw);
  } catch (e) {
    let error = toIpcError(e);
    while (error.kind === "encrypted" || error.kind === "wrongPassword") {
      const a = await askPassword({
        mode: "unlock",
        title,
        message:
          error.kind === "wrongPassword"
            ? "That password does not open it. Try again."
            : "This export file is encrypted with a password.",
      });
      if (!a) return null;
      try {
        return await attempt(a.password);
      } catch (again) {
        error = toIpcError(again);
      }
    }
    throw error;
  }
}
