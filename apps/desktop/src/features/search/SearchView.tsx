import type { SearchLocation } from "../../bindings/SearchLocation";
import type { SearchQuery } from "../../bindings/SearchQuery";
import type { WsSearchHit } from "../../bindings/WsSearchHit";
import { useAppStore } from "../../state/app";
import { useWebSocketsStore } from "../../state/websockets";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { statusTone } from "../../lib/format";
import { useSearchStore } from "../../state/search";
import { useFindingsStore } from "../../state/findings";
import { isMenuKey, openContextMenu, openMenuBelow } from "../../state/contextMenu";
import { exchangeMenu, sendToOrganizer, sendToRepeater } from "../../state/tools";

const LOCATIONS: { key: keyof SearchQuery; label: string }[] = [
  { key: "inUrls", label: "URLs" },
  { key: "inRequestHeaders", label: "Request headers" },
  { key: "inRequestBodies", label: "Request bodies" },
  { key: "inResponseHeaders", label: "Response headers" },
  { key: "inResponseBodies", label: "Response bodies" },
  { key: "inNotes", label: "Notes" },
  { key: "inWebsockets", label: "WebSocket messages" },
];

const LOCATION_LABEL: Record<SearchLocation, string> = {
  url: "URL",
  requestHeaders: "Request headers",
  requestBody: "Request body",
  responseHeaders: "Response headers",
  responseBody: "Response body",
  notes: "Notes",
};

export function SearchView() {
  return (
    <div className="history-view">
      <SearchForm />
      <div className="history-split">
        <SplitPane direction="vertical" initial={0.45} first={<Results />} second={<HitDetail />} />
      </div>
    </div>
  );
}

function SearchForm() {
  const query = useSearchStore((s) => s.query);
  const setQuery = useSearchStore((s) => s.setQuery);
  const setInScopeOnly = useSearchStore((s) => s.setInScopeOnly);
  const run = useSearchStore((s) => s.run);
  const running = useSearchStore((s) => s.running);
  const noLocation = LOCATIONS.every((l) => !query[l.key]);
  return (
    <form
      className="filters search-form"
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        void run();
      }}
    >
      <label className="filter-search grow">
        <Icon name="search" size={14} />
        <input
          type="search"
          placeholder={query.regex ? "Regular expression…" : "Search all traffic…"}
          aria-label="Search text"
          value={query.text}
          onChange={(e) => setQuery({ text: e.target.value })}
        />
      </label>
      <Switch label="Regex" checked={query.regex} onChange={(regex) => setQuery({ regex })} />
      <Switch
        label="Match case"
        checked={query.caseSensitive}
        onChange={(caseSensitive) => setQuery({ caseSensitive })}
      />
      <Switch label="In scope only" checked={query.filter.inScopeOnly} onChange={setInScopeOnly} />
      <Button
        type="submit"
        variant="primary"
        size="sm"
        disabled={running || query.text.trim() === "" || noLocation}
      >
        {running ? "Searching…" : "Search"}
      </Button>
      <div className="chip-group search-locations" role="group" aria-label="Search in">
        {LOCATIONS.map((l) => (
          <button
            key={l.key}
            type="button"
            className="chip"
            aria-pressed={Boolean(query[l.key])}
            onClick={() => setQuery({ [l.key]: !query[l.key] })}
          >
            {l.label}
          </button>
        ))}
      </div>
    </form>
  );
}

