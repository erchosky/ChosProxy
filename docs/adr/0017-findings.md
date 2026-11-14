# ADR-0017: Findings store

* Status: Accepted
* Date: 2026-10-28

## Context
Milestone 9 needs a place for problems found during an assessment, shared
later by the passive analyzer, the scanner, plugins and reporting
(ARCHITECTURE §11). The automatic passive checks are not part of this
change; findings are documented manually from captured requests first.

## Decisions
1. **Schema v7**: `findings` (type, title, severity info…critical,
   confidence tentative/firm/certain, review status open / confirmed /
   false positive / accepted risk / fixed, location, description,
   remediation, notes, `source`, optional unique `dedup_key` for automatic
   sources) and `finding_evidence` (copies of the request and response,
   origin exchange id, note). A request can be evidence of a finding once.
2. **Evidence is copied**, like Organizer items, so findings survive history
   clean-ups and in-scope exports (which keep evidence blobs).
3. **Suppression is a status**, not a deletion: false positives and accepted
   risks stay as records and are hidden by default in the UI. Deleting is
   possible but asks for confirmation; creation, status and severity changes
   and deletions go to the project audit log.
4. The dashboard shows open and confirmed findings per severity.

## Consequences
* Automatic sources (passive analysis, scanner, plugins) will write through
  the same store using `dedup_key` to merge repeated detections.
