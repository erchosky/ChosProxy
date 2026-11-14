import { useEffect, useMemo, useState } from "react";
import type { DomEventRow } from "../../bindings/DomEventRow";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { formatClock, formatDateTime } from "../../lib/format";
import { copyText } from "../../services/clipboard";
import { useAppStore } from "../../state/app";
import { splitMatch, useDomStore } from "../../state/dom";
import { sendToRepeater } from "../../state/tools";

const KINDS: { id: string; label: string; tone: "danger" | "warn" | "accent" | "neutral" }[] = [
  { id: "flow", label: "Source → sink", tone: "danger" },
  { id: "redirect", label: "Navigation", tone: "warn" },
  { id: "messageListener", label: "postMessage listener", tone: "accent" },
  { id: "prototypePollution", label: "Prototype pollution", tone: "danger" },
  { id: "clobbering", label: "DOM clobbering", tone: "neutral" },
];
const kindOf = (id: string) => KINDS.find((k) => k.id === id) ?? KINDS[0];

function copy(text: string, what: string) {
  void copyText(text).then((ok) =>
    useAppStore.getState().notify(ok ? "info" : "error", ok ? `${what} copied` : "Could not copy"),
  );
}

/** DOM Inspector: source → sink analysis in the lab browser. */
export function DomInspectorView() {
  const loaded = useDomStore((s) => s.loaded);
  const error = useDomStore((s) => s.error);
  useEffect(() => {
    void useDomStore.getState().load();
  }, []);
  if (!loaded) return <LoadingState />;
  if (error)
    return (
      <ErrorState
        title="Could not load the DOM Inspector"
        message={error}
        onRetry={() => void useDomStore.getState().load()}
      />
    );
  return (
    <div className="page dom-page">
      <div className="page-header">
        <div>
          <h1>DOM Inspector</h1>
          <p className="page-sub">
            Watches the lab browser while you use the application: where values from the URL, the
            referrer, window.name, cookies or postMessage end up in dangerous DOM sinks.
          </p>
        </div>
      </div>
      <SessionCard />
      <div className="dom-workspace">
        <SplitPane
          direction="vertical"
          initial={0.45}
          first={<EventList />}
          second={<EventDetail />}
        />
      </div>
    </div>
  );
}

