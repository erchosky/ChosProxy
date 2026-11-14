# ADR-0041: Encrypted projects, temporary files and the Performance view

* Status: Accepted
* Date: 2026-11-13

## Context
M31 (Hardening / Recovery / Encryption) planned seven steps. These were
completed:
* B — encrypted projects (PROJECT-005);
* D — temporary file management (spec §117);
* E — a performance view (PLATFORM-006).

Steps A (crash recovery), C (encrypted sharing, user interface) and F
(Security Center and audit coverage) were not completed by this decision.
The user selected their completion on 2026-11-14; implementation is pending.

The specification asks for audited primitives only, never home-made
cryptography, and for never deleting project files by mistake.

## Decisions
1. **SQLCipher for project files.**
   * rusqlite is built with `bundled-sqlcipher-vendored-openssl`.
   * Plain projects open exactly as before.
   * A password goes to `PRAGMA key`, and SQLCipher 4 derives the key
     (PBKDF2-HMAC-SHA512) and encrypts every page and the WAL.
   * Encrypted temporary projects use a random raw key held in memory.
   * Before switching, a spike checked: copies with `VACUUM INTO` stay
     encrypted; wrong and missing keys fail; `rekey` and `sqlcipher_export`
     work; the macOS build (33 s for storage) and a Kali release build
     (no dynamic OpenSSL or SQLite) both succeed.
2. **Keys only in memory.**
   * `ProjectKey` hides itself in `Debug` and is never serialized, logged,
     audited or returned to the UI.
   * A wrong password is detected before the open project is closed.
   * The CLI reads the password from an environment variable or a file,
     never from arguments.
3. **Copies choose their encryption.**
   * Save As and export can keep the encryption, drop it, or use a new
     password (`CopyKey`). Dropping and changing go through
     `sqlcipher_export`; the copy gets the schema version and application
     id.
   * Backups keep the project's key and are verified with it.
   * A temporary project's random key is never carried into a file.
   * Changing the password is `PRAGMA rekey` on the writer, outside any
     transaction. The pooled readers are reset afterwards.
4. **Temporary files by name and ownership.**
   * ChosProxy only looks inside its own temporary directory, and only at
     names it creates.
   * The open project and the lab browser profile are protected.
   * Leftovers of this process (its pid is in the name) are cleaned on
     request and at exit.
   * Leftovers of other runs, which may be another live instance, are
     never removed automatically. They are removed only on request, once
     unchanged for a day.
   * Symbolic links are never followed or removed.
5. **A local Performance view.**
   * Memory and CPU of this process come from `sysinfo` 0.38 (feature
     `system` only).
   * The other counters come from the engine: database size, a stored
     counter fed by the insert hook, proxy stats, pools and tasks.
   * The UI turns two readings into a rate. It reads only while the page
     is open, and nothing is sent anywhere.
6. **`ring` at `opt-level = 3` in debug builds,** so key derivation and
   TLS stay fast in tests.

## Consequences
* A lost password means a lost project, and the UI says so.
* Builds compile OpenSSL from source. Release builds on Kali take longer
  (about 9.5 minutes under emulation for the CLI).
* Already in the code but not finished:
  * read-only project opening in storage (`OpenOptions.read_only`);
  * password-sealed export files (`storage::sealed`) with Organizer
    collection export and import and encrypted settings exports in the
    engine. There are no Tauri commands or UI for them.
* Deferred: the unrelated items parked on M31 move to M31b (ROADMAP).
  Steps A, C and F are selected next; implementation is pending.
