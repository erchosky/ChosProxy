import { SessionRulesCard } from "./SessionRulesCard";
import { useEffect, useState } from "react";
import type { BackupSettings } from "../../bindings/BackupSettings";
import type { ProjectSettings } from "../../bindings/ProjectSettings";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Markdown } from "../../components/Markdown";
import { ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { formatBytes, formatDateTime } from "../../lib/format";
import { dialogs } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useProjectStore } from "../../state/project";

export function ProjectView() {
  const project = useAppStore((s) => s.project);
  const settings = useProjectStore((s) => s.settings);
  const error = useProjectStore((s) => s.error);
  const load = useProjectStore((s) => s.load);
  useEffect(() => {
    void load();
  }, [load, project?.path]);
  if (!project) return null;
  if (error)
    return <ErrorState title="Could not load the project" message={error} onRetry={load} />;
  if (!settings) return <LoadingState />;
  return (
    <div className="page project-view">
      <div className="page-header">
        <div>
          <h1>{project.name}</h1>
          <p className="page-sub mono selectable">{project.path}</p>
        </div>
        <div className="dialog-row">
          {project.readOnly && <Badge tone="warn">read-only</Badge>}
          {project.encrypted && <Badge tone="ok">encrypted</Badge>}
          <Badge tone={project.kind === "temporary" ? "warn" : "ok"}>{project.kind}</Badge>
        </div>
      </div>
      <FileCard />
      <BackupCard key={JSON.stringify(settings.backup)} settings={settings} />
      <SessionRulesCard />
      <NotesCard />
      <AuditCard />
    </div>
  );
}

function FileCard() {
  const project = useAppStore((s) => s.project);
  const saveProjectAs = useAppStore((s) => s.saveProjectAs);
  const changePassword = useAppStore((s) => s.changeProjectPassword);
  const busy = useAppStore((s) => s.busy);
  const exportProject = useProjectStore((s) => s.exportProject);
  const [inScopeOnly, setInScopeOnly] = useState(false);
  return (
    <section className="card">
      <h2>Project file</h2>
      <p className="field-help">
        {project?.kind === "temporary"
          ? "This temporary project is deleted when it is closed. Save it as a file to keep it and enable automatic backups."
          : "Everything is saved continuously: history, site map marks, WebSockets, Organizer, scope override, notes and settings."}
      </p>
      <div className="form-actions">
        <Button variant="primary" disabled={busy !== null} onClick={() => void saveProjectAs()}>
          Save as…
        </Button>
        <Button onClick={() => void exportProject(inScopeOnly)}>Export a copy…</Button>
        <Switch label="Only in-scope traffic" checked={inScopeOnly} onChange={setInScopeOnly} />
        {project?.encrypted && project.kind === "persistent" && !project.readOnly && (
          <Button disabled={busy !== null} onClick={() => void changePassword()}>
            Change password…
          </Button>
        )}
      </div>
      <p className="muted">
        {project?.encrypted
          ? project.kind === "temporary"
            ? "Encrypted with a random key that exists only while it is open."
            : "Encrypted with SQLCipher. The password is not stored: without it the file cannot be opened."
          : "Not encrypted. “Save as…” can make an encrypted copy (SQLCipher, AES-256)."}
      </p>
      <p className="muted">
        “Save as” continues in the new file; “Export a copy” writes a file and keeps working here.
      </p>
    </section>
  );
}

