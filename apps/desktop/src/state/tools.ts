// "Send to" actions shared by every view that shows traffic.
import { useSearchStore } from "./search";
import { useEngageStore } from "./engage";
import type { MessageView } from "../bindings/MessageView";
import { base64ToBytes, bytesToText } from "../lib/bytes";
import { toCurl } from "../lib/curl";
import { toFetch, toPython } from "../lib/codegen";
import { copyText } from "../services/clipboard";
import { api, toIpcError } from "../services/ipc";
import { useAgentStore } from "./agent";
import { useAiStore } from "./ai";
import { useAppStore } from "./app";
import { usableScripts, useScriptsStore } from "./scripts";
import { useComparerStore } from "./comparer";
import { type MenuEntry, SEPARATOR } from "./contextMenu";
import { useDecoderStore } from "./decoder";
import { useFindingsStore } from "./findings";
import { useOrganizerStore } from "./organizer";
import { useRepeaterStore } from "./repeater";
import { useScopeStore } from "./scope";
import { useFuzzerStore } from "./fuzzer";
import { useSequencerStore } from "./sequencer";
import { useTasksStore } from "./tasks";

/** Opens the Decoder with `text` and a proposed decoding chain. */
export function sendToDecoder(text: string) {
  useDecoderStore.getState().load(text);
  useAppStore.getState().setView("decoder");
}

/** Adds a message (exact bytes as text) to the Comparer. */
export function sendToComparer(label: string, message: MessageView) {
  useComparerStore.getState().add(label, bytesToText(base64ToBytes(message.rawBase64)));
  useAppStore.getState().notify("info", `Sent to Comparer: ${label}`);
}

/** Copies a history entry into the Organizer. */
export async function sendToOrganizer(exchangeId: number) {
  const { notify } = useAppStore.getState();
  try {
    await api.organizerAdd(exchangeId);
    notify("info", `Request #${exchangeId} added to the Organizer`);
    void useOrganizerStore.getState().load();
  } catch (e) {
    notify("error", `Could not add to the Organizer: ${toIpcError(e).message}`);
  }
}

/** Opens a stored request in a new Repeater tab. */
export function sendToRepeater(exchangeId: number) {
  void useRepeaterStore.getState().openExchange(exchangeId);
}

/** Opens a request that is not stored (e.g. a paused one) in a new Repeater tab. */
export function sendTextToRepeater(url: string, request: string) {
  void useRepeaterStore.getState().openRequest(url, request);
}

/** Sends the stored request or response of an exchange to the Comparer. */
async function compareStored(exchangeId: number, part: "request" | "response") {
  try {
    const detail = await api.exchangeDetail(exchangeId);
    const message = part === "request" ? detail.request : detail.response;
    if (!message) {
      useAppStore.getState().notify("error", `Request #${exchangeId} has no ${part}`);
      return;
    }
    sendToComparer(`#${exchangeId} ${part}`, message);
  } catch (e) {
    useAppStore
      .getState()
      .notify("error", `Could not load #${exchangeId}: ${toIpcError(e).message}`);
  }
}

/** Copies the stored request as a curl command, Python or fetch code. */
async function copyCode(exchangeId: number, url: string, kind: "curl" | "python" | "fetch") {
  const { notify } = useAppStore.getState();
  try {
    const detail = await api.exchangeDetail(exchangeId);
    const raw = base64ToBytes(detail.request.rawBase64);
    const code =
      kind === "curl"
        ? toCurl(url, raw)
        : kind === "python"
          ? toPython(url, raw)
          : toFetch(url, raw);
    const ok = await copyText(code);
    notify(
      ok ? "info" : "error",
      ok
        ? `${kind === "curl" ? "curl command" : kind === "python" ? "Python code" : "fetch code"} copied`
        : "Could not copy to the clipboard",
    );
  } catch (e) {
    notify("error", `Could not load #${exchangeId}: ${toIpcError(e).message}`);
  }
}

/** Opens the stored request of an exchange in the Decoder. */
async function decodeStored(exchangeId: number) {
  try {
    const detail = await api.exchangeDetail(exchangeId);
    sendToDecoder(bytesToText(base64ToBytes(detail.request.rawBase64)));
  } catch (e) {
    useAppStore
      .getState()
      .notify("error", `Could not load #${exchangeId}: ${toIpcError(e).message}`);
  }
}

