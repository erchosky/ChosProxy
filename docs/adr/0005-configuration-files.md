# ADR-0005: Configuration split into per-section TOML files

* Status: Accepted
* Date: 2026-10-21

## Context
The spec asks for independent files for global config, project config,
scope, proxy rules, scanner, rate limits and plugins.

## Options
1. One big config file.
2. **One TOML file per section** for global settings in the user config dir,
   and project-scoped sections stored inside the project DB (so a project is
   one portable file) with per-section import/export as TOML.
3. Everything in the project DB.

## Decision
Option 2. Global: `settings.toml` (UI, logging, capture limits) and
`proxy.toml` (listeners) now; `scope.toml`, `rules.toml`, `scanner.toml`,
`ratelimits.toml`, `plugins.toml` arrive with their milestones. Writes are
atomic (temp file + rename) with `0600` permissions. A file that fails to
parse is **never overwritten**; defaults are used and the error is surfaced.

## Consequences
* ➕ Human-editable, diff-able, versionable settings; sections evolve
  independently (each carries `version`).
* ➖ Two homes for settings (global files vs. project DB); documented in the
  Settings UI by grouping "Application" vs. "Project".
