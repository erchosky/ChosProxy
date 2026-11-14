import { useEffect, useMemo, useState } from "react";
import type { WsMessageDetail } from "../../bindings/WsMessageDetail";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { CodeView } from "../../components/message/CodeView";
import { HexView } from "../../components/message/HexView";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { Segmented } from "../../components/Tabs";
import { base64ToBytes } from "../../lib/bytes";
import { formatBytes, formatClock, formatDateTime } from "../../lib/format";
import { languageOf, prettyBody } from "../../lib/pretty";
import { useComparerStore } from "../../state/comparer";
import { useAppStore } from "../../state/app";
import { sendToDecoder } from "../../state/tools";
import { WS_PAGE_SIZE, useWebSocketsStore } from "../../state/websockets";

const SEARCH_DEBOUNCE_MS = 300;

export function WebSocketsView() {
  const setActive = useWebSocketsStore((s) => s.setActive);
  useEffect(() => {
    setActive(true);
    return () => setActive(false);
  }, [setActive]);
  return (
    <div className="history-split">
      <SplitPane
        direction="vertical"
        initial={0.32}
        first={<ConnectionTable />}
        second={
          <SplitPane
            direction="horizontal"
            initial={0.55}
            first={<MessageList />}
            second={<MessageDetail />}
          />
        }
      />
    </div>
  );
}

