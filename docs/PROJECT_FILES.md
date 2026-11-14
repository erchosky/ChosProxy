# Project files, crash recovery, encrypted exports, Security Center, temporary files and the Performance view

## Encrypted projects

A project file can be encrypted with a password. Everything in it is then
unreadable without the password: traffic, tokens and cookies, findings,
notes, the Organizer, and the write-ahead log next to the file.

* **New project file…** asks whether to encrypt it.
* **Open project file…** asks for the password when the file is encrypted.
  A wrong password changes nothing: the project you have open stays open.
* **Encrypted temporary project** (Welcome screen, or the command palette)
  uses a random key that exists only while it is open.
* **Save as…** and **Export a copy…** let you keep the encryption, save
  without it, or choose a new password. A copy of an encrypted temporary
  project is not encrypted unless you give a password.
* **Change password…** (Project view) re-encrypts the open file. Backups
  made before keep the old password; restoring one asks for it.
* Automatic backups of an encrypted project are encrypted with its
  password and verified with it.

ChosProxy does not keep the password anywhere and cannot recover it.
Without it the file cannot be opened.

The encryption is SQLCipher's (AES-256, with PBKDF2-HMAC-SHA512 to derive
the key from the password). ChosProxy has no cryptography of its own.

Headless and API:

* `chosproxy-cli serve --project FILE` reads the password from
  `CHOSPROXY_PROJECT_PASSWORD` or from the first line of
  `--password-file FILE` (`-` reads standard input). With `--new FILE` it
  creates the file encrypted. Do not put a password on the command line.
* The automation API's `POST /v1/project/open` and `/v1/project/create`
  accept an optional `password` field.

## Crash recovery and read-only projects

While a project is open, ChosProxy writes a per-instance session marker in
`sessions/<session_id>.json` (`0600`) recording the project path, kind,
whether it is encrypted, the process ID, the process start time, and running
task IDs — never keys, passwords, tokens, or message bodies. When ChosProxy
closes cleanly, it removes its own session marker.

If ChosProxy exits unexpectedly, the next launch checks `sessions/` and
verifies via the operating system whether the recorded process (`pid` +
start time) is still alive so another running ChosProxy instance is never
interfered with. When a stale session is found, the startup **Crash
Recovery** dialog offers three choices:

* **Restore project**: runs SQLite's `PRAGMA quick_check` before opening
  (asking for the password if encrypted), marks interrupted traffic-sending
  tasks (`crawl`, `scan`, `fuzz`, `sequencer`, `discovery`, `browserCrawl`,
  `apiSend`, `agent`) as `Cancelled` with an explanation so no requests are
  re-sent automatically, and resumes passive live tasks (`liveAudit`,
  `liveCrawl`).
* **Open read-only**: opens the project file in read-only mode so you can
  inspect traffic, findings, and notes without modifying the database. A
  read-only banner is shown at the top of the window, and proxy listeners,
  tool sends, tasks, and edits are disabled until a writable project is
  opened or saved via **Save as…**.
* **Discard recovery**: removes the stale session marker (with an option to
  delete the orphaned temporary project file if the crashed session used a
  temporary project).

While a temporary project is waiting for crash recovery, automatic temporary
file cleanup protects it so it is never deleted before you decide.

## Encrypted sharing (Organizer collections and settings)

Organizer collections (including nested `Parent/Child` sub-collections) and
settings exports can be saved either as plain JSON or as password-sealed
files (`PBKDF2-HMAC-SHA256` with 600,000 rounds + `AES-256-GCM`, with an
authenticated header).

* **Organizer → Export…** exports all items or a chosen collection (and its
  nested sub-collections), optionally sealed with a password.
* **Organizer → Import…** inspects the file first, prompts for the password
  when the file is sealed, and imports all items in a single transaction.
* **Settings → Transfer** supports optional password encryption when
  exporting settings and prompts for the password before showing the import
  diff preview.
* Every export writes to a temporary sibling file (`0600`) and renames it
  atomically so a failed export never overwrites or truncates an existing
  file.

## Security Center and project audit log

Settings → **Security & Privacy** shows a local-only posture overview across
13 checks: local CA status, proxy listener exposure and authentication,
Automation API status, plugin permissions and signed-only policy, ChosAI
mode, ChosAgent permissions, upstream proxies, ChosOAST mode, TLS
verification exceptions, project encryption and read-only status, temporary
files, and credential keyring protection. Clicking any row navigates
directly to the relevant setting.

Project → **Audit log** records security-relevant project actions (project
open/close/encryption/rekey, crash recovery, plugin permission changes,
traffic-sending tasks, active scans, ChosAgent approvals and rejections,
exports, and scope/finding changes) and supports filtering by category,
search text, and date range.

## Temporary files

Settings → **Storage** lists ChosProxy's temporary directory: temporary
projects, lab browser profiles (also used by the DOM Inspector and browser
crawls) and reports being written for the API. For each entry it shows the
size, when it changed, and whether it is in use.

* **Clean now** removes this run's leftovers that nothing uses. ChosProxy
  also removes them when it closes.
* **Remove old leftovers…** removes entries from earlier runs, once
  unchanged for a day, after you confirm. Earlier runs' entries are never
  removed on their own, because they may belong to another ChosProxy open
  right now.
* ChosProxy never removes:
  * the open project;
  * the running lab browser's profile;
  * files it did not make;
  * symbolic links;
  * anything outside its temporary directory.

## Performance view

Tools → **Performance** shows this ChosProxy process every 2 seconds
while the page is open: memory, CPU, the project database size (file and
write-ahead log), how many requests and responses are stored per second,
open proxy connections, running tasks, resource-pool requests in flight
and waiting, and captures dropped because storage could not keep up. It
is read locally and never sent anywhere.
