import { useEffect, useMemo, useState } from "react";
import { LaunchPoolSelect } from "../../components/LaunchPoolSelect";
import type { ApiDefinitionDetail } from "../../bindings/ApiDefinitionDetail";
import type { ApiDefinitionInfo } from "../../bindings/ApiDefinitionInfo";
import type { ApiEndpoint } from "../../bindings/ApiEndpoint";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { EmptyState, ErrorState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { formatDateTime } from "../../lib/format";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useFuzzerStore } from "../../state/fuzzer";
import { useRepeaterStore } from "../../state/repeater";
import { useTasksStore } from "../../state/tasks";
import { ScanDialog } from "../tasks/ScanDialog";
import { GraphqlPanel } from "./GraphqlPanel";

const FORMATS: Record<string, string> = {
  openApi3: "OpenAPI 3",
  swagger2: "Swagger 2",
  postman: "Postman",
  wsdl: "WSDL",
  graphql: "GraphQL",
};

const SAFE = new Set(["GET", "HEAD", "OPTIONS"]);

/** Whether sending an endpoint should only read (GraphQL queries, safe methods). */
export function isReadOnly(e: ApiEndpoint): boolean {
  return e.readOnly ?? SAFE.has(e.method.toUpperCase());
}

/** Endpoints whose method, path, name or group contain `text`. */
export function filterEndpoints(endpoints: ApiEndpoint[], text: string): number[] {
  const needle = text.trim().toLowerCase();
  return endpoints
    .map((e, i) => ({ e, i }))
    .filter(
      ({ e }) =>
        needle === "" ||
        `${e.method} ${e.path} ${e.name} ${e.group} ${e.url}`.toLowerCase().includes(needle),
    )
    .map(({ i }) => i);
}

