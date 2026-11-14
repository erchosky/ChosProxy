import { useEffect, useState } from "react";
import type { KeyringProtection } from "../../bindings/KeyringProtection";
import type { KeyringStatus } from "../../bindings/KeyringStatus";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

export const PROTECTION_LABELS: Record<KeyringProtection, string> = {
  file: "A key file on this computer (default)",
  osKeychain: "The operating system's keychain",
  password: "A master password (asked once per session)",
};

/** Why a protection cannot be chosen with what is typed; null when it can. */
export function protectionProblem(
  status: KeyringStatus,
  target: KeyringProtection,
  password: string,
  repeat: string,
): string | null {
  if (status.locked) return "Unlock the keyring first.";
  if (target === "osKeychain" && !status.osKeychainAvailable)
    return "The operating system's keychain is not supported on this system yet.";
  if (target === "password") {
    if ([...password].length < 8) return "The master password needs at least 8 characters.";
    if (password !== repeat) return "The two passwords are not the same.";
  }
  if (target === status.protection && target !== "password") return "Already in use.";
  return null;
}

/** Settings → Credentials: what protects the keyring key, and unlocking. */
export function KeyringProtectionCard({ onChanged }: { onChanged(): void }) {
  const [status, setStatus] = useState<KeyringStatus | null>(null);
  const [target, setTarget] = useState<KeyringProtection>("file");
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [unlock, setUnlock] = useState("");
  const [busy, setBusy] = useState(false);
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    api.keyringStatus().then(
      (s) => {
        if (!alive) return;
        setStatus(s);
        setTarget(s.protection);
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);

  if (!status) return null;

  async function run(action: () => Promise<KeyringStatus>, done: string) {
    setBusy(true);
    try {
      const next = await action();
      setStatus(next);
      setTarget(next.protection);
      setPassword("");
      setRepeat("");
      setUnlock("");
      notify("info", done);
      onChanged();
    } catch (e) {
      notify("error", toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  const problem = protectionProblem(status, target, password, repeat);
  return (
    <section className="card" aria-label="Keyring protection">
      <h2>Keyring protection</h2>
      <div className="dialog-row">
        <Badge tone={status.locked ? "warn" : "ok"}>{status.locked ? "locked" : "open"}</Badge>
        <span className="muted">{PROTECTION_LABELS[status.protection]}</span>
        <span className="spacer" />
        {status.protection === "password" && !status.locked && (
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void run(api.keyringLock, "Keyring locked.")}
          >
            Lock now
          </Button>
        )}
      </div>
      {status.locked ? (
        <div className="dialog-row">
          <input
            type="password"
            aria-label="Master password"
            placeholder="Master password"
            value={unlock}
            onChange={(e) => setUnlock(e.target.value)}
            onKeyDown={(e) =>
              e.key === "Enter" &&
              unlock !== "" &&
              void run(() => api.keyringUnlock(unlock), "Keyring unlocked.")
            }
          />
          <Button
            size="sm"
            variant="primary"
            disabled={busy || unlock === ""}
            onClick={() => void run(() => api.keyringUnlock(unlock), "Keyring unlocked.")}
          >
            Unlock
          </Button>
        </div>
      ) : (
        <>
          <label className="inline-field">
            Keep the key in
            <select value={target} onChange={(e) => setTarget(e.target.value as KeyringProtection)}>
              {(Object.keys(PROTECTION_LABELS) as KeyringProtection[]).map((p) => (
                <option
                  key={p}
                  value={p}
                  disabled={p === "osKeychain" && !status.osKeychainAvailable}
                >
                  {PROTECTION_LABELS[p]}
                </option>
              ))}
            </select>
          </label>
          {target === "password" && (
            <div className="dialog-row">
              <input
                type="password"
                aria-label="New master password"
                placeholder="New master password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
              <input
                type="password"
                aria-label="Repeat the master password"
                placeholder="Repeat it"
                value={repeat}
                onChange={(e) => setRepeat(e.target.value)}
              />
            </div>
          )}
          {target === "password" && (
            <p className="callout callout-warn">
              The password is not stored anywhere. If you forget it, the saved credentials cannot be
              read again. Rules that use a credential do not work while the keyring is locked.
            </p>
          )}
          <div className="form-actions">
            <span className="muted">{problem !== "Already in use." ? problem : ""}</span>
            <span className="spacer" />
            <Button
              size="sm"
              disabled={busy || problem !== null}
              onClick={() =>
                void run(
                  () => api.keyringSetProtection(target, target === "password" ? password : null),
                  "Keyring protection changed; the credentials were kept.",
                )
              }
            >
              {busy ? "Changing…" : "Apply"}
            </Button>
          </div>
        </>
      )}
    </section>
  );
}
