import { useState } from "react";
import type { SettingsImportPreview } from "../../bindings/SettingsImportPreview";
import type { SettingsSection } from "../../bindings/SettingsSection";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { diffText } from "../../lib/diff";
import { toggle } from "../../lib/filters";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { askPassword, withSealedFilePassword } from "../../state/password";
import { useScopeStore } from "../../state/scope";

const SECTIONS: { id: SettingsSection; label: string; project: boolean }[] = [
  { id: "app", label: "Application settings", project: false },
  { id: "proxy", label: "Proxy listeners, HTTPS policy and Intercept rules", project: false },
  { id: "scope", label: "Global scope", project: false },
  { id: "projectScope", label: "This project's scope", project: true },
  { id: "projectBackup", label: "This project's backup settings", project: true },
];
const label = (s: SettingsSection) => SECTIONS.find((x) => x.id === s)?.label ?? s;

/** Export chosen settings to JSON (plain or password-sealed); import with validation and a preview. */
export function SettingsTransferCard() {
  const project = useAppStore((s) => s.project);
  const notify = useAppStore((s) => s.notify);
  const [exportSections, setExportSections] = useState<SettingsSection[]>([
    "app",
    "proxy",
    "scope",
  ]);
  const [importing, setImporting] = useState<{
    path: string;
    preview: SettingsImportPreview;
    password: string | null;
  } | null>(null);
  const [chosen, setChosen] = useState<SettingsSection[]>([]);
  const [error, setError] = useState<string | null>(null);

  async function doExport() {
    const path = dialogs.chooseSettingsDestination
      ? await dialogs.chooseSettingsDestination("chosproxy-settings.chossettings")
      : await dialogs.chooseJsonDestination("chosproxy-settings.json");
    if (!path) return;
    const answer = await askPassword({
      mode: "choose",
      title: "Export settings",
      message: "Optionally encrypt this settings export with a password.",
      encrypt: false,
    });
    if (!answer) return;
    try {
      const pw = answer.encrypt ? answer.password : null;
      const saved = await api.exportSettings(path, exportSections, pw);
      notify("info", `Settings exported${pw ? " (encrypted)" : ""} to ${saved}`);
      setError(null);
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }

  async function pickImport() {
    const path = dialogs.chooseSettingsFile
      ? await dialogs.chooseSettingsFile()
      : await dialogs.chooseJsonFile();
    if (!path) return;
    try {
      const res = await withSealedFilePassword(path, "Unlock settings export", async (pw) => ({
        preview: await api.previewSettingsImport(path, pw),
        password: pw,
      }));
      if (!res) return;
      setImporting({ path, preview: res.preview, password: res.password });
      setChosen(res.preview.sections.filter((s) => s.valid && s.changed).map((s) => s.section));
      setError(null);
    } catch (e) {
      setImporting(null);
      setError(toIpcError(e).message);
    }
  }

  async function apply() {
    if (!importing) return;
    try {
      const applied = await api.applySettingsImport(importing.path, chosen, importing.password);
      notify("info", `Imported: ${applied.map(label).join(", ")}`);
      setImporting(null);
      // Settings, proxy configuration and scope may all have changed.
      await useAppStore.getState().reloadConfiguration();
      await useScopeStore.getState().load();
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }

  return (
    <section className="card">
      <h2>Import / export settings</h2>
      <p className="field-help">
        Settings are exported as JSON. Listener passwords are never exported. An import is validated
        and previewed first; only the sections you choose are applied.
      </p>
      <div className="chip-group transfer-sections" role="group" aria-label="Sections to export">
        {SECTIONS.filter((s) => !s.project || project).map((s) => (
          <button
            key={s.id}
            type="button"
            className="chip"
            aria-pressed={exportSections.includes(s.id)}
            onClick={() => setExportSections(toggle(exportSections, s.id))}
          >
            {s.label}
          </button>
        ))}
      </div>
      <div className="form-actions">
        <Button disabled={exportSections.length === 0} onClick={() => void doExport()}>
          Export…
        </Button>
        <Button onClick={() => void pickImport()}>Import…</Button>
      </div>
      {error && <p className="field-error selectable">{error}</p>}
      {importing && (
        <div className="import-preview">
          <h3>
            Preview of <span className="mono">{importing.path}</span>
          </h3>
          {importing.preview.warnings.map((w) => (
            <p key={w} className="callout callout-warn">
              {w}
            </p>
          ))}
          {importing.preview.sections.length === 0 && (
            <p className="muted">The file contains no known sections.</p>
          )}
          {importing.preview.sections.map((s) => {
            const diff = diffText(s.current, s.incoming, "line");
            return (
              <details key={s.section} className="import-section" open={s.changed && s.valid}>
                <summary>
                  <input
                    type="checkbox"
                    aria-label={`Apply ${label(s.section)}`}
                    checked={chosen.includes(s.section)}
                    disabled={!s.valid}
                    onChange={() => setChosen(toggle(chosen, s.section))}
                    onClick={(e) => e.stopPropagation()}
                  />{" "}
                  {label(s.section)}{" "}
                  {!s.valid ? (
                    <Badge tone="danger">invalid</Badge>
                  ) : s.changed ? (
                    <Badge tone="accent">
                      +{diff.added} −{diff.removed}
                    </Badge>
                  ) : (
                    <Badge>unchanged</Badge>
                  )}
                </summary>
                {s.error && <p className="field-error">{s.error}</p>}
                {s.changed && (
                  <pre className="mono diff diff-line">
                    {diff.ops.map((op, i) =>
                      op.kind === "equal" ? (
                        <span key={i}>{op.tokens.join("")}</span>
                      ) : op.kind === "insert" ? (
                        <ins key={i}>{op.tokens.join("")}</ins>
                      ) : (
                        <del key={i}>{op.tokens.join("")}</del>
                      ),
                    )}
                  </pre>
                )}
              </details>
            );
          })}
          <div className="form-actions">
            <Button variant="primary" disabled={chosen.length === 0} onClick={() => void apply()}>
              Apply {chosen.length} section{chosen.length === 1 ? "" : "s"}
            </Button>
            <Button variant="ghost" onClick={() => setImporting(null)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