/** The API workspace (M27): imported definitions and their endpoints. */
export function ApiView() {
  const [definitions, setDefinitions] = useState<ApiDefinitionInfo[] | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [busy, setBusy] = useState(false);

  const reload = (select?: number) =>
    api
      .apiDefinitions()
      .then((list) => {
        setDefinitions(list);
        setSelected((current) => select ?? current ?? list[0]?.id ?? null);
      })
      .catch((e: unknown) => setError(toIpcError(e).message));

  useEffect(() => {
    api
      .apiDefinitions()
      .then((list) => {
        setDefinitions(list);
        setSelected(list[0]?.id ?? null);
      })
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, []);

  async function importFile() {
    const path = await dialogs.chooseApiDefinition();
    if (!path) return;
    setBusy(true);
    setError(null);
    try {
      const info = await api.apiImport(path, baseUrl.trim() || null);
      useAppStore
        .getState()
        .notify("info", `Imported ${info.name}: ${info.endpointCount} endpoint(s)`);
      await reload(info.id);
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="api-view">
      <aside className="api-list" aria-label="API definitions">
        <div className="filters">
          <Button variant="primary" icon="plus" disabled={busy} onClick={() => void importFile()}>
            Import…
          </Button>
        </div>
        <label className="field-stack">
          Base URL (for relative servers)
          <input
            className="mono"
            placeholder="https://api.target.example"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
          />
        </label>
        <p className="muted">
          OpenAPI 3 and Swagger 2 (JSON or YAML), Postman collections v2 and WSDL 1.1. Only the file
          is read: nothing is fetched or sent on import.
        </p>
        {error && <p className="callout callout-danger">{error}</p>}
        {definitions?.length === 0 && (
          <EmptyState title="No API definitions">Import a definition file to begin.</EmptyState>
        )}
        <ul className="api-defs">
          {definitions?.map((d) => (
            <li key={d.id}>
              <button
                type="button"
                className={d.id === selected ? "api-def selected" : "api-def"}
                onClick={() => setSelected(d.id)}
              >
                <strong>{d.title || d.name}</strong>
                <span className="muted">
                  {FORMATS[d.format] ?? d.format} · {d.endpointCount} endpoint(s) ·{" "}
                  {formatDateTime(d.createdAt)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <section className="api-detail">
        {selected !== null && (
          <DefinitionPanel
            key={selected}
            id={selected}
            onDeleted={() => {
              setSelected(null);
              void reload();
            }}
          />
        )}
      </section>
    </div>
  );
}

function DefinitionPanel({ id, onDeleted }: { id: number; onDeleted(): void }) {
  const [detail, setDetail] = useState<ApiDefinitionDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [sending, setSending] = useState(false);
  const [scanning, setScanning] = useState(false);

  useEffect(() => {
    api
      .apiDefinition(id)
      .then(setDetail)
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, [id]);

  const shown = useMemo(
    () => (detail ? filterEndpoints(detail.endpoints, filter) : []),
    [detail, filter],
  );

  if (error) return <ErrorState title="Could not load the definition" message={error} />;
  if (!detail) return null;
  const { info } = detail;

  async function open(index: number, tool: "repeater" | "fuzzer") {
    try {
      const req = await api.apiRequest(id, index);
      if (tool === "repeater") {
        const t = req.target;
        await useRepeaterStore
          .getState()
          .openRequest(`${t.scheme}://${t.host}:${t.port}/`, req.request);
        useAppStore.getState().setView("repeater");
      } else {
        await useFuzzerStore.getState().openRequest(req.target, req.request);
      }
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    }
  }

  return (
    <div className="api-panel">
      <div className="filters">
        <strong>{info.title || info.name}</strong>
        {info.version && <Badge>v{info.version}</Badge>}
        <Badge tone="accent">{FORMATS[info.format] ?? info.format}</Badge>
        <span className="spacer" />
        <Button variant="primary" icon="play" onClick={() => setSending(true)}>
          Send endpoints…
        </Button>
        <Button icon="spider" onClick={() => setScanning(true)}>
          Audit endpoints…
        </Button>
        <IconButton
          icon="trash"
          label="Delete this definition"
          onClick={async () => {
            const ok = await dialogs.confirm(
              `Delete ${info.name}? Requests already sent stay in the history.`,
              "Delete definition?",
              "Delete",
            );
            if (!ok) return;
            try {
              await api.apiDefinitionDelete(id);
              onDeleted();
            } catch (e) {
              setError(toIpcError(e).message);
            }
          }}
        />
      </div>
      {info.warnings.length > 0 && (
        <details className="callout">
          <summary>{info.warnings.length} warning(s) while reading the file</summary>
          <ul>
            {info.warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </details>
      )}
      {info.format === "graphql" && <GraphqlPanel id={id} />}
      <input
        aria-label="Filter endpoints"
        placeholder="Filter by method, path, name or group"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <div className="api-table" role="table" aria-label="Endpoints">
        {shown.map((i) => {
          const e = detail.endpoints[i];
          if (!e) return null;
          return (
            <div key={i} className="api-row" role="row">
              <span role="cell" className={isReadOnly(e) ? "api-method" : "api-method unsafe"}>
                {e.method}
              </span>
              <span role="cell" className="mono api-path" title={e.url}>
                {e.path || e.url}
              </span>
              <span role="cell" className="muted api-name">
                {e.name}
                {e.group && ` · ${e.group}`}
              </span>
              <span role="cell" className="api-actions">
                <Button size="sm" variant="ghost" onClick={() => void open(i, "repeater")}>
                  Repeater
                </Button>
                <Button size="sm" variant="ghost" onClick={() => void open(i, "fuzzer")}>
                  Fuzzer
                </Button>
              </span>
            </div>
          );
        })}
      </div>
      {sending && (
        <SendDialog
          detail={detail}
          indexes={shown.length === detail.endpoints.length ? [] : shown}
          onClose={() => setSending(false)}
        />
      )}
      {scanning && (
        <ScanDialog
          initialUrls={Array.from(
            new Set(
              detail.endpoints
                .map((e) => {
                  try {
                    return `${new URL(e.url).origin}/`;
                  } catch {
                    return "";
                  }
                })
                .filter(Boolean),
            ),
          )}
          initialConfigName="Audit stored traffic"
          onClose={() => setScanning(false)}
        />
      )}
    </div>
  );
}

function SendDialog({
  detail,
  indexes,
  onClose,
}: {
  detail: ApiDefinitionDetail;
  indexes: number[];
  onClose(): void;
}) {
  const [hosts, setHosts] = useState<string[]>([]);
  const [unsafe, setUnsafe] = useState(false);
  const [rate, setRate] = useState(5);
  const [error, setError] = useState<string | null>(null);
  const [pool, setPool] = useState("");
  const count = indexes.length === 0 ? detail.endpoints.length : indexes.length;

  async function start() {
    try {
      const task = await api.apiSendStart(
        {
          definition: detail.info.id,
          endpoints: indexes,
          hosts,
          includeUnsafe: unsafe,
          requestsPerSecond: rate,
        },
        pool || null,
      );
      useAppStore.getState().notify("info", `${task.name} started`);
      await useTasksStore.getState().refresh();
      useTasksStore.getState().select(task.id);
      useAppStore.getState().setView("tasks");
      onClose();
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-form"
        role="dialog"
        aria-modal="true"
        aria-label="Send endpoints"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Send {count} endpoint(s)</strong>
        </header>
        <p className="muted">
          Sends each endpoint's example request once, through the crawler's resource pool and
          session rules, and stores it in HTTP history (source “scanner”) for passive analysis and
          scans. With a scope defined, out-of-scope URLs are skipped. Tick only hosts you are
          allowed to test.
        </p>
        <fieldset className="api-hosts">
          <legend>Hosts</legend>
          {detail.hosts.map((h) => (
            <label key={h} className="inline-field">
              <input
                type="checkbox"
                checked={hosts.includes(h)}
                onChange={(e) =>
                  setHosts(e.target.checked ? [...hosts, h] : hosts.filter((x) => x !== h))
                }
              />
              <span className="mono">{h}</span>
            </label>
          ))}
        </fieldset>
        <Switch
          checked={unsafe}
          onChange={setUnsafe}
          label="Also send POST, PUT, PATCH and DELETE (they may change or delete data)"
        />
        <label className="inline-field">
          Requests per second
          <input
            type="number"
            min={0}
            max={100}
            value={rate}
            onChange={(e) => setRate(Number(e.target.value))}
          />
        </label>
        {error && <p className="callout callout-danger">{error}</p>}
        <div className="form-actions">
          <span className="spacer" />
          <LaunchPoolSelect value={pool} onChange={setPool} />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={hosts.length === 0} onClick={() => void start()}>
            Send
          </Button>
        </div>
      </div>
    </div>
  );
}
