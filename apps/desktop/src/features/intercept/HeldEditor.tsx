import { useMemo, useState } from "react";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { CodeView } from "../../components/message/CodeView";
import { graphqlParts, withGraphql } from "../../lib/graphql";
import { InspectorPanel } from "../../components/message/InspectorPanel";
import { HexView } from "../../components/message/HexView";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { base64ToBytes } from "../../lib/bytes";
import { formatBytes } from "../../lib/format";
import { heldKindLabel, isWebSocketKind } from "../../lib/heldKind";
import { bodyOf, headersOf, reformatBody, replaceBody } from "../../lib/httpText";
import { copyText } from "../../services/clipboard";
import { useInterceptStore } from "../../state/intercept";
import { useComparerStore } from "../../state/comparer";
import { useAppStore } from "../../state/app";
import { sendTextToRepeater, sendToDecoder } from "../../state/tools";

/** Editor and actions for the selected paused message. */
export function HeldEditor() {
  const detail = useInterceptStore((s) => s.detail);
  const loading = useInterceptStore((s) => s.loading);
  const draft = useInterceptStore((s) => s.draft);
  const updateLength = useInterceptStore((s) => s.updateContentLength);
  const { setDraft, setUpdateContentLength, forward, drop, unpack } = useInterceptStore.getState();
  const bytes = useMemo(() => (detail ? base64ToBytes(detail.rawBase64) : null), [detail]);
  const [showInspector, setShowInspector] = useState(false);
  const [selection, setSelection] = useState("");
  const [showGraphql, setShowGraphql] = useState(false);

  if (loading && !detail) return <LoadingState />;
  if (!detail || !bytes) return <EmptyState title="Nothing selected" />;
  const isRequest = detail.summary.kind === "request";
  const isWebSocket = isWebSocketKind(detail.summary.kind);
  const current = draft ?? detail.text ?? "";
  const isGraphql = isRequest && graphqlParts(bodyOf(current)) !== null;
  const reformat = (mode: "pretty" | "compact") => {
    const next = reformatBody(current, mode);
    if (next === null) useAppStore.getState().notify("info", "The body is not JSON or XML");
    else if (next !== current) setDraft(next);
  };

  const editor = (
    <section className="held-editor" aria-label="Paused message">
      <header className="message-toolbar">
        <Badge tone={isRequest ? "accent" : isWebSocket ? "ok" : "warn"}>
          {heldKindLabel(detail.summary.kind)}
        </Badge>
        <span className="mono held-url" title={detail.summary.url}>
          {detail.summary.url}
        </span>
        <span className="muted">{formatBytes(detail.summary.size)}</span>
        {draft !== null && <Badge tone="accent">edited</Badge>}
        <span className="spacer" />
        {isRequest && detail.text !== null && (
          <Button
            size="sm"
            variant={showInspector ? "secondary" : "ghost"}
            aria-pressed={showInspector}
            onClick={() => setShowInspector((v) => !v)}
            title="Edit parameters, cookies and headers as a table"
          >
            Inspector
          </Button>
        )}
        {isGraphql && (
          <Button
            size="sm"
            variant={showGraphql ? "secondary" : "ghost"}
            aria-pressed={showGraphql}
            onClick={() => setShowGraphql((v) => !v)}
            title="Read and edit the GraphQL query and its variables"
          >
            GraphQL
          </Button>
        )}
        {!isWebSocket && detail.text !== null && (
          <>
            <Button
              size="sm"
              variant="ghost"
              title="Indent the JSON or XML body to edit it comfortably (Content-Length follows)"
              onClick={() => reformat("pretty")}
            >
              Format body
            </Button>
            <Button
              size="sm"
              variant="ghost"
              title="Remove the indentation of a JSON or XML body (Content-Length follows)"
              onClick={() => reformat("compact")}
            >
              Compact
            </Button>
          </>
        )}
        <Button size="sm" variant="ghost" icon="copy" onClick={() => void copyText(current)}>
          Raw
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={detail.text === null}
          title={
            selection ? "Open the selected text in the Decoder" : "Open the message in the Decoder"
          }
          onClick={() => sendToDecoder(selection || current)}
        >
          Decode{selection ? " selection" : ""}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={detail.text === null}
          onClick={() => {
            useComparerStore
              .getState()
              .add(
                `Paused ${heldKindLabel(detail.summary.kind)} ${detail.summary.url}${draft !== null ? " (edited)" : ""}`,
                current,
              );
            useAppStore.getState().notify("info", "Sent to Comparer");
          }}
        >
          Compare
        </Button>
        {isRequest && (
          <Button
            size="sm"
            variant="ghost"
            icon="repeater"
            disabled={detail.text === null}
            title="Open this request (with your edits) in a new Repeater tab; it stays paused here"
            onClick={() => sendTextToRepeater(detail.summary.url, current)}
          >
            Repeater
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          icon="copy"
          onClick={() => void copyText(detail.summary.url)}
        >
          URL
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="copy"
          disabled={isWebSocket}
          onClick={() => void copyText(headersOf(current))}
        >
          Headers
        </Button>
      </header>
      <div className="message-body">
        {detail.text !== null && isGraphql && showGraphql ? (
          <SplitPane
            direction="horizontal"
            initial={0.5}
            first={
              <CodeView
                key={`${detail.summary.id}:${detail.rawBase64.length}`}
                text={current}
                language="http"
                wrap
                editable
                label="Edit paused message"
                onChange={setDraft}
                onSubmit={() => void forward(false)}
                onSelection={setSelection}
              />
            }
            second={<GraphqlEditor key={bodyOf(current)} text={current} onChange={setDraft} />}
          />
        ) : detail.text !== null && isRequest && showInspector ? (
          <SplitPane
            direction="horizontal"
            initial={0.6}
            first={
              <CodeView
                key={`${detail.summary.id}:${detail.rawBase64.length}`}
                text={current}
                language={isWebSocket ? "text" : "http"}
                wrap
                editable
                label="Edit paused message"
                onChange={setDraft}
                onSubmit={() => void forward(false)}
                onSelection={setSelection}
              />
            }
            second={<InspectorPanel text={current} onChange={setDraft} />}
          />
        ) : detail.text !== null ? (
          <CodeView
            // Re-created per message and after unpacking.
            key={`${detail.summary.id}:${detail.rawBase64.length}`}
            text={current}
            language={isWebSocket ? "text" : "http"}
            wrap
            editable
            label="Edit paused message"
            onChange={setDraft}
            onSubmit={() => void forward(false)}
            onSelection={setSelection}
          />
        ) : (
          <div className="held-binary">
            <p className="callout callout-warn">Not editable as text: {detail.notEditable}.</p>
            <HexView bytes={bytes} />
          </div>
        )}
      </div>
      <footer className="held-actions">
        <Button
          variant="primary"
          icon="play"
          onClick={() => void forward(false)}
          title="Ctrl/Cmd+Enter"
        >
          Forward
        </Button>
        {isRequest && (
          <Button
            onClick={() => void forward(true)}
            title="Forward and pause the response to this request"
          >
            Forward &amp; intercept response
          </Button>
        )}
        <Button variant="danger" icon="trash" onClick={() => void drop()}>
          Drop
        </Button>
        {detail.packed && (
          <Button
            icon="file"
            onClick={() => void unpack()}
            title="Remove chunked framing and compression so the body can be edited"
          >
            Unpack body
          </Button>
        )}
        <span className="spacer" />
        {!isWebSocket && (
          <Switch
            label="Update Content-Length"
            checked={updateLength}
            onChange={setUpdateContentLength}
            disabled={detail.text === null}
          />
        )}
      </footer>
    </section>
  );

  if (isRequest || !detail.request) return editor;
  return (
    <SplitPane
      direction="vertical"
      initial={0.7}
      first={editor}
      second={<MessageViewer title="Request" message={detail.request} emptyText="No request" />}
    />
  );
}

/** The GraphQL query and variables of a paused request, pretty and
 * editable; "Apply" writes them back into the JSON body. */
function GraphqlEditor({ text, onChange }: { text: string; onChange(next: string): void }) {
  const parts = graphqlParts(bodyOf(text));
  const [query, setQuery] = useState(parts?.query ?? "");
  const [variables, setVariables] = useState(parts?.variables ?? "{}");
  const [error, setError] = useState<string | null>(null);
  if (!parts) return <EmptyState title="Not a GraphQL request" />;
  const changed = query !== parts.query || variables !== parts.variables;
  const apply = () => {
    const next = withGraphql(bodyOf(text), query, variables);
    if (next === null) return setError("The body is no longer a GraphQL operation.");
    if ("error" in next) return setError(next.error);
    const message = replaceBody(text, next.body);
    if (message === null) return setError("The body of this request cannot be replaced.");
    setError(null);
    onChange(message);
  };
  return (
    <section className="graphql-editor" aria-label="GraphQL">
      <div className="dialog-row">
        <strong>{parts.operationName ?? "GraphQL operation"}</strong>
        {parts.batch > 1 && <Badge>first of {parts.batch}</Badge>}
        <span className="spacer" />
        <Button
          size="sm"
          variant="ghost"
          disabled={!changed}
          onClick={() => {
            setQuery(parts.query);
            setVariables(parts.variables);
            setError(null);
          }}
        >
          Reset
        </Button>
        <Button size="sm" variant="primary" disabled={!changed} onClick={apply}>
          Apply to the request
        </Button>
      </div>
      <label className="field-stack">
        Query
        <textarea
          className="mono"
          rows={12}
          spellCheck={false}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </label>
      <label className="field-stack">
        Variables (JSON)
        <textarea
          className="mono"
          rows={6}
          spellCheck={false}
          value={variables}
          onChange={(e) => setVariables(e.target.value)}
        />
      </label>
      {error && <p className="field-error">{error}</p>}
    </section>
  );
}
