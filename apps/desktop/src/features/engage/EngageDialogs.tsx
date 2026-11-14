import { useEffect, useMemo, useState } from "react";
import type { ExchangeDetail } from "../../bindings/ExchangeDetail";
import type { SiteFilter } from "../../bindings/SiteFilter";
import type { TargetAnalysis } from "../../bindings/TargetAnalysis";
import { Tabs } from "../../components/Tabs";
import { siteLabel } from "../../lib/filters";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { CodeView } from "../../components/message/CodeView";
import { ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import {
  DEFAULT_FRAME,
  csrfPoc,
  framePoc,
  framingVerdict,
  type FrameOptions,
} from "../../lib/engage";
import { copyText } from "../../services/clipboard";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useEngageStore } from "../../state/engage";

function useExchange(id: number) {
  const [state, setState] = useState<
    | { status: "loading" }
    | { status: "ready"; detail: ExchangeDetail }
    | { status: "error"; message: string }
  >({ status: "loading" });
  useEffect(() => {
    let live = true;
    api
      .exchangeDetail(id)
      .then((detail) => live && setState({ status: "ready", detail }))
      .catch((e: unknown) => live && setState({ status: "error", message: toIpcError(e).message }));
    return () => {
      live = false;
    };
  }, [id]);
  return state;
}

function headersOf(head: string): [string, string][] {
  const out: [string, string][] = [];
  for (const line of head.split(/\r?\n/).slice(1)) {
    const at = line.indexOf(":");
    if (at > 0) out.push([line.slice(0, at).trim(), line.slice(at + 1).trim()]);
  }
  return out;
}

function copy(text: string) {
  void copyText(text).then((ok) =>
    useAppStore.getState().notify(ok ? "info" : "error", ok ? "Copied" : "Could not copy"),
  );
}

function Frame({ label, children }: { label: string; children: React.ReactNode }) {
  const close = useEngageStore.getState().close;
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
        {children}
      </div>
    </div>
  );
}

/** Both engagement dialogs; at most one is open. */
export function EngageDialogs() {
  const csrfFor = useEngageStore((s) => s.csrfFor);
  const frameFor = useEngageStore((s) => s.frameFor);
  const analyzeFor = useEngageStore((s) => s.analyzeFor);
  if (analyzeFor !== null) return <AnalyzerDialog site={analyzeFor} />;
  if (csrfFor !== null) return <CsrfDialog key={csrfFor} id={csrfFor} />;
  if (frameFor !== null) return <FrameDialog key={frameFor} id={frameFor} />;
  return null;
}

