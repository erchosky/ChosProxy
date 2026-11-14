import { useState } from "react";
import type { RecoveryStatus } from "../bindings/RecoveryStatus";
import { formatBytes, formatDateTime } from "../lib/format";
import { useAppStore } from "../state/app";
import { Badge } from "./Badge";
import { Button } from "./Button";
import { Switch } from "./Switch";

/** Startup crash recovery prompt (M31 A, PROJECT-004, spec §118). */
export function RecoveryDialog() {
  const recovery = useAppStore((s) => s.recovery);
  if (!recovery) return null;
  return <Dialog key={recovery.markerPath} recovery={recovery} />;
}

function Dialog({ recovery }: { recovery: RecoveryStatus }) {
  const busy = useAppStore((s) => s.busy);
  const restoreRecovery = useAppStore((s) => s.restoreRecovery);
  const openRecoveryReadOnly = useAppStore((s) => s.openRecoveryReadOnly);
  const discardRecovery = useAppStore((s) => s.discardRecovery);
  const dismissRecovery = useAppStore((s) => s.dismissRecovery);
  const [deleteOrphanTemp, setDeleteOrphanTemp] = useState(
    recovery.projectKind === "temporary" && recovery.ephemeralKey,
  );

  const isTemp = !recovery.markerCorrupt && recovery.projectKind === "temporary";

  return (
    <div className="palette-backdrop" role="presentation" onClick={dismissRecovery}>
      <div
        className="dialog dialog-form"
        role="dialog"
        aria-modal="true"
        aria-label="Recover previous session"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && dismissRecovery()}
      >
        <header className="dialog-head">
          <strong>Recover previous session</strong>
          {!recovery.markerCorrupt && (
            <div className="dialog-row">
              <Badge tone={recovery.projectKind === "temporary" ? "warn" : "ok"}>
                {recovery.projectKind}
              </Badge>
              {recovery.encrypted && <Badge tone="ok">encrypted</Badge>}
            </div>
          )}
        </header>

        <p className="callout callout-warn">{recovery.message}</p>

        {!recovery.markerCorrupt && (
          <div className="field-stack">
            <div>
              <strong>{recovery.projectName}</strong>
              <div className="mono muted selectable">{recovery.projectPath}</div>
            </div>
            <div className="muted">
              {recovery.fileExists
                ? `Size: ${formatBytes(recovery.fileBytes)}`
                : "Project file not found on disk"}
              {recovery.updatedAt > 0 && ` · Last active ${formatDateTime(recovery.updatedAt)}`}
            </div>
          </div>
        )}

        {recovery.interruptedTasks.length > 0 && (
          <div className="field-stack">
            <strong>Interrupted tasks ({recovery.interruptedTasks.length})</strong>
            <ul className="compact-list">
              {recovery.interruptedTasks.map((t) => (
                <li key={t.id}>
                  <span>{t.name}</span>{" "}
                  <Badge tone={t.kind === "live" ? "ok" : "warn"}>
                    {t.kind === "live" ? "live (resumes)" : `${t.kind} (will not restart)`}
                  </Badge>
                </li>
              ))}
            </ul>
          </div>
        )}

        {isTemp && recovery.fileExists && (
          <Switch
            label="Delete the orphaned temporary project file when discarding recovery"
            checked={deleteOrphanTemp}
            onChange={setDeleteOrphanTemp}
          />
        )}

        <div className="form-actions">
          <Button variant="ghost" disabled={busy !== null} onClick={dismissRecovery}>
            Cancel
          </Button>
          <span className="spacer" />
          <Button
            variant="danger"
            disabled={busy !== null}
            onClick={() => void discardRecovery(isTemp && deleteOrphanTemp)}
          >
            Discard recovery
          </Button>
          {recovery.restorable && (
            <>
              <Button disabled={busy !== null} onClick={() => void openRecoveryReadOnly()}>
                Open read-only
              </Button>
              <Button
                variant="primary"
                disabled={busy !== null}
                onClick={() => void restoreRecovery()}
              >
                Restore
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
