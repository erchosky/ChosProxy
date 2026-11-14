# ADR-0016: Projects — copies, backups, settings layers, audit log

* Status: Accepted
* Date: 2026-10-28

## Context
Milestone 8 completes project handling: Save As and export, automatic
backups, project settings that override the user's, settings
import/export, and an audit log (ARCHITECTURE §11).

## Decisions
1. **Consistent copies with `VACUUM INTO`** from a separate connection after
   flushing the writer queue: Save As, export and backups never stop
   capture and produce a compact standalone file. A destination that exists
   is refused. "In-scope only" copies delete out-of-scope traffic (and its
   orphaned blobs) from the copy, never from the project.
2. **Save As continues in the copy**; a temporary project becomes a file
   this way (the temporary file is deleted as usual). **Export** writes a
   copy and keeps working in the current project.
3. **Backups**: per-project settings (interval 5–1440 min, keep 1–100,
   in-scope only, folder). Written to `<file>.partial`, verified with
   `inspect_project_file` (application id, schema version, `quick_check`),
   then renamed; rotation deletes the oldest only after that, and never the
   newest. Nothing is written while the project is unchanged (contents
   fingerprint). Only project files are backed up. Folders the app creates
   are `0700`; files are `0600`. **Restore** opens a *copy* of a backup as a
   new project file next to the current one — the backup and the current
   project are never overwritten.
4. **Settings layers**: `project_settings` (schema v6) stores per-project
   values. The scope can be "global" (`scope.toml`) or overridden by the
   project; the effective scope is project ?? global, and saving goes to the
   layer in effect. Backup settings and notes are project-only. Other
   sections stay global for now.
5. **Settings import/export**: JSON (`chosproxy-settings-v1`) with chosen
   sections; listener credentials are never exported and existing ones are
   kept on import. Import is a preview (validation per section, current vs
   incoming) followed by an explicit apply of chosen sections, which are
   validated again and applied all-or-nothing.
6. **Audit log**: an append-only table (update/delete blocked by triggers)
   for scope and scope-layer changes, Save As/export, backups/restore,
   settings import/export, proxy configuration and credential changes, and
   Organizer deletions.
7. **Notes** are Markdown stored in the project and rendered by our own
   parser into React elements (no HTML; links are shown, not followed).

## Consequences
* Repeater tabs (M4), findings (M9) and tasks (M10) join the project file
  when those features exist.
* Encrypted projects and crash recovery remain M31.
