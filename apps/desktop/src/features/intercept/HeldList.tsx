import { useEffect, useState } from "react";
import { Badge } from "../../components/Badge";
import { EmptyState } from "../../components/States";
import { heldKindLabel, isWebSocketKind } from "../../lib/heldKind";
import { waited } from "../../lib/httpText";
import { useInterceptStore } from "../../state/intercept";

/** Paused messages, oldest first. */
export function HeldList() {
  const state = useInterceptStore((s) => s.state);
  const selectedId = useInterceptStore((s) => s.selectedId);
  const select = useInterceptStore((s) => s.select);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);

  if (state.pending.length === 0) {
    return (
      <EmptyState title={state.enabled ? "Waiting for traffic" : "Intercept is off"}>
        {state.enabled
          ? "Matching requests, responses and WebSocket messages will pause here until you forward or drop them."
          : "Turn Intercept on to pause matching messages before they are delivered."}
      </EmptyState>
    );
  }
  return (
    <ul className="held-list" role="listbox" aria-label="Paused messages">
      {state.pending.map((m) => (
        <li
          key={m.id}
          role="option"
          aria-selected={m.id === selectedId}
          className="held-item"
          onClick={() => void select(m.id)}
        >
          <Badge tone={m.kind === "request" ? "accent" : isWebSocketKind(m.kind) ? "ok" : "warn"}>
            {heldKindLabel(m.kind)}
          </Badge>
          {m.wsOpcode && <span className="muted">{m.wsOpcode}</span>}
          <span className={`method method-${m.method.toLowerCase()}`}>{m.method}</span>
          {m.status !== null && <span className="status">{m.status}</span>}
          <span className="held-url" title={m.url}>
            {m.url}
          </span>
          <span className="muted">{waited(m.heldAt, now)}</span>
        </li>
      ))}
    </ul>
  );
}
