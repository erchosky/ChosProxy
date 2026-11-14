import { useState } from "react";
import { type PasswordRequest, passwordProblem, usePasswordStore } from "../state/password";
import { Button } from "./Button";
import { Switch } from "./Switch";

/** The password dialog (mounted once at the app root). */
export function PasswordDialog() {
  const request = usePasswordStore((s) => s.request);
  if (!request) return null;
  return <Dialog key={JSON.stringify(request)} request={request} />;
}

function Dialog({ request }: { request: PasswordRequest }) {
  const answer = usePasswordStore((s) => s.answer);
  const [encrypt, setEncrypt] = useState(request.mode !== "choose" || (request.encrypt ?? false));
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const choosing = request.mode !== "unlock" && encrypt;
  const keeping = request.mode === "choose" && request.canKeep && encrypt && password === "";
  const problem =
    request.mode === "unlock"
      ? password === ""
        ? "Type the password."
        : null
      : choosing && !keeping
        ? passwordProblem(password, repeat)
        : null;
  const cancel = () => answer(null);
  const ok = () => {
    if (problem) return;
    answer({ encrypt: request.mode === "unlock" || encrypt, password });
  };
  return (
    <div className="palette-backdrop" role="presentation" onClick={cancel}>
      <form
        className="dialog dialog-form"
        role="dialog"
        aria-modal="true"
        aria-label={request.title}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && cancel()}
        onSubmit={(e) => {
          e.preventDefault();
          ok();
        }}
      >
        <header className="dialog-head">
          <strong>{request.title}</strong>
        </header>
        {request.message && <p>{request.message}</p>}
        {request.mode === "choose" && (
          <Switch label="Encrypt with a password" checked={encrypt} onChange={setEncrypt} />
        )}
        {(request.mode === "unlock" || encrypt) && (
          <>
            <label className="field-stack">
              {request.mode === "unlock" ? "Password" : "New password"}
              <input
                type="password"
                autoFocus
                autoComplete={request.mode === "unlock" ? "current-password" : "new-password"}
                value={password}
                placeholder={
                  request.mode === "choose" && request.canKeep ? "Empty: keep the current one" : ""
                }
                onChange={(e) => setPassword(e.target.value)}
              />
            </label>
            {request.mode !== "unlock" && !keeping && (
              <label className="field-stack">
                Repeat it
                <input
                  type="password"
                  autoComplete="new-password"
                  value={repeat}
                  onChange={(e) => setRepeat(e.target.value)}
                />
              </label>
            )}
          </>
        )}
        {request.mode !== "unlock" && encrypt && !keeping && (
          <p className="callout callout-warn">
            ChosProxy does not keep this password and cannot recover it. Without it the project
            cannot be opened.
          </p>
        )}
        {problem && password !== "" && <p className="muted">{problem}</p>}
        <div className="form-actions">
          <span className="spacer" />
          <Button variant="ghost" onClick={cancel}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" disabled={problem !== null}>
            {request.mode === "unlock" ? "Open" : "OK"}
          </Button>
        </div>
      </form>
    </div>
  );
}
