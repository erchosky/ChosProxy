import { useMemo, useState } from "react";
import type { MessageView } from "../../bindings/MessageView";
import { base64ToBytes, bytesToText } from "../../lib/bytes";
import { formatBytes } from "../../lib/format";
import { graphqlParts } from "../../lib/graphql";
import { languageOf, prettyBody } from "../../lib/pretty";
import { copyText } from "../../services/clipboard";
import { useAiStore } from "../../state/ai";
import { sendToComparer, sendToDecoder } from "../../state/tools";
import { Button } from "../Button";
import { Badge } from "../Badge";
import { IconButton } from "../Button";
import { EmptyState } from "../States";
import { Segmented, type TabItem } from "../Tabs";
import { CodeView } from "./CodeView";
import { HexView } from "./HexView";
import { InspectorPanel } from "./InspectorPanel";
import { copyAsEntries } from "../../lib/copyAs";
import { RenderView } from "./RenderView";

type Mode = "pretty" | "raw" | "hex" | "inspector" | "render" | "graphql";

const MODES: readonly TabItem<Mode>[] = [
  { id: "pretty", label: "Pretty", title: "Head plus decoded, formatted body" },
  { id: "raw", label: "Raw", title: "Exact bytes as on the wire (text)" },
  { id: "hex", label: "Hex", title: "Exact bytes as on the wire (hex dump)" },
  { id: "inspector", label: "Inspector", title: "Parameters, cookies and headers" },
];
const GRAPHQL_MODES: readonly TabItem<Mode>[] = [
  ...MODES,
  { id: "graphql", label: "GraphQL", title: "The GraphQL operation and its variables" },
];
const HTML_MODES: readonly TabItem<Mode>[] = [
  ...MODES,
  { id: "render", label: "Render", title: "Static, sandboxed preview (no scripts, no network)" },
];

interface MessageViewerProps {
  title: string;
  message: MessageView | null;
  emptyText: string;
  /** Shows a "Compare" action that sends the message to the Comparer under this label. */
  compareLabel?: string;
  /** URL of a request message: enables "Copy as" in the editor menu. */
  url?: string;
}

