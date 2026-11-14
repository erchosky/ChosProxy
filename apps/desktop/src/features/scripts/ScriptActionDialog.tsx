import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { ErrorState, LoadingState } from "../../components/States";
import { copyText } from "../../services/clipboard";
import { useAppStore } from "../../state/app";
import { useScriptsStore } from "../../state/scripts";

/** The output of an action script run from a request's menu. */
export function ScriptActionDialog() {
  const action = useScriptsStore((s) => s.action);
  const close = useScriptsStore((s) => s.closeAction);
  if (!action) return null;
  const { run } = action;
  const label = `${action.name} — request #${action.exchangeId}`;
  return (
    <div className="palette-backdrop" role="presentation" onClick={close}>
      <div
        className="dialog engage-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        <header className="dialog-head">
          <strong>{label}</strong>
          <Button size="sm" variant="ghost" onClick={close}>
            Close
          </Button>
        </header>
        {action.error && <ErrorState title="The script could not run" message={action.error} />}
        {!action.error && !run && <LoadingState label="Running…" />}
        {run?.problem && (
          <p className="callout callout-danger">
            {run.problem.line !== null ? `Line ${run.problem.line}: ` : ""}
            {run.problem.message}
          </p>
        )}
        {run && !run.problem && (
          <>
            <div className="dialog-row">
              <Badge tone="neutral">{run.elapsedMs} ms</Badge>
              <span className="spacer" />
              <Button
                size="sm"
                icon="copy"
                onClick={() =>
                  void copyText(run.output ?? "").then((ok) =>
                    useAppStore
                      .getState()
                      .notify(ok ? "info" : "error", ok ? "Copied" : "Could not copy"),
                  )
                }
              >
                Copy output
              </Button>
            </div>
            <pre className="script-output">{run.output || "(no output)"}</pre>
            {run.printed.length > 0 && (
              <pre className="script-output script-printed">{run.printed.join("\n")}</pre>
            )}
          </>
        )}
      </div>
    </div>
  );
}