function CsrfDialog({ id }: { id: number }) {
  const state = useExchange(id);
  const [autoSubmit, setAutoSubmit] = useState(false);
  const poc = useMemo(() => {
    if (state.status !== "ready") return null;
    const d = state.detail;
    const method = d.request.head.split(" ")[0] ?? "GET";
    return csrfPoc(
      {
        method,
        url: d.row.url,
        contentType: d.request.contentType,
        body: d.request.bodyText ?? "",
      },
      { autoSubmit },
    );
  }, [state, autoSubmit]);
  return (
    <Frame label={`CSRF proof of concept for request #${id}`}>
      {state.status === "loading" && <LoadingState />}
      {state.status === "error" && (
        <ErrorState title="Could not load the request" message={state.message} />
      )}
      {poc && (
        <>
          <div className="dialog-row">
            <Badge tone="accent">{poc.encoding}</Badge>
            <Switch
              label="Submit automatically when opened"
              checked={autoSubmit}
              onChange={setAutoSubmit}
            />
            <span className="spacer" />
            <Button variant="primary" icon="copy" onClick={() => copy(poc.html)}>
              Copy HTML
            </Button>
          </div>
          <ul className="engage-notes">
            {poc.notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
          <div className="engage-code">
            <CodeView text={poc.html} language="html" wrap label="CSRF PoC" />
          </div>
          <p className="muted">
            Host the page on another origin you control and open it in a browser logged in to the
            target (for example the lab browser), only for applications you are authorized to test.
          </p>
        </>
      )}
    </Frame>
  );
}

function FrameDialog({ id }: { id: number }) {
  const state = useExchange(id);
  const [o, setO] = useState<FrameOptions>(DEFAULT_FRAME);
  const verdict = useMemo(
    () =>
      state.status === "ready" && state.detail.response
        ? framingVerdict(headersOf(state.detail.response.head))
        : null,
    [state],
  );
  const html = state.status === "ready" ? framePoc(state.detail.row.url, o) : "";
  const num = (key: keyof FrameOptions, label: string, step = 1) => (
    <label>
      {label}
      <input
        type="number"
        step={step}
        value={o[key] as number}
        onChange={(e) => setO({ ...o, [key]: Number(e.target.value) })}
      />
    </label>
  );
  return (
    <Frame label={`FrameLab: clickjacking test for request #${id}`}>
      {state.status === "loading" && <LoadingState />}
      {state.status === "error" && (
        <ErrorState title="Could not load the request" message={state.message} />
      )}
      {state.status === "ready" && (
        <>
          <div className="engage-verdict">
            {verdict ? (
              <>
                <Badge tone={verdict.frameable ? "warn" : "ok"}>
                  {verdict.frameable ? "can be framed by other sites" : "framing is blocked"}
                </Badge>
                <ul className="engage-notes">
                  {verdict.reasons.map((r) => (
                    <li key={r} className="mono">
                      {r}
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p className="muted">No response stored: the verdict needs the response headers.</p>
            )}
          </div>
          <div className="engage-options">
            {num("opacity", "Opacity", 0.1)}
            {num("width", "Width")}
            {num("height", "Height")}
            {num("left", "Decoy left")}
            {num("top", "Decoy top")}
            <label>
              Decoy text
              <input value={o.decoy} onChange={(e) => setO({ ...o, decoy: e.target.value })} />
            </label>
            <Button variant="primary" icon="copy" onClick={() => copy(html)}>
              Copy HTML
            </Button>
          </div>
          <div className="engage-code">
            <CodeView text={html} language="html" wrap label="Clickjacking PoC" />
          </div>
          <p className="muted">
            Open the page from another origin; raise the opacity to align the decoy with the
            target&apos;s button, then lower it. A real issue needs a state-changing action
            reachable with clicks.
          </p>
        </>
      )}
    </Frame>
  );
}

type AnalyzerTab = "parameters" | "comments" | "scripts";

function AnalyzerDialog({ site }: { site: SiteFilter }) {
  const [state, setState] = useState<
    | { status: "loading" }
    | { status: "ready"; a: TargetAnalysis }
    | { status: "error"; message: string }
  >({ status: "loading" });
  const [tab, setTab] = useState<AnalyzerTab>("parameters");
  useEffect(() => {
    let live = true;
    api
      .targetAnalysis(site)
      .then((a) => live && setState({ status: "ready", a }))
      .catch((e: unknown) => live && setState({ status: "error", message: toIpcError(e).message }));
    return () => {
      live = false;
    };
  }, [site]);
  return (
    <Frame label={`Target analyzer: ${siteLabel(site)}`}>
      {state.status === "loading" && <LoadingState />}
      {state.status === "error" && <ErrorState title="Could not analyze" message={state.message} />}
      {state.status === "ready" && (
        <>
          <p className="muted">
            From {state.a.exchanges.toLocaleString()} stored requests (nothing is sent):{" "}
            {state.a.urls.toLocaleString()} URLs, {state.a.dynamicUrls.toLocaleString()} taking
            parameters, {state.a.staticUrls.toLocaleString()} static files.
            {state.a.truncated && " Lists were cut at their limit."}
          </p>
          <Tabs<AnalyzerTab>
            label="Analysis"
            value={tab}
            onChange={setTab}
            items={[
              { id: "parameters", label: `Parameters (${state.a.parameters.length})` },
              { id: "comments", label: `Comments (${state.a.comments.length})` },
              { id: "scripts", label: `Scripts (${state.a.scripts.length})` },
            ]}
          />
          <div className="endpoint-table-wrap analyzer-table">
            {tab === "parameters" ? (
              <table className="endpoint-table organizer-table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Where</th>
                    <th className="num">URLs</th>
                    <th>Values seen</th>
                  </tr>
                </thead>
                <tbody>
                  {state.a.parameters.map((p) => (
                    <tr key={p.name}>
                      <td className="mono selectable">{p.name}</td>
                      <td>{p.locations.join(", ")}</td>
                      <td className="num">{p.urls}</td>
                      <td className="mono selectable">{p.samples.join("  ·  ")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <table className="endpoint-table organizer-table">
                <thead>
                  <tr>
                    <th>{tab === "comments" ? "Comment" : "Script"}</th>
                    <th className="num">Pages</th>
                    <th>First seen on</th>
                  </tr>
                </thead>
                <tbody>
                  {(tab === "comments" ? state.a.comments : state.a.scripts).map((c) => (
                    <tr key={c.text}>
                      <td className="mono selectable">{c.text}</td>
                      <td className="num">{c.pages}</td>
                      <td className="mono selectable">{c.url}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </>
      )}
    </Frame>
  );
}