/** Where a crawl from a request starts: its URL without the query. */
export function crawlStart(url: string): string {
  return url.split(/[?#]/, 1)[0] ?? url;
}

/** Runs a project search for `text` everywhere. */
export function searchProjectFor(text: string) {
  const search = useSearchStore.getState();
  search.setQuery({ text: text.slice(0, 4096), regex: false });
  useAppStore.getState().setView("search");
  void search.run();
}

/** Searches stored responses for links to `url` (its path, or the host for "/"). */
export function findReferences(url: string) {
  const u = new URL(url);
  const text = u.pathname === "/" ? u.host : u.pathname;
  const search = useSearchStore.getState();
  search.setQuery({
    text,
    regex: false,
    inUrls: false,
    inRequestHeaders: false,
    inRequestBodies: false,
    inResponseHeaders: true,
    inResponseBodies: true,
    inNotes: false,
    inWebsockets: false,
  });
  useAppStore.getState().setView("search");
  void search.run();
}

/** The folder of a URL (`https://a/b/c?x` → `https://a/b/`). */
export function folderOf(url: string): string {
  const clean = crawlStart(url);
  const at = clean.lastIndexOf("/");
  return at > clean.indexOf("//") + 1 ? clean.slice(0, at + 1) : `${clean}/`;
}

/** What a "send to" menu needs to know about a stored request. */
export interface ExchangeRef {
  id: number;
  url: string;
  /** Unknown (undefined) shows both scope actions. */
  inScope?: boolean;
}

/** The "send to" actions shared by every view that lists stored requests
 * (the context menu of HTTP history, the Logger, Search, the Organizer…). */
/** Sends the introspection query on a stored GraphQL request's endpoint
 * and opens the schema in the API workspace. */
async function introspectGraphql(exchangeId: number) {
  const { notify, setView } = useAppStore.getState();
  try {
    const info = await api.graphqlIntrospect(exchangeId);
    notify("info", `GraphQL schema read: ${info.endpointCount} operation(s)`);
    setView("api");
  } catch (e) {
    notify("error", `GraphQL introspection: ${toIpcError(e).message}`);
  }
}

export function exchangeMenu(ex: ExchangeRef, extra: MenuEntry[] = []): MenuEntry[] {
  const { notify } = useAppStore.getState();
  const scope = useScopeStore.getState();
  const entries: MenuEntry[] = [
    { heading: `Request #${ex.id}` },
    {
      label: "Send to Repeater",
      icon: "repeater",
      onSelect: () => sendToRepeater(ex.id),
    },
    {
      label: "Send to Fuzzer",
      icon: "bolt",
      onSelect: () => void useFuzzerStore.getState().openExchange(ex.id),
    },
    {
      label: "Send to Sequencer",
      icon: "dice",
      onSelect: () => void useSequencerStore.getState().openExchange(ex.id),
    },
    {
      label: "Explain with ChosAI…",
      icon: "bolt",
      onSelect: () =>
        useAiStore.getState().open({
          title: `Request #${ex.id}`,
          attachments: [{ kind: "exchange", id: ex.id }],
          task: "explainExchange",
        }),
    },
    {
      label: "Give to ChosAgent…",
      icon: "agent",
      onSelect: () => useAgentStore.getState().startWith(ex.id),
    },
    {
      label: "Get GraphQL schema (introspection)",
      icon: "api",
      onSelect: () => void introspectGraphql(ex.id),
    },
    {
      label: "Add to Organizer",
      icon: "bookmark",
      onSelect: () => void sendToOrganizer(ex.id),
    },
    {
      label: "Document as finding…",
      icon: "shield",
      onSelect: () => useFindingsStore.getState().openAdd(ex.id, `request #${ex.id}`),
    },
    SEPARATOR,
    {
      label: "Compare request",
      icon: "compare",
      onSelect: () => void compareStored(ex.id, "request"),
    },
    {
      label: "Compare response",
      icon: "compare",
      onSelect: () => void compareStored(ex.id, "response"),
    },
    { label: "Decode request", icon: "decoder", onSelect: () => void decodeStored(ex.id) },
    {
      label: "Crawl from this URL…",
      icon: "spider",
      onSelect: () => void useTasksStore.getState().openCrawl([crawlStart(ex.url)]),
    },
    {
      label: "Find references to this URL",
      icon: "search",
      onSelect: () => findReferences(ex.url),
    },
    {
      label: "Analyze target…",
      icon: "target",
      onSelect: () => {
        const u = new URL(ex.url);
        const port = u.port ? Number(u.port) : u.protocol === "https:" ? 443 : 80;
        useEngageStore
          .getState()
          .openAnalyzer({ scheme: u.protocol.replace(":", ""), host: u.hostname, port, path: "/" });
      },
    },
    {
      label: "Generate CSRF PoC…",
      icon: "file",
      onSelect: () => useEngageStore.getState().openCsrf(ex.id),
    },
    {
      label: "Test clickjacking (FrameLab)…",
      icon: "layers",
      onSelect: () => useEngageStore.getState().openFrame(ex.id),
    },
    {
      label: "Discover content here…",
      icon: "search",
      onSelect: () => void useTasksStore.getState().openDiscovery(folderOf(ex.url)),
    },
    SEPARATOR,
  ];
  if (ex.inScope !== true) {
    entries.push({
      label: "Add host to scope",
      icon: "target",
      onSelect: () => void scope.addUrl(ex.url, false, false),
    });
  }
  if (ex.inScope !== false) {
    entries.push({
      label: "Exclude host from scope",
      icon: "target",
      onSelect: () => void scope.addUrl(ex.url, true, false),
    });
  }
  entries.push({
    label: "Copy as curl command",
    icon: "copy",
    onSelect: () => void copyCode(ex.id, ex.url, "curl"),
  });
  entries.push({
    label: "Copy as Python (requests)",
    icon: "copy",
    onSelect: () => void copyCode(ex.id, ex.url, "python"),
  });
  entries.push({
    label: "Copy as JavaScript (fetch)",
    icon: "copy",
    onSelect: () => void copyCode(ex.id, ex.url, "fetch"),
  });
  entries.push({
    label: "Copy URL",
    icon: "copy",
    onSelect: () =>
      void copyText(ex.url).then((ok) =>
        notify(ok ? "info" : "error", ok ? "URL copied" : "Could not copy to the clipboard"),
      ),
  });
  const scripts = useScriptsStore.getState();
  const actions = usableScripts(scripts.library, "action");
  if (scripts.pluginActions.length > 0) {
    entries.push(SEPARATOR, { heading: "Plugin actions" });
    for (const a of scripts.pluginActions) {
      entries.push({
        label: `${a.label} (${a.pluginName})`,
        icon: "puzzle",
        onSelect: () => void scripts.runPluginAction(a, ex.id),
      });
    }
  }
  if (actions.length > 0) {
    entries.push(SEPARATOR, { heading: "Run script" });
    for (const a of actions) {
      entries.push({
        label: a.name,
        icon: "code",
        onSelect: () => void scripts.runAction(a.id, a.name, ex.id),
      });
    }
  }
  if (extra.length > 0) entries.push(SEPARATOR, ...extra);
  return entries;
}
