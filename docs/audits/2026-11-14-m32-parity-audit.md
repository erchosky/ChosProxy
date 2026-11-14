# M32 parity audit (2026-11-14)

Every row of [BURP_PARITY_MATRIX.md](../BURP_PARITY_MATRIX.md) was read
against the code and the tests after M31b and the quality pass.

## How it was checked

* Every file or test named in a row's "Tests" column was looked up in the
  repository: all resolve (one is a crate name, `chosproxy-report`).
* Complete rows whose notes still said "Missing", complete rows without a
  test, planned rows whose milestone had passed, and partial rows that did
  not say what is missing were read one by one and compared with the code.
* The summary table is now counted from the rows by
  `node scripts/matrix-summary.mjs` (`--check` fails when they differ).

## Result

**Parity is not declared.** 134 rows: 87 complete, 41 partial, 4 not
built, 2 not applicable.

### Rows that were wrong and are corrected

| Row | Was | Now | Why |
|---|---|---|---|
| SCRIPT-006 HelloPlugin | planned | complete | It has shipped since M14 and has a test. |
| PROXY-022 wire-exact relay | partial | complete | Nothing was missing; two tests cover it. |
| PROJECT-001 projects | partial | complete | Nothing was missing. |
| TARGET-006 issue definitions | planned (M9, passed) | complete | Built in this audit: Findings → Issue definitions. |
| TARGET-007, TARGET-009 | complete, no test named | complete, tests named | TARGET-009 got a test (`state/tools.test.ts`). |
| EDITOR-007 extension tabs | planned for M23 (passed) | not built, needs a decision | M23 delivered menu actions, not editor tabs. |
| SCANNER-002 active audit | planned for M13/M25 | not built, pending | Active audit is absent; selected next on 2026-11-14. |
| EDITOR-006 | missing tab colours | tab colours done (M31b) | Only Fuzzer result colours are missing. |
| PROXY-015, PROXY-020, REPEATER-006, INTRUDER-003, SCANNER-003, SCANNER-015, SESSION-004 | "→ M31" | "needs a decision, not scheduled" | M31 and M31b passed without them. |
| EDITOR-004, TARGET-002, CRAWL-003, BROWSER-001, PLATFORM-005 | partial without a reason | partial, with what is missing | |

### Rows not built (they keep parity from being declared)

| Row | Capability | State |
|---|---|---|
| EDITOR-007 | Extension-provided message tabs | — (needs a decision) |
| SCANNER-002 | Active audit (authorized targets) | M13 (pending) |
| INSTR-001 | Runtime instrumentation of test apps | — (needs a decision) |
| PROJECT-004 | Crash recovery | M31 (pending) |

* **SCANNER-002** (active audit) is the largest gap. M13 is absent and was selected next by the user on 2026-11-14;
  this audit does not claim it is delivered. It also holds back SCANNER-013 and the automatic part of
  OAST-001.
* **PROJECT-004** (crash recovery) remains incomplete in M31 step A (selected next on 2026-11-14).
* **INSTR-001** and **EDITOR-007** wait for a design decision.

### Partial rows (41)

PROXY-009, PROXY-014, PROXY-015, PROXY-017, PROXY-020, PROXY-021, EDITOR-004, EDITOR-006, TARGET-002, REPEATER-002, REPEATER-004, REPEATER-006, INTRUDER-002, INTRUDER-003, INTRUDER-004, INTRUDER-005, SEQUENCER-001, SEQUENCER-002, LOGGER-001, ORGANIZER-003, SCANNER-003, SCANNER-004, SCANNER-005, SCANNER-011, SCANNER-013, SCANNER-014, SCANNER-015, CRAWL-001, CRAWL-003, OAST-001, DOM-001, BROWSER-001, SESSION-003, SESSION-004, SESSION-007, EXT-003, SETTINGS-001, PLATFORM-001, PLATFORM-002, PLATFORM-005, PLATFORM-007.

Each says in its notes what is missing. None is scheduled: the roadmap has
no milestone after M32, so what to build next is a decision for the owner.
The ones that matter most for day-to-day testing:

* raw / invalid HTTP/2 (PROXY-020, REPEATER-006);
* reports as PDF or from templates (PLATFORM-005);
* out-of-scope traffic dropped or not stored (TARGET-002);
* Kerberos and NTLM to an upstream proxy (PROXY-015, PROXY-017);
* the pending M31 work: encrypted sharing in the UI (ORGANIZER-003), Security
  Center and audit coverage.

## Not verified by this audit

* Nothing was looked at in the running application: rows were checked
  against code and automated tests only.
* The comparison is with the capabilities listed in the matrix, not with a
  fresh reading of the reference product's current documentation.
