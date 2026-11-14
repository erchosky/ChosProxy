import { Badge } from "../../components/Badge";
import { IconButton } from "../../components/Button";
import { useAppStore } from "../../state/app";

/** Compact list of listeners with start/stop/restart. */
export function ListenerControls() {
  const listeners = useAppStore((s) => s.listeners);
  const busy = useAppStore((s) => s.busy);
  const start = useAppStore((s) => s.startListener);
  const stop = useAppStore((s) => s.stopListener);
  const restart = useAppStore((s) => s.restartListener);

  if (listeners.length === 0) {
    return <p className="muted">No listeners configured. Add one in Proxy → Listeners.</p>;
  }
  return (
    <ul className="listener-list">
      {listeners.map(({ config, state }) => (
        <li key={config.id}>
          <span
            className={`dot ${state.state === "running" ? "dot-ok" : state.state === "failed" ? "dot-danger" : ""}`}
          />
          <span className="mono">
            {state.state === "running" ? state.address : `${config.bind_address}:${config.port}`}
          </span>
          <span className="muted">{config.id}</span>
          {state.state === "failed" && (
            <Badge tone="danger" title={state.error}>
              failed
            </Badge>
          )}
          {config.allow_remote && <Badge tone="warn">remote allowed</Badge>}
          <span className="spacer" />
          {state.state === "running" ? (
            <>
              <IconButton
                icon="restart"
                label={`Restart ${config.id}`}
                disabled={busy !== null}
                onClick={() => void restart(config.id)}
              />
              <IconButton
                icon="stop"
                label={`Stop ${config.id}`}
                disabled={busy !== null}
                onClick={() => void stop(config.id)}
              />
            </>
          ) : (
            <IconButton
              icon="play"
              label={`Start ${config.id}`}
              disabled={busy !== null}
              onClick={() => void start(config.id)}
            />
          )}
        </li>
      ))}
    </ul>
  );
}
