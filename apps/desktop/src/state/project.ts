import { create } from "zustand";
import type { AuditEntry } from "../bindings/AuditEntry";
import type { AuditQuery } from "../bindings/AuditQuery";
import type { BackupInfo } from "../bindings/BackupInfo";
import type { BackupSettings } from "../bindings/BackupSettings";
import type { ProjectSettings } from "../bindings/ProjectSettings";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";
import { askPassword, encryptionOf, withPassword } from "./password";

const DEFAULT_AUDIT_QUERY: AuditQuery = {
  actionPrefix: null,
  search: null,
  fromMs: null,
  toMs: null,
  limit: 200,
  offset: 0,
};

interface ProjectState {
  settings: ProjectSettings | null;
  backups: BackupInfo[];
  notes: string | null;
  audit: AuditEntry[];
  auditQuery: AuditQuery;
  error: string | null;
  backingUp: boolean;

  load(): Promise<void>;
  queryAudit(patch?: Partial<AuditQuery>): Promise<void>;
  saveBackupSettings(settings: BackupSettings): Promise<string | null>;
  backupNow(): Promise<void>;
  restore(backup: BackupInfo): Promise<void>;
  saveNotes(notes: string): Promise<string | null>;
  exportProject(inScopeOnly: boolean): Promise<void>;
  reset(): void;
}

export const useProjectStore = create<ProjectState>((set, get) => {
  const notify = (level: "info" | "error", message: string) =>
    useAppStore.getState().notify(level, message);

  async function refreshBackups() {
    try {
      const q = get().auditQuery;
      const hasFilter =
        q.actionPrefix !== null ||
        q.search !== null ||
        q.fromMs !== null ||
        q.toMs !== null ||
        q.offset > 0 ||
        q.limit !== 200;
      const [settings, backups, audit] = await Promise.all([
        api.projectSettings(),
        api.listBackups(),
        hasFilter ? api.auditLog(q) : api.auditLog(),
      ]);
      set({ settings, backups, audit });
    } catch (e) {
      set({ error: toIpcError(e).message });
    }
  }

  return {
    settings: null,
    backups: [],
    notes: null,
    audit: [],
    auditQuery: DEFAULT_AUDIT_QUERY,
    error: null,
    backingUp: false,

    async load() {
      try {
        const q = get().auditQuery;
        const hasFilter =
          q.actionPrefix !== null ||
          q.search !== null ||
          q.fromMs !== null ||
          q.toMs !== null ||
          q.offset > 0 ||
          q.limit !== 200;
        const [settings, backups, notes, audit] = await Promise.all([
          api.projectSettings(),
          api.listBackups(),
          api.projectNotes(),
          hasFilter ? api.auditLog(q) : api.auditLog(),
        ]);
        set({ settings, backups, notes, audit, error: null });
      } catch (e) {
        const err = toIpcError(e);
        set({ error: err.kind === "noProject" ? null : err.message });
      }
    },

    async queryAudit(patch) {
      const next: AuditQuery = { ...get().auditQuery, ...patch };
      set({ auditQuery: next });
      try {
        const hasFilter =
          next.actionPrefix !== null ||
          next.search !== null ||
          next.fromMs !== null ||
          next.toMs !== null ||
          next.offset > 0 ||
          next.limit !== 200;
        const audit = hasFilter ? await api.auditLog(next) : await api.auditLog();
        set({ audit, error: null });
      } catch (e) {
        const err = toIpcError(e);
        set({ error: err.kind === "noProject" ? null : err.message });
      }
    },

    async saveBackupSettings(settings) {
      try {
        set({ settings: await api.saveBackupSettings(settings) });
        await refreshBackups();
        return null;
      } catch (e) {
        return toIpcError(e).message;
      }
    },

    async backupNow() {
      set({ backingUp: true });
      try {
        const b = await api.backupNow();
        notify("info", `Backup written: ${b.fileName}`);
      } catch (e) {
        notify("error", `Backup failed: ${toIpcError(e).message}`);
      } finally {
        set({ backingUp: false });
        await refreshBackups();
      }
    },

    async restore(backup) {
      const ok = await dialogs.confirm(
        `Open a copy of “${backup.fileName}” as a new project file? The current project is ` +
          "closed (it stays on disk) and the backup itself is not changed.",
        "Restore backup?",
        "Restore",
      );
      if (!ok) return;
      try {
        const project = await withPassword("Restore an encrypted backup", (pw) =>
          api.restoreBackup(backup.path, pw),
        );
        if (!project) return;
        useAppStore.setState({ project });
        notify("info", `Restored into ${project.path}`);
        await get().load();
      } catch (e) {
        notify("error", `Restore failed: ${toIpcError(e).message}`);
      }
    },

    async saveNotes(notes) {
      try {
        await api.saveProjectNotes(notes);
        set({ notes });
        return null;
      } catch (e) {
        return toIpcError(e).message;
      }
    },

    async exportProject(inScopeOnly) {
      const path = await dialogs.chooseProjectToCreate(useAppStore.getState().info?.dataDir);
      if (!path) return;
      const current = useAppStore.getState().project;
      const canKeep = current?.encrypted === true && current.kind === "persistent";
      const choice = await askPassword({
        mode: "choose",
        title: "Export a copy",
        encrypt: canKeep,
        canKeep,
      });
      if (!choice) return;
      try {
        const info = await api.exportProject(path, inScopeOnly, encryptionOf(choice, canKeep));
        notify(
          "info",
          `Exported ${info.exchanges} requests to ${path}${info.encrypted ? " (encrypted)" : ""}`,
        );
        await refreshBackups();
      } catch (e) {
        notify("error", `Export failed: ${toIpcError(e).message}`);
      }
    },

    reset: () =>
      set({
        settings: null,
        backups: [],
        notes: null,
        audit: [],
        auditQuery: DEFAULT_AUDIT_QUERY,
        error: null,
      }),
  };
});
