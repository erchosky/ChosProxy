import { useState } from "react";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { formatDateTime, formatDuration } from "../../lib/format";
import { useHistoryCtx } from "./historyContext";
import { useScopeStore } from "../../state/scope";
import { useFindingsStore } from "../../state/findings";
import { sendToOrganizer, sendToRepeater } from "../../state/tools";
import { AnnotationEditor } from "./AnnotationEditor";

/** Request/response pair of the selected history entry. */
export function ExchangePanel() {
  const useStore = useHistoryCtx();
  const detail = useStore((s) => s.detail);
  const select = useStore((s) => s.select);
  const addUrl = useScopeStore((s) => s.addUrl);
  const [showOriginal, setShowOriginal] = useState(false);

  if (detail.status === "idle") {
    return (
      <EmptyState title="Select a request">
        Pick a row above to inspect the request and response.
      </EmptyState>
    );
  }
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error") {
    return (
      <ErrorState
        title="Could not load the exchange"
        message={detail.message}
        onRetry={() => select(detail.id)}
      />
    );
  }
  const { row, request, response, clientAddr, reason, originalRequest, originalResponse } =
    detail.detail;
  const hasOriginal = originalRequest !== null || originalResponse !== null;
  const original = showOriginal && hasOriginal;
  return (
    <div className="exchange-panel">
      <div className="exchange-summary">
        <span className="mono selectable" title={row.url}>
          <strong>{row.method}</strong> {row.url}
        </span>
        <span className="spacer" />
        <Badge tone={row.inScope ? "ok" : "neutral"}>
          {row.inScope ? "in scope" : "out of scope"}
        </Badge>
        {!row.inScope && (
          <Button size="sm" variant="ghost" onClick={() => void addUrl(row.url, false, false)}>
            Add host to scope
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          icon="repeater"
          title="Open this request in a new Repeater tab"
          onClick={() => sendToRepeater(row.id)}
        >
          Repeater
        </Button>
        <Button size="sm" variant="ghost" onClick={() => void sendToOrganizer(row.id)}>
          Organizer
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => useFindingsStore.getState().openAdd(row.id, `request #${row.id}`)}
        >
          Finding
        </Button>
        {row.status !== null && (
          <Badge tone={row.status >= 500 ? "danger" : row.status >= 400 ? "warn" : "ok"}>
            {row.status} {reason}
          </Badge>
        )}
        {row.error && (
          <Badge tone="danger" title={row.error}>
            {row.error}
          </Badge>
        )}
        {row.edited && <Badge tone="accent">edited in Intercept</Badge>}
        {row.dropped && <Badge tone="warn">dropped in Intercept</Badge>}
        {hasOriginal && (
          <Switch label="Show original" checked={showOriginal} onChange={setShowOriginal} />
        )}
        <span className="muted">{formatDuration(row.durationMs)}</span>
        <span
          className="muted"
          title={`Client ${clientAddr ?? "?"} → server ${row.serverIp ?? "?"}`}
        >
          {formatDateTime(row.startedAt)}
        </span>
      </div>
      <AnnotationEditor row={row} />
      <div className="exchange-split">
        <SplitPane
          direction="horizontal"
          first={
            <MessageViewer
              title={original && originalRequest ? "Request (original)" : "Request"}
              message={original && originalRequest ? originalRequest : request}
              emptyText="No request"
              compareLabel={`#${row.id} request${original && originalRequest ? " (original)" : ""}`}
              url={row.url}
            />
          }
          second={
            <MessageViewer
              title={original && originalResponse ? "Response (original)" : "Response"}
              message={original && originalResponse ? originalResponse : response}
              compareLabel={`#${row.id} response${original && originalResponse ? " (original)" : ""}`}
              emptyText={row.error ? `No response: ${row.error}` : "No response received"}
            />
          }
        />
      </div>
    </div>
  );
}