function SessionCard() {
  const status = useDomStore((s) => s.status);
  const launching = useDomStore((s) => s.launching);
  const browsers = useAppStore((s) => s.browsers);
  const browser = useAppStore((s) => s.browser);
  const chromium = browsers.filter((b) => b.kind === "chromium");
  const [browserId, setBrowserId] = useState<string>("");
  const [url, setUrl] = useState("");
  const chosen = browserId || chromium[0]?.id || "";

  if (status?.active && status.canary) {
    const c = status.canary;
    const withCanary = (where: "hash" | "query") => {
      const base = url.trim() || "http://chosproxy/";
      const clean = base.split("#")[0] ?? base;
      setUrl(
        where === "hash" ? `${clean}#${c}` : `${clean}${clean.includes("?") ? "&" : "?"}chos=${c}`,
      );
    };
    return (
      <section className="card dom-session" aria-label="DOM Inspector session">
        <div className="dialog-row">
          <Badge tone="ok">instrumenting the lab browser</Badge>
          <span className="muted">{status.observed.toLocaleString()} reports this session</span>
        </div>
        <p>
          Put the canary where the page reads input, then use the page. Values that reach a sink
          with the canary are certain flows.
        </p>
        <div className="dom-canary">
          <code className="mono selectable">{c}</code>
          <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(c, "Canary")}>
            Copy canary
          </Button>
          <span className="muted">
            e.g. <code className="mono">#{c}</code>, <code className="mono">?q={c}</code>, or{" "}
            <code className="mono">window.name</code> / a postMessage containing it
          </span>
        </div>
        <form
          className="dialog-row dom-start"
          onSubmit={(ev) => {
            ev.preventDefault();
            void useDomStore.getState().navigate(url);
          }}
        >
          <input
            className="mono"
            aria-label="URL to open in the lab browser"
            placeholder="https://target.example/page"
            value={url}
            onChange={(ev) => setUrl(ev.target.value)}
          />
          <Button size="sm" variant="ghost" onClick={() => withCanary("hash")}>
            + canary in #
          </Button>
          <Button size="sm" variant="ghost" onClick={() => withCanary("query")}>
            + canary in ?
          </Button>
          <Button type="submit" size="sm" icon="play" disabled={url.trim() === ""}>
            Open in the lab browser
          </Button>
        </form>
      </section>
    );
  }
  return (
    <section className="card dom-session" aria-label="Start the DOM Inspector">
      {status?.error && <p className="callout callout-warn">{status.error}</p>}
      {browser?.running && (
        <p className="muted">
          A lab browser is already open without the DOM Inspector; close it first.
        </p>
      )}
      {chromium.length === 0 ? (
        <p className="callout">
          The DOM Inspector needs a Chromium-family browser (Chromium, Chrome, Edge or Brave). On
          Kali: <code>sudo apt install chromium</code>.
        </p>
      ) : (
        <div className="dialog-row dom-start">
          {chromium.length > 1 && (
            <select
              aria-label="Browser"
              value={chosen}
              onChange={(e) => setBrowserId(e.target.value)}
            >
              {chromium.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.name}
                </option>
              ))}
            </select>
          )}
          <input
            className="mono"
            aria-label="Start URL"
            placeholder="https://target.example/ (optional)"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
          <Button
            variant="primary"
            icon="play"
            disabled={launching || browser?.running === true}
            onClick={() => void useDomStore.getState().launch(chosen || null, url)}
          >
            Open lab browser with DOM Inspector
          </Button>
        </div>
      )}
      <p className="muted">
        The instrumentation runs in the isolated lab browser only, before the page&apos;s scripts,
        and only observes. While it runs, the browser&apos;s DevTools endpoint listens on a random
        loopback port.
      </p>
    </section>
  );
}