function ConnectionTable() {
  const connections = useWebSocketsStore((s) => s.connections);
  const loaded = useWebSocketsStore((s) => s.loaded);
  const error = useWebSocketsStore((s) => s.error);
  const selected = useWebSocketsStore((s) => s.selectedConnection);
  const inScopeOnly = useWebSocketsStore((s) => s.inScopeOnly);
  const { selectConnection, setConnectionFilter, load } = useWebSocketsStore.getState();
  const [search, setSearch] = useState(useWebSocketsStore.getState().connectionSearch);
  useEffect(() => {
    const t = window.setTimeout(() => setConnectionFilter({ search }), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [search, setConnectionFilter]);

  return (
    <div className="organizer-list">
      <div className="filters" role="search">
        <label className="filter-search">
          <Icon name="search" size={14} />
          <input
            type="search"
            placeholder="Filter connections by URL…"
            aria-label="Filter WebSocket connections"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <Switch
          label="In scope only"
          checked={inScopeOnly}
          onChange={(v) => setConnectionFilter({ inScopeOnly: v })}
        />
        <span className="spacer" />
        <span className="muted">{connections.length} connections</span>
      </div>
      {error ? (
        <ErrorState title="Could not load WebSockets" message={error} onRetry={load} />
      ) : !loaded ? (
        <LoadingState />
      ) : connections.length === 0 ? (
        <EmptyState title="No WebSocket connections yet">
          WebSocket connections opened through the proxy appear here with every message. Extensions
          such as compression are removed from the handshake so messages stay readable.
        </EmptyState>
      ) : (
        <div className="endpoint-table-wrap">
          <table className="endpoint-table organizer-table">
            <thead>
              <tr>
                <th>#</th>
                <th>Opened</th>
                <th>URL</th>
                <th className="num">Messages</th>
                <th className="num">Bytes</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {connections.map((c) => (
                <tr
                  key={c.id}
                  className={selected === c.id ? "selected" : undefined}
                  aria-selected={selected === c.id}
                  onClick={() => selectConnection(c.id)}
                >
                  <td className="muted">{c.id}</td>
                  <td className="muted">{formatDateTime(c.startedAt)}</td>
                  <td className="mono" title={c.url}>
                    <span className={`scope-dot${c.inScope ? " in" : ""}`} />{" "}
                    {c.url.replace(/^http/, "ws")}
                  </td>
                  <td className="num">{c.messages}</td>
                  <td className="num">{formatBytes(c.bytes)}</td>
                  <td>
                    {c.endedAt === null ? (
                      <Badge tone="ok">open</Badge>
                    ) : (
                      <span className="muted">
                        closed{c.closeCode !== null ? ` ${c.closeCode}` : ""}
                        {c.closeReason ? ` (${c.closeReason})` : ""}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function MessageFilters() {
  const filter = useWebSocketsStore((s) => s.filter);
  const { setFilter, exportConnection } = useWebSocketsStore.getState();
  const [search, setSearch] = useState(filter.search);
  useEffect(() => {
    if (search === filter.search) return;
    const t = window.setTimeout(() => setFilter({ search }), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [search, filter.search, setFilter]);
  return (
    <div className="filters" role="search">
      <label className="filter-search">
        <Icon name="search" size={14} />
        <input
          type="search"
          placeholder={filter.regex ? "Payload regex…" : "Search payloads…"}
          aria-label="Search messages"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </label>
      <Switch label="Regex" checked={filter.regex} onChange={(regex) => setFilter({ regex })} />
      <Segmented<"all" | "toServer" | "toClient">
        label="Direction"
        value={filter.direction ?? "all"}
        onChange={(d) => setFilter({ direction: d === "all" ? null : d })}
        items={[
          { id: "all", label: "Both" },
          { id: "toServer", label: "→ Server" },
          { id: "toClient", label: "← Client" },
        ]}
      />
      <Switch
        label="Hide ping/pong/close"
        checked={filter.hideControl}
        onChange={(hideControl) => setFilter({ hideControl })}
      />
      <span className="spacer" />
      <Button size="sm" variant="ghost" onClick={() => void exportConnection()}>
        Export JSON
      </Button>
    </div>
  );
}

function MessageList() {
  const selectedConnection = useWebSocketsStore((s) => s.selectedConnection);
  const page = useWebSocketsStore((s) => s.page);
  const offset = useWebSocketsStore((s) => s.offset);
  const error = useWebSocketsStore((s) => s.messagesError);
  const selected = useWebSocketsStore((s) => s.selectedMessage);
  const { selectMessage, loadMessages } = useWebSocketsStore.getState();
  if (selectedConnection === null) return <EmptyState title="Select a connection" />;
  return (
    <div className="organizer-list">
      <MessageFilters />
      {error ? (
        <ErrorState
          title="Could not load messages"
          message={error}
          onRetry={() => void loadMessages()}
        />
      ) : !page ? (
        <LoadingState />
      ) : page.total === 0 ? (
        <EmptyState title="No messages match" />
      ) : (
        <div className="endpoint-table-wrap">
          <div className="muted ws-page-info">
            {page.total.toLocaleString()} messages
            {page.searchTruncated && " (search stopped at its limit)"}
            {page.total > WS_PAGE_SIZE && (
              <>
                {" "}
                · showing {offset + 1}–{Math.min(offset + WS_PAGE_SIZE, page.total)}{" "}
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={offset === 0}
                  onClick={() => void loadMessages(Math.max(0, offset - WS_PAGE_SIZE))}
                >
                  Previous
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={offset + WS_PAGE_SIZE >= page.total}
                  onClick={() => void loadMessages(offset + WS_PAGE_SIZE)}
                >
                  Next
                </Button>
              </>
            )}
          </div>
          <table className="endpoint-table organizer-table ws-messages">
            <thead>
              <tr>
                <th>Time</th>
                <th>Dir.</th>
                <th>Type</th>
                <th className="num">Length</th>
                <th>Content</th>
              </tr>
            </thead>
            <tbody>
              {page.rows.map((m) => (
                <tr
                  key={m.id}
                  className={selected === m.id ? "selected" : undefined}
                  aria-selected={selected === m.id}
                  onClick={() => selectMessage(m.id)}
                >
                  <td className="muted">{formatClock(m.at)}</td>
                  <td className={m.direction === "toServer" ? "ws-out" : "ws-in"}>
                    {m.direction === "toServer" ? "→" : "←"}
                  </td>
                  <td>
                    {m.opcode}
                    {m.edited && <Badge tone="accent">edited</Badge>}
                    {m.dropped && <Badge tone="warn">dropped</Badge>}
                  </td>
                  <td className="num">{formatBytes(m.length)}</td>
                  <td className="mono" title={m.preview}>
                    {m.preview}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function PayloadView({ detail, original }: { detail: WsMessageDetail; original: boolean }) {
  const text = original ? detail.originalText : detail.text;
  const b64 = original ? detail.originalBase64 : detail.payloadBase64;
  const bytes = useMemo(() => base64ToBytes(b64 ?? ""), [b64]);
  const [selection, setSelection] = useState("");
  if (text === null) return <HexView bytes={bytes} />;
  const language = languageOf(null, text);
  return (
    <>
      <div className="message-toolbar">
        <span className="muted">{language}</span>
        <span className="spacer" />
        <Button size="sm" variant="ghost" onClick={() => sendToDecoder(selection || text)}>
          Decode{selection ? " selection" : ""}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            useComparerStore
              .getState()
              .add(`WS message #${detail.row.id}${original ? " (original)" : ""}`, text);
            useAppStore.getState().notify("info", "Sent to Comparer");
          }}
        >
          Compare
        </Button>
      </div>
      <div className="message-body">
        <CodeView
          text={prettyBody(text, language)}
          language={language}
          wrap
          label="WebSocket message"
          onSelection={setSelection}
        />
      </div>
    </>
  );
}

function MessageDetail() {
  const detail = useWebSocketsStore((s) => s.detail);
  const selectMessage = useWebSocketsStore((s) => s.selectMessage);
  const [showOriginal, setShowOriginal] = useState(false);
  if (detail.status === "idle") return <EmptyState title="Select a message" />;
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error") {
    return (
      <ErrorState
        title="Could not load the message"
        message={detail.message}
        onRetry={() => selectMessage(detail.id)}
      />
    );
  }
  const d = detail.detail;
  const hasOriginal = d.originalBase64 !== null;
  return (
    <section className="message-viewer" aria-label="WebSocket message">
      <header className="message-toolbar">
        <strong>
          {d.row.direction === "toServer" ? "Client → server" : "Server → client"} · {d.row.opcode}
        </strong>
        <span className="muted">{formatBytes(d.row.length)}</span>
        {d.row.truncated && <Badge tone="warn">capture truncated</Badge>}
        {d.row.dropped && <Badge tone="warn">dropped in Intercept</Badge>}
        {d.row.extension && <Badge tone="warn">extension data</Badge>}
        <span className="spacer" />
        {hasOriginal && (
          <Switch label="Show original" checked={showOriginal} onChange={setShowOriginal} />
        )}
      </header>
      <PayloadView
        key={`${d.row.id}:${showOriginal}`}
        detail={d}
        original={showOriginal && hasOriginal}
      />
    </section>
  );
}
