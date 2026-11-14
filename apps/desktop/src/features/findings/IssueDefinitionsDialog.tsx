import { useEffect, useMemo, useState } from "react";
import type { ActiveCheck } from "../../bindings/ActiveCheck";
import type { PassiveCheck } from "../../bindings/PassiveCheck";
import { Button } from "../../components/Button";
import { api, toIpcError } from "../../services/ipc";

export type CheckDefinition = PassiveCheck | ActiveCheck;

/** The definitions that match a search (type, summary, explanation, id). */
export function matchingDefinitions<T extends CheckDefinition>(checks: T[], search: string): T[] {
  const needle = search.trim().toLowerCase();
  const sorted = [...checks].sort((a, b) => a.kind.localeCompare(b.kind));
  if (needle === "") return sorted;
  return sorted.filter((c) =>
    [c.kind, c.summary, c.description, c.remediation, c.id].some((f) =>
      f.toLowerCase().includes(needle),
    ),
  );
}

/**
 * Issue definitions: what each automatic check reports, why it matters and
 * how to fix it, to read without waiting for a finding (matrix TARGET-006).
 */
export function IssueDefinitionsDialog({ onClose }: { onClose(): void }) {
  const [checks, setChecks] = useState<CheckDefinition[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  useEffect(() => {
    Promise.all([api.passiveChecks(), api.activeChecks().catch(() => [] as ActiveCheck[])])
      .then(([passive, active]) => setChecks([...passive, ...active]))
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, []);
  const shown = useMemo(() => matchingDefinitions(checks ?? [], search), [checks, search]);

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-wide issue-definitions"
        role="dialog"
        aria-modal="true"
        aria-label="Issue definitions"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Issue definitions</strong>
          <input
            type="search"
            autoFocus
            placeholder="Search…"
            aria-label="Search issue definitions"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <span className="muted">
            {shown.length} of {checks?.length ?? 0}
          </span>
          <Button size="sm" variant="ghost" onClick={onClose}>
            Close
          </Button>
        </header>
        {error && <p className="callout callout-danger">{error}</p>}
        <div className="issue-definitions-list">
          {shown.map((c) => (
            <details key={c.id}>
              <summary>
                <span className={`sev sev-${c.severity}`}>{c.severity}</span>{" "}
                <strong>{c.kind}</strong> <span className="muted">{c.summary}</span>
              </summary>
              <p>{c.description}</p>
              <p>
                <strong>Remediation.</strong> {c.remediation}
              </p>
              <p className="muted mono">{c.id}</p>
            </details>
          ))}
          {checks && shown.length === 0 && <p className="muted">No definition matches.</p>}
        </div>
      </div>
    </div>
  );
}
