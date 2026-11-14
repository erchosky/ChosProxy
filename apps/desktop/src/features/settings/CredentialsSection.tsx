import { useEffect, useState } from "react";
import type { CredentialInfo } from "../../bindings/CredentialInfo";
import { Button, IconButton } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { scriptId } from "../../lib/scriptTemplates";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { KeyringProtectionCard } from "./KeyringProtectionCard";

/** Settings → Credentials: how the keyring is protected, then its entries
 * (read again whenever the protection or the lock changes). */
export function KeyringArea() {
  const [version, setVersion] = useState(0);
  return (
    <>
      <KeyringProtectionCard onChanged={() => setVersion((v) => v + 1)} />
      <CredentialsSection key={version} />
    </>
  );
}

/** Settings → Credentials: the encrypted keyring (secrets are write-only). */
export function CredentialsSection() {
  const [list, setList] = useState<CredentialInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<CredentialInfo | null>(null);
  const [secret, setSecret] = useState("");
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    api.credentials().then(
      (l) => alive && setList(l),
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
  }, []);

  async function save() {
    if (!editing) return;
    try {
      setList(await api.credentialSet(editing, secret === "" ? null : secret));
      setEditing(null);
      setSecret("");
      notify("info", "Credential saved (encrypted).");
    } catch (e) {
      notify("error", `Not saved: ${toIpcError(e).message}`);
    }
  }

  async function remove(c: CredentialInfo) {
    const ok = await dialogs.confirm(`Delete “${c.label}”?`, "Delete credential?", "Delete");
    if (!ok) return;
    try {
      setList(await api.credentialDelete(c.id));
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }

  if (error) return <ErrorState title="Could not open the keyring" message={error} />;
  if (!list) return <LoadingState />;
  return (
    <section className="card" aria-label="Credentials">
      <h2>Credentials</h2>
      <p className="muted rule-hint">
        User names and secrets for platform authentication and login macros, encrypted on this
        computer. A secret cannot be shown again after it is saved; type a new one to replace it.
      </p>
      {list.length > 0 && (
        <ul className="rule-list">
          {list.map((c) => (
            <li key={c.id} className="rule-row">
              <div className="rule-fields">
                <strong>{c.label}</strong>
                <span className="muted">
                  {c.username || "(no user name)"} · {c.hasSecret ? "secret stored" : "no secret"} ·
                  id <code>{c.id}</code>
                </span>
              </div>
              <div className="rule-actions">
                <IconButton
                  icon="settings"
                  label={`Edit ${c.label}`}
                  onClick={() => {
                    setEditing(c);
                    setSecret("");
                  }}
                />
                <IconButton
                  icon="trash"
                  label={`Delete ${c.label}`}
                  onClick={() => void remove(c)}
                />
              </div>
            </li>
          ))}
        </ul>
      )}
      {editing ? (
        <div className="rule-row">
          <div className="rule-fields">
            <label>
              Name
              <input
                type="text"
                value={editing.label}
                onChange={(e) => setEditing({ ...editing, label: e.target.value })}
              />
            </label>
            <label>
              User name
              <input
                type="text"
                autoComplete="off"
                value={editing.username}
                onChange={(e) => setEditing({ ...editing, username: e.target.value })}
              />
            </label>
            <label>
              Secret
              <input
                type="password"
                autoComplete="new-password"
                placeholder={editing.hasSecret ? "unchanged" : ""}
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
              />
            </label>
          </div>
          <div className="form-actions">
            <span className="spacer" />
            <Button variant="ghost" onClick={() => setEditing(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={editing.label.trim() === ""}
              onClick={() => void save()}
            >
              Save
            </Button>
          </div>
        </div>
      ) : (
        <div className="form-actions">
          <Button
            size="sm"
            icon="plus"
            onClick={() =>
              setEditing({
                id: scriptId(
                  "credential",
                  list.map((c) => c.id),
                ),
                label: "",
                username: "",
                hasSecret: false,
              })
            }
          >
            Add credential
          </Button>
        </div>
      )}
    </section>
  );
}