function EventList() {
  const events = useDomStore((s) => s.events);
  const kinds = useDomStore((s) => s.kinds);
  const selectedId = useDomStore((s) => s.selectedId);
  const { select, toggleKind, clear } = useDomStore.getState();
  const shown = useMemo(
    () => (kinds.length === 0 ? events : events.filter((e) => kinds.includes(e.kind))),
    [events, kinds],
  );
  return (
    <section className="dom-events" aria-label="Observations">
      <div className="dialog-row">
        <div className="chip-group" role="group" aria-label="Kinds">
          {KINDS.map((k) => (
            <button
              key={k.id}
              type="button"
              className="chip"
              aria-pressed={kinds.includes(k.id)}
              onClick={() => toggleKind(k.id)}
            >
              {k.label}
            </button>
          ))}
        </div>
        <span className="spacer" />
        <span className="muted">{shown.length.toLocaleString()} shown</span>
        {events.length > 0 && (
          <Button size="sm" variant="ghost" icon="trash" onClick={() => void clear()}>
            Clear
          </Button>
        )}
      </div>
      {shown.length === 0 ? (
        <EmptyState title="Nothing observed yet">
          Open the lab browser with the DOM Inspector and use the application. Try the lab&apos;s
          intentionally vulnerable pages at /dom/.
        </EmptyState>
      ) : (
        <div className="endpoint-table-wrap">
          <table className="endpoint-table organizer-table">
            <thead>
              <tr>
                <th>Last seen</th>
                <th>Kind</th>
                <th>Source</th>
                <th>Sink</th>
                <th>Page</th>
                <th className="num">Hits</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((e) => {
                const k = kindOf(e.kind);
                return (
                  <tr
                    key={e.id}
                    className={selectedId === e.id ? "selected" : undefined}
                    aria-selected={selectedId === e.id}
                    onClick={() => select(e.id)}
                  >
                    <td className="num">{formatClock(e.lastAt)}</td>
                    <td>
                      <Badge tone={k?.tone ?? "neutral"}>{k?.label ?? e.kind}</Badge>
                      {e.canary && <Badge tone="warn">canary</Badge>}
                    </td>
                    <td className="mono">{e.source ?? "—"}</td>
                    <td className="mono">{e.sink}</td>
                    <td className="mono" title={e.frameUrl}>
                      {e.frameUrl}
                    </td>
                    <td className="num">{e.hits}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function Highlighted({ text, part }: { text: string; part: string | null }) {
  const m = splitMatch(text, part);
  if (!m) return <span className="mono selectable">{text}</span>;
  return (
    <span className="mono selectable">
      {m.before}
      <mark>{m.match}</mark>
      {m.after}
    </span>
  );
}

function EventDetail() {
  const events = useDomStore((s) => s.events);
  const selectedId = useDomStore((s) => s.selectedId);
  const e: DomEventRow | undefined = events.find((x) => x.id === selectedId);
  if (!e) return <EmptyState title="Select an observation" />;
  const k = kindOf(e.kind);
  return (
    <div className="dom-detail">
      <div className="exchange-summary">
        <Badge tone={k?.tone ?? "neutral"}>{k?.label ?? e.kind}</Badge>
        <span className="mono selectable">{e.frameUrl}</span>
        <span className="spacer" />
        <span className="muted">
          first {formatDateTime(e.firstAt)} · {e.hits} time{e.hits === 1 ? "" : "s"}
          {!e.topFrame && " · in a frame"}
        </span>
      </div>
      <div className="dom-flow" aria-label="Source to sink">
        <div className="dom-step">
          <span className="dom-step-label">Source</span>
          <strong className="mono">{e.source ?? "—"}</strong>
          {e.sourceValue !== null && <Highlighted text={e.sourceValue} part={e.matched} />}
        </div>
        <span className="dom-arrow" aria-hidden="true">
          →
        </span>
        <div className="dom-step">
          <span className="dom-step-label">Transformation</span>
          <strong>{e.transformation ?? "—"}</strong>
          {e.matched !== null && (
            <span className="muted">
              matched <code className="mono">{e.matched}</code>
            </span>
          )}
        </div>
        <span className="dom-arrow" aria-hidden="true">
          →
        </span>
        <div className="dom-step">
          <span className="dom-step-label">Sink</span>
          <strong className="mono">{e.sink}</strong>
          {e.kind === "messageListener" && (
            <span className={e.checksOrigin ? "muted" : "field-error"}>
              {e.checksOrigin ? "mentions the origin" : "never checks the sender's origin"}
            </span>
          )}
        </div>
      </div>
      <div className="dom-detail-body">
        <h3>{e.kind === "messageListener" ? "Listener code" : "Value at the sink"}</h3>
        <pre className="dom-value">
          <Highlighted text={e.value} part={e.matched} />
        </pre>
        {e.location && (
          <p>
            Code location: <code className="mono selectable">{e.location}</code>
          </p>
        )}
        {e.stack.length > 0 && (
          <>
            <h3>Stack</h3>
            <pre className="dom-stack mono selectable">{e.stack.join("\n")}</pre>
          </>
        )}
        {e.exchangeId !== null ? (
          <p className="dialog-row">
            <span className="muted">Page response stored as request #{e.exchangeId}.</span>
            <Button
              size="sm"
              variant="ghost"
              icon="repeater"
              onClick={() => sendToRepeater(e.exchangeId ?? 0)}
            >
              Send to Repeater
            </Button>
          </p>
        ) : (
          <p className="muted">
            The page response is not in the history, so no finding was created for it.
          </p>
        )}
      </div>
    </div>
  );
}