function BackupCard({ settings }: { settings: ProjectSettings }) {
  const backups = useProjectStore((s) => s.backups);
  const backingUp = useProjectStore((s) => s.backingUp);
  const { saveBackupSettings, backupNow, restore } = useProjectStore.getState();
  const [draft, setDraft] = useState<BackupSettings>(settings.backup);
  const [error, setError] = useState<string | null>(null);
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings.backup);
  const patch = (p: Partial<BackupSettings>) => setDraft((d) => ({ ...d, ...p }));
  const number = (v: string, fallback: number) => {
    const n = Number(v);
    return Number.isInteger(n) ? n : fallback;
  };
  return (
    <section className="card">
      <div className="card-head">
        <div>
          <h2>Automatic backups</h2>
          <div className="field-help">
            A verified copy is written every interval while the project changes. Old backups are
            removed only after a new one is complete; the newest is never deleted.
          </div>
        </div>
        <Button
          size="sm"
          disabled={!settings.backupsAvailable || backingUp}
          onClick={() => void backupNow()}
          title={settings.backupsAvailable ? undefined : "Save the project as a file first"}
        >
          {backingUp ? "Backing up…" : "Back up now"}
        </Button>
      </div>
      {!settings.backupsAvailable && (
        <p className="callout callout-warn">
          Temporary projects are not backed up. Use “Save as…” first.
        </p>
      )}
      <div className="backup-grid">
        <Switch
          label="Enabled"
          checked={draft.enabled}
          onChange={(enabled) => patch({ enabled })}
        />
        <label>
          Every
          <input
            type="number"
            min={5}
            max={1440}
            value={draft.intervalMinutes}
            onChange={(e) =>
              patch({ intervalMinutes: number(e.target.value, draft.intervalMinutes) })
            }
          />
          minutes
        </label>
        <label>
          Keep
          <input
            type="number"
            min={1}
            max={100}
            value={draft.maxBackups}
            onChange={(e) => patch({ maxBackups: number(e.target.value, draft.maxBackups) })}
          />
          backups
        </label>
        <Switch
          label="Only in-scope traffic"
          checked={draft.inScopeOnly}
          onChange={(inScopeOnly) => patch({ inScopeOnly })}
        />
      </div>
      <div className="backup-dir">
        <span className="field-label">Folder</span>
        <span className="mono selectable">
          {draft.directory ?? settings.defaultBackupDirectory}
        </span>
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            const dir = await dialogs.chooseDirectory(
              draft.directory ?? settings.defaultBackupDirectory,
            );
            if (dir) patch({ directory: dir });
          }}
        >
          Change…
        </Button>
        {draft.directory && (
          <Button size="sm" variant="ghost" onClick={() => patch({ directory: null })}>
            Use default
          </Button>
        )}
      </div>
      {error && <p className="field-error">{error}</p>}
      <div className="form-actions">
        <Button
          variant="primary"
          size="sm"
          disabled={!dirty}
          onClick={async () => setError(await saveBackupSettings(draft))}
        >
          Save backup settings
        </Button>
        {settings.lastBackupAt !== null && (
          <span className="muted">Last backup {formatDateTime(settings.lastBackupAt)}</span>
        )}
        {settings.lastBackupError && (
          <span className="field-error">Last error: {settings.lastBackupError}</span>
        )}
      </div>
      {backups.length > 0 && (
        <table className="endpoint-table">
          <thead>
            <tr>
              <th>Backup</th>
              <th>Written</th>
              <th className="num">Size</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {backups.map((b) => (
              <tr key={b.path}>
                <td className="mono" title={b.path}>
                  {b.fileName}
                </td>
                <td className="muted">{formatDateTime(b.modifiedAt)}</td>
                <td className="num">{formatBytes(b.size)}</td>
                <td>
                  <Button size="sm" variant="ghost" onClick={() => void restore(b)}>
                    Restore…
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function NotesCard() {
  const notes = useProjectStore((s) => s.notes);
  const saveNotes = useProjectStore((s) => s.saveNotes);
  const [draft, setDraft] = useState<string | null>(null);
  const [preview, setPreview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const text = draft ?? notes ?? "";
  const dirty = draft !== null && draft !== notes;
  return (
    <section className="card">
      <div className="card-head">
        <h2>Notes</h2>
        <span className="spacer" />
        <Switch label="Preview" checked={preview} onChange={setPreview} />
      </div>
      {preview ? (
        text.trim() === "" ? (
          <p className="muted">No notes yet.</p>
        ) : (
          <Markdown source={text} />
        )
      ) : (
        <textarea
          className="mono notes-editor"
          aria-label="Project notes (Markdown)"
          placeholder="Markdown: # headings, - lists, **bold**, `code`, ```code blocks```…"
          rows={10}
          maxLength={200000}
          value={text}
          onChange={(e) => setDraft(e.target.value)}
        />
      )}
      {error && <p className="field-error">{error}</p>}
      <div className="form-actions">
        <Button
          size="sm"
          variant="primary"
          disabled={!dirty}
          onClick={async () => {
            const e = await saveNotes(text);
            setError(e);
            if (!e) setDraft(null);
          }}
        >
          Save notes
        </Button>
        {dirty && <span className="muted">Unsaved changes</span>}
      </div>
    </section>
  );
}

const AUDIT_CATEGORIES: { label: string; prefix: string | null }[] = [
  { label: "All categories", prefix: null },
  { label: "Project & backups", prefix: "project." },
  { label: "Scope", prefix: "scope." },
  { label: "Tasks & scans", prefix: "task." },
  { label: "Plugins", prefix: "plugin." },
  { label: "ChosAgent & ChosAI", prefix: "agent." },
  { label: "Findings & suppressions", prefix: "finding." },
  { label: "Keyring & credentials", prefix: "keyring." },
  { label: "Settings", prefix: "settings." },
  { label: "Organizer", prefix: "organizer." },
];

function AuditCard() {
  const audit = useProjectStore((s) => s.audit);
  const auditQuery = useProjectStore((s) => s.auditQuery);
  const queryAudit = useProjectStore((s) => s.queryAudit);
  const [searchInput, setSearchInput] = useState(auditQuery.search ?? "");

  return (
    <section className="card">
      <h2>Audit log</h2>
      <p className="field-help">
        Security-relevant actions on this project (scope, tasks, plugins, agent approvals, exports,
        backups, settings imports, proxy and credential changes). Entries cannot be edited or
        deleted.
      </p>
      <div className="form-actions" style={{ marginBottom: 10 }}>
        <select
          aria-label="Audit category"
          value={auditQuery.actionPrefix ?? ""}
          onChange={(e) =>
            void queryAudit({
              actionPrefix: e.target.value === "" ? null : e.target.value,
              offset: 0,
            })
          }
        >
          {AUDIT_CATEGORIES.map((c) => (
            <option key={c.label} value={c.prefix ?? ""}>
              {c.label}
            </option>
          ))}
        </select>
        <input
          type="search"
          aria-label="Search audit log"
          placeholder="Search action or details…"
          value={searchInput}
          onChange={(e) => {
            const val = e.target.value;
            setSearchInput(val);
            void queryAudit({
              search: val.trim() === "" ? null : val.trim(),
              offset: 0,
            });
          }}
        />
        <Button size="sm" variant="ghost" onClick={() => void queryAudit()}>
          Refresh
        </Button>
      </div>
      {audit.length === 0 ? (
        <p className="muted">No entries match the current filter.</p>
      ) : (
        <table className="endpoint-table">
          <thead>
            <tr>
              <th>When</th>
              <th>Action</th>
              <th>Details</th>
            </tr>
          </thead>
          <tbody>
            {audit.map((e) => (
              <tr key={e.id}>
                <td className="muted">{formatDateTime(e.at)}</td>
                <td className="mono">{e.action}</td>
                <td className="selectable" title={e.detail}>
                  {e.detail}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