function Results() {
  const results = useSearchStore((s) => s.results);
  const running = useSearchStore((s) => s.running);
  const error = useSearchStore((s) => s.error);
  const selected = useSearchStore((s) => s.selected);
  const select = useSearchStore((s) => s.select);
  const run = useSearchStore((s) => s.run);
  if (error) return <ErrorState title="Search failed" message={error} onRetry={() => void run()} />;
  if (running && !results) return <LoadingState label="Searching…" />;
  if (!results) {
    return (
      <EmptyState title="Search the project">
        Finds text in URLs, headers, bodies (after removing chunked framing and gzip/deflate/brotli
        compression) and notes, newest traffic first.
      </EmptyState>
    );
  }
  if (results.hits.length === 0 && results.wsHits.length === 0) {
    return (
      <EmptyState title="No matches">
        Searched {results.scanned.toLocaleString()} requests
        {results.wsScanned > 0 &&
          ` and ${results.wsScanned.toLocaleString()} WebSocket messages`}{" "}
        in {results.elapsedMs} ms.
      </EmptyState>
    );
  }
  return (
    <div className="search-results">
      <div className="search-summary muted">
        {results.hits.length.toLocaleString()} matches in{" "}
        {results.matchedExchanges.toLocaleString()} requests · searched{" "}
        {results.scanned.toLocaleString()}
        {results.wsScanned > 0 &&
          ` · ${results.wsHits.length.toLocaleString()} in ${results.wsScanned.toLocaleString()} WebSocket messages`}{" "}
        in {results.elapsedMs} ms
        {results.truncated && " · stopped at the result limit; refine the search"}
      </div>
      <ul className="hit-list" aria-label="Search results">
        {results.hits.map((h, i) => (
          <li key={`${h.row.id}-${h.location}`}>
            <button
              type="button"
              className={`hit${selected === i ? " selected" : ""}`}
              aria-current={selected === i ? "true" : undefined}
              onClick={() => select(i)}
              onContextMenu={(e) => {
                select(i);
                openContextMenu(e, "Request actions", exchangeMenu(h.row));
              }}
              onKeyDown={(e) => {
                if (!isMenuKey(e)) return;
                e.preventDefault();
                select(i);
                openMenuBelow(e.currentTarget, "Request actions", exchangeMenu(h.row));
              }}
            >
              <span className="hit-head">
                <span className="muted">#{h.row.id}</span>
                <span className={`method method-${h.row.method.toLowerCase()}`}>
                  {h.row.method}
                </span>
                <span className="mono hit-url" title={h.row.url}>
                  {h.row.url}
                </span>
                <span className={`status status-${statusTone(h.row.status)}`}>
                  {h.row.status ?? "—"}
                </span>
                <Badge>{LOCATION_LABEL[h.location]}</Badge>
                {h.count > 1 && <span className="muted">×{h.count}</span>}
              </span>
              <span className="mono hit-snippet">
                {h.before}
                <mark>{h.matched}</mark>
                {h.after}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {results.wsHits.length > 0 && <WsHits hits={results.wsHits} />}
    </div>
  );
}

/** WebSocket message matches; each opens in the WebSockets view. */
function WsHits({ hits }: { hits: WsSearchHit[] }) {
  return (
    <>
      <h3 className="hit-section">WebSocket messages</h3>
      <ul className="hit-list" aria-label="WebSocket message results">
        {hits.map((h) => (
          <li key={h.message.id}>
            <button
              type="button"
              className="hit"
              title="Open in the WebSockets view"
              onClick={() => {
                useAppStore.getState().setView("websockets");
                void useWebSocketsStore
                  .getState()
                  .openMessage(h.message.connectionId, h.message.id, h.matched);
              }}
            >
              <span className="hit-head">
                <span className="muted">msg {h.message.id}</span>
                <Badge tone={h.message.direction === "toServer" ? "accent" : "neutral"}>
                  {h.message.direction === "toServer" ? "to server" : "to client"}
                </Badge>
                <span className="mono hit-url" title={h.url}>
                  {h.url}
                </span>
                {h.count > 1 && <span className="muted">×{h.count}</span>}
              </span>
              <span className="mono hit-snippet">
                {h.before}
                <mark>{h.matched}</mark>
                {h.after}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </>
  );
}

function HitDetail() {
  const detail = useSearchStore((s) => s.detail);
  const selected = useSearchStore((s) => s.selected);
  const select = useSearchStore((s) => s.select);
  if (detail.status === "idle") return <EmptyState title="Select a match" />;
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error") {
    return (
      <ErrorState
        title="Could not load the exchange"
        message={detail.message}
        onRetry={() => select(selected)}
      />
    );
  }
  const { row, request, response } = detail.detail;
  return (
    <div className="exchange-panel">
      <div className="exchange-summary">
        <span className="mono selectable" title={row.url}>
          <strong>#{row.id}</strong> {row.method} {row.url}
        </span>
        <span className="spacer" />
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
      </div>
      <div className="exchange-split">
        <SplitPane
          direction="horizontal"
          first={
            <MessageViewer
              title="Request"
              message={request}
              emptyText="No request"
              compareLabel={`#${row.id} request`}
            />
          }
          second={
            <MessageViewer
              title="Response"
              message={response}
              compareLabel={`#${row.id} response`}
              emptyText={row.error ? `No response: ${row.error}` : "No response received"}
            />
          }
        />
      </div>
    </div>
  );
}