/** Read-only viewer for one HTTP message, reused by every tool. */
export function MessageViewer({
  title,
  message,
  emptyText,
  compareLabel,
  url,
}: MessageViewerProps) {
  const [mode, setMode] = useState<Mode>("pretty");
  const [selection, setSelection] = useState("");
  const [wrap, setWrap] = useState(true);

  const raw = useMemo(() => (message ? base64ToBytes(message.rawBase64) : null), [message]);
  const requestMenu =
    url && raw && message && !message.head.startsWith("HTTP/")
      ? () => copyAsEntries(url, () => raw)
      : undefined;
  const rawText = useMemo(() => (raw ? bytesToText(raw) : ""), [raw]);
  const pretty = useMemo(() => {
    if (!message) return null;
    if (message.bodySize === 0) {
      return { kind: "headOnly" as const, head: message.head.trimEnd() };
    }
    if (message.bodyText === null) return null;
    const language = languageOf(message.contentType, message.bodyText);
    return {
      kind: "withBody" as const,
      head: message.head.trimEnd(),
      body: prettyBody(message.bodyText, language),
      language,
    };
  }, [message]);
  const isRequest = message !== null && !message.head.startsWith("HTTP/");
  const canRender = !isRequest && pretty?.kind === "withBody" && pretty.language === "html";
  const graphql = useMemo(
    () => (isRequest && message ? graphqlParts(message.bodyText) : null),
    [isRequest, message],
  );
  const shownMode =
    (mode === "render" && !canRender) || (mode === "graphql" && !graphql) ? "pretty" : mode;
  const modes = canRender ? HTML_MODES : graphql ? GRAPHQL_MODES : MODES;

  return (
    <section className="message-viewer" aria-label={title}>
      <header className="message-toolbar">
        <strong>{title}</strong>
        {message && (
          <>
            <Segmented<Mode>
              items={modes}
              value={shownMode}
              onChange={setMode}
              label={`${title} view`}
            />
            <span className="message-meta">
              {formatBytes(message.wireSize ?? message.rawSize)}
              {message.decodedFrom.length > 0 && (
                <Badge tone="accent" title="Codings removed for the Pretty view">
                  decoded: {message.decodedFrom.join(", ")}
                </Badge>
              )}
              {message.captureTruncated && (
                <Badge
                  tone="warn"
                  title="Stored copy cut at the capture limit (Settings → Capture)"
                >
                  capture truncated
                </Badge>
              )}
              {message.rawDisplayTruncated && (
                <Badge tone="warn" title="Only the first 4 MB are shown">
                  display truncated
                </Badge>
              )}
              {message.decodeNote && (
                <Badge tone="warn" title={message.decodeNote}>
                  decode issue
                </Badge>
              )}
            </span>
            <span className="spacer" />
            <IconButton
              icon="wrap"
              label={wrap ? "Disable line wrap" : "Enable line wrap"}
              aria-pressed={wrap}
              onClick={() => setWrap((w) => !w)}
              disabled={shownMode === "hex" || shownMode === "inspector" || shownMode === "render"}
            />
            <Button
              size="sm"
              variant="ghost"
              title={
                selection ? "Open the selected text in the Decoder" : "Open the body in the Decoder"
              }
              onClick={() =>
                // Selection first, then the decoded body, else the whole message.
                sendToDecoder(selection || message.bodyText || rawText)
              }
            >
              Decode{selection ? " selection" : ""}
            </Button>
            {selection && (
              <Button
                size="sm"
                variant="ghost"
                title="Ask ChosAI about the selected text"
                onClick={() =>
                  useAiStore.getState().open({
                    title: `${title}: selection`,
                    attachments: [{ kind: "text", label: `Selected in ${title}`, text: selection }],
                    task:
                      pretty?.kind === "withBody" && pretty.language === "javascript"
                        ? "explainJavascript"
                        : "ask",
                  })
                }
              >
                ChosAI
              </Button>
            )}
            {compareLabel && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => sendToComparer(compareLabel, message)}
              >
                Compare
              </Button>
            )}
            <IconButton
              icon="copy"
              label="Copy raw message"
              onClick={() => void copyText(rawText)}
            />
          </>
        )}
      </header>
      <div className="message-body">
        {!message || !raw ? (
          <EmptyState title={emptyText} />
        ) : shownMode === "hex" ? (
          <HexView bytes={raw} />
        ) : shownMode === "inspector" ? (
          <InspectorPanel text={rawText} />
        ) : shownMode === "graphql" && graphql ? (
          <CodeView
            text={`${graphql.batch > 1 ? `# Batch of ${graphql.batch} operations; the first is shown\n` : ""}${
              graphql.operationName ? `# Operation: ${graphql.operationName}\n` : ""
            }${graphql.query}\n\n# Variables\n${graphql.variables}`}
            language="text"
            wrap={wrap}
            label={`${title} GraphQL`}
            onSelection={setSelection}
          />
        ) : shownMode === "render" && pretty?.kind === "withBody" ? (
          <RenderView html={message.bodyText ?? pretty.body} title={title} />
        ) : shownMode === "raw" ? (
          <CodeView
            text={rawText}
            language="http"
            wrap={wrap}
            label={`${title} raw`}
            onSelection={setSelection}
            menu={requestMenu}
          />
        ) : pretty === null ? (
          <EmptyState title="Binary body">
            The body is not text ({formatBytes(message.bodySize)}). Use the Hex view.
          </EmptyState>
        ) : pretty.kind === "withBody" ? (
          <div className="pretty-split">
            <div className="pretty-head">
              <CodeView
                text={pretty.head}
                language="http"
                wrap={wrap}
                label={`${title} head`}
                onSelection={setSelection}
                menu={requestMenu}
              />
            </div>
            <div className="pretty-body">
              <CodeView
                text={pretty.body}
                language={pretty.language}
                wrap={wrap}
                label={`${title} body`}
                onSelection={setSelection}
                menu={requestMenu}
              />
            </div>
          </div>
        ) : (
          <CodeView
            text={pretty.head}
            language="http"
            wrap={wrap}
            label={`${title} head`}
            onSelection={setSelection}
          />
        )}
      </div>
    </section>
  );
}
