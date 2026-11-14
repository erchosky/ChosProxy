import { ConfigLibraryBar } from "../../components/ConfigLibraryBar";
import { LaunchPoolSelect } from "../../components/LaunchPoolSelect";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { FuzzAttackMode } from "../../bindings/FuzzAttackMode";
import type { FuzzConfig } from "../../bindings/FuzzConfig";
import type { FuzzPayloadSet } from "../../bindings/FuzzPayloadSet";
import type { FuzzPayloadSource } from "../../bindings/FuzzPayloadSource";
import type { FuzzProcessing } from "../../bindings/FuzzProcessing";
import type { FuzzResultRow } from "../../bindings/FuzzResultRow";
import type { FuzzSort } from "../../bindings/FuzzSort";
import type { FuzzStopConditions } from "../../bindings/FuzzStopConditions";
import type { Operation } from "../../bindings/Operation";
import type { PointKind } from "../../bindings/PointKind";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { MessageViewer } from "../../components/message/MessageViewer";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { Segmented } from "../../components/Tabs";
import { formatBytes, formatDuration, statusTone } from "../../lib/format";
import { parseTarget } from "../../lib/repeaterTarget";
import {
  useFuzzerStore,
  addMark,
  countPositions,
  emptySet,
  MARKER,
  OPTIONAL_COLUMNS,
  type ResultColumn,
  parseStatusList,
  setsNeeded,
} from "../../state/fuzzer";
import { type MenuEntry, openContextMenu, openMenuBelow, SEPARATOR } from "../../state/contextMenu";
import { HIGHLIGHTS } from "../../lib/filters";
import { useScriptsStore } from "../../state/scripts";
import { useTasksStore } from "../../state/tasks";

const MODES: { id: FuzzAttackMode; label: string; help: string }[] = [
  {
    id: "sniper",
    label: "Sniper",
    help: "One payload set. Each position in turn gets every payload; the others keep their original value. Use it to test inputs one by one.",
  },
  {
    id: "batteringRam",
    label: "Battering ram",
    help: "One payload set. The same payload goes into every position at once.",
  },
  {
    id: "pitchfork",
    label: "Pitchfork",
    help: "One payload set per position. Request n uses the n-th payload of each set (stops at the shortest set).",
  },
  {
    id: "clusterBomb",
    label: "Cluster bomb",
    help: "One payload set per position. Every combination is sent — the count grows quickly.",
  },
];

const ENCODINGS: { id: Operation; label: string }[] = [
  { id: "urlEncode", label: "URL-encode" },
  { id: "urlEncodeAll", label: "URL-encode all" },
  { id: "base64Encode", label: "Base64" },
  { id: "base64UrlEncode", label: "Base64 URL" },
  { id: "hexEncode", label: "Hex" },
  { id: "htmlEncode", label: "HTML entities" },
  { id: "md5", label: "MD5" },
  { id: "sha1", label: "SHA-1" },
  { id: "sha256", label: "SHA-256" },
];

/** Fuzzer: mark positions in a request, choose payloads, send, compare. */
export function FuzzerView() {
  const tab = useFuzzerStore((s) => s.tab);
  const config = useFuzzerStore((s) => s.config);
  const { init, setTab } = useFuzzerStore.getState();
  useEffect(() => {
    void init();
  }, [init]);
  if (!config) return <LoadingState />;
  return (
    <div className="fuzzer">
      <div className="fuzzer-tabs">
        <Segmented<"setup" | "results">
          label="Fuzzer"
          value={tab}
          onChange={setTab}
          items={[
            { id: "setup", label: "Attack setup" },
            { id: "results", label: "Results" },
          ]}
        />
        <span className="muted">
          Repeat a request with payloads in marked positions. Use it only on targets you are
          authorized to test.
        </span>
      </div>
      {tab === "setup" ? <Setup config={config} /> : <Results />}
    </div>
  );
}

function Setup({ config }: { config: FuzzConfig }) {
  return (
    <div className="fuzzer-body">
      <SplitPane
        direction="horizontal"
        initial={0.5}
        first={<RequestPanel config={config} />}
        second={<PayloadPanel config={config} />}
      />
    </div>
  );
}

const NO_STOP: FuzzStopConditions = { afterMatches: 0, afterErrors: 0, onStatus: [] };

const ALL_POINT_KINDS: PointKind[] = [
  "pathSegment",
  "query",
  "form",
  "cookie",
  "header",
  "json",
  "multipart",
  "xml",
];

function RequestPanel({ config }: { config: FuzzConfig }) {
  const targetText = useFuzzerStore((s) => s.targetText);
  const { edit, setTargetText, autoMark } = useFuzzerStore.getState();
  const area = useRef<HTMLTextAreaElement>(null);
  const target = parseTarget(targetText);
  const positions = countPositions(config.template);
  const unbalanced = (config.template.split(MARKER).length - 1) % 2 === 1;

  function mark() {
    const el = area.current;
    if (!el) return;
    const { selectionStart, selectionEnd } = el;
    edit({ template: addMark(config.template, selectionStart, selectionEnd) });
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(selectionEnd + 2, selectionEnd + 2);
    });
  }

  return (
    <section className="fuzzer-panel" aria-label="Request">
      <div className="fuzzer-bar">
        <label className="repeater-target">
          <span>Target</span>
          <input
            className="mono"
            value={targetText}
            spellCheck={false}
            aria-invalid={"error" in target}
            placeholder="https://example.com"
            onChange={(e) => setTargetText(e.target.value)}
          />
        </label>
        {"error" in target && <span className="field-error">{target.error}</span>}
      </div>
      <div className="fuzzer-bar">
        <Button size="sm" title="Mark the selected text as a payload position" onClick={mark}>
          Add §
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title="Remove every marker"
          onClick={() => edit({ template: config.template.replaceAll(MARKER, "") })}
        >
          Clear §
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title="Mark parameter, cookie and body values (query, form, JSON, multipart, XML)"
          onClick={() => void autoMark()}
        >
          Auto §
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label="More ways to mark positions"
          title="Choose which insertion points to mark"
          onClick={(e) =>
            openMenuBelow(e.currentTarget, "Mark insertion points", [
              { label: "Parameters, cookies and body", onSelect: () => void autoMark() },
              { label: "URL path segments", onSelect: () => void autoMark(["pathSegment"]) },
              {
                label: "Headers (User-Agent, Referer, Origin…)",
                onSelect: () => void autoMark(["header"]),
              },
              { label: "Everything", onSelect: () => void autoMark(ALL_POINT_KINDS) },
            ])
          }
        >
          ▾
        </Button>
        <span className="spacer" />
        {unbalanced ? (
          <Badge tone="danger">unbalanced §</Badge>
        ) : (
          <Badge tone={positions > 0 ? "accent" : "neutral"}>
            {positions} position{positions === 1 ? "" : "s"}
          </Badge>
        )}
      </div>
      <textarea
        ref={area}
        className="mono fuzzer-template"
        aria-label="Request with payload positions"
        spellCheck={false}
        placeholder={
          "Send a request here with “Send to Fuzzer” (right-click a request), or paste one,\nthen select a value and press Add §."
        }
        value={config.template}
        onChange={(e) => edit({ template: e.target.value })}
      />
      <div className="fuzzer-bar">
        <Switch
          label="Update Content-Length"
          checked={config.updateContentLength}
          onChange={(updateContentLength) => edit({ updateContentLength })}
        />
        <Switch
          label="Use the cookie jar"
          checked={config.useCookieJar}
          onChange={(useCookieJar) => edit({ useCookieJar })}
        />
      </div>
    </section>
  );
}

function NumberInput({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange(v: number): void;
}) {
  return (
    <label className="fuzzer-number">
      <span>{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
  );
}

function PayloadPanel({ config }: { config: FuzzConfig }) {
  const preview = useFuzzerStore((s) => s.preview);
  const starting = useFuzzerStore((s) => s.starting);
  const { edit, setSet, start } = useFuzzerStore.getState();
  const [error, setError] = useState<string | null>(null);
  const [pool, setPool] = useState("");
  const positions = countPositions(config.template);
  const needed = setsNeeded(config.mode, positions);
  const mode = MODES.find((m) => m.id === config.mode);
  // Saved configurations from before stop conditions have none.
  const stop = config.stop ?? NO_STOP;

  return (
    <section className="fuzzer-panel fuzzer-scroll" aria-label="Payloads">
      <ConfigLibraryBar tool="fuzzer" current={config} onLoad={(patch) => edit(patch)} />
      <h3>Attack mode</h3>
      <select
        aria-label="Attack mode"
        value={config.mode}
        onChange={(e) => edit({ mode: e.target.value as FuzzAttackMode })}
      >
        {MODES.map((m) => (
          <option key={m.id} value={m.id}>
            {m.label}
          </option>
        ))}
      </select>
      <p className="muted fuzzer-help">{mode?.help}</p>

      {Array.from({ length: needed }, (_, i) => (
        <PayloadSetEditor
          key={i}
          index={i}
          label={needed > 1 ? `Payload set ${i + 1} (position ${i + 1})` : "Payloads"}
          value={config.payloadSets[i] ?? emptySet()}
          onChange={(v) => setSet(i, v)}
        />
      ))}

      <h3>Grep</h3>
      <label className="fuzzer-field">
        Flag responses containing (one per line)
        <textarea
          rows={2}
          className="mono"
          value={config.grepMatch.join("\n")}
          onChange={(e) => edit({ grepMatch: e.target.value.split("\n") })}
        />
      </label>
      <div className="dialog-row">
        <Switch
          label="Regular expressions"
          checked={config.grepRegex}
          onChange={(grepRegex) => edit({ grepRegex })}
        />
        <Switch
          label="Match case"
          checked={config.grepCaseSensitive}
          onChange={(grepCaseSensitive) => edit({ grepCaseSensitive })}
        />
      </div>
      <label className="fuzzer-field">
        Extract with regular expressions (first group; one per line)
        <textarea
          rows={2}
          className="mono"
          placeholder="<title>(.*?)</title>"
          value={config.grepExtract.join("\n")}
          onChange={(e) => edit({ grepExtract: e.target.value.split("\n") })}
        />
      </label>

      <h3>Options</h3>
      <div className="fuzzer-numbers">
        <NumberInput
          label="At a time"
          value={config.concurrency}
          min={1}
          max={32}
          onChange={(concurrency) => edit({ concurrency })}
        />
        <NumberInput
          label="Per second (0 = no limit)"
          value={config.requestsPerSecond}
          min={0}
          max={500}
          onChange={(requestsPerSecond) => edit({ requestsPerSecond })}
        />
        <NumberInput
          label="Timeout (s)"
          value={config.timeoutSecs}
          min={1}
          max={300}
          onChange={(timeoutSecs) => edit({ timeoutSecs })}
        />
        <NumberInput
          label="Retries"
          value={config.retries}
          min={0}
          max={3}
          onChange={(retries) => edit({ retries })}
        />
        <NumberInput
          label="Maximum requests"
          value={config.maxRequests}
          min={1}
          max={200000}
          onChange={(maxRequests) => edit({ maxRequests })}
        />
      </div>

      <h3>Stop conditions</h3>
      <p className="muted">Off at 0. The baseline request does not count.</p>
      <div className="fuzzer-numbers">
        <NumberInput
          label="Stop after matches"
          value={stop.afterMatches}
          min={0}
          max={200000}
          onChange={(afterMatches) => edit({ stop: { ...stop, afterMatches } })}
        />
        <NumberInput
          label="Stop after requests without response"
          value={stop.afterErrors}
          min={0}
          max={200000}
          onChange={(afterErrors) => edit({ stop: { ...stop, afterErrors } })}
        />
        <label className="fuzzer-number">
          <span>Stop on status codes</span>
          <input
            type="text"
            placeholder="e.g. 429, 503"
            defaultValue={stop.onStatus.join(", ")}
            onBlur={(e) =>
              edit({ stop: { ...stop, onStatus: parseStatusList(e.currentTarget.value) } })
            }
          />
        </label>
      </div>

      <div className="fuzzer-start">
        {preview?.error ? (
          <span className="field-error">{preview.error}</span>
        ) : preview ? (
          <span>
            <strong>{(preview.requests + 1).toLocaleString()}</strong> requests (baseline +{" "}
            {preview.requests.toLocaleString()})
            {preview.samples.length > 0 && (
              <span className="muted mono">
                {" "}
                · {preview.samples.map((s) => s.join(" | ")).join(", ")}
                {preview.requests > preview.samples.length ? ", …" : ""}
              </span>
            )}
          </span>
        ) : null}
        {error && <span className="field-error">{error}</span>}
        <span className="spacer" />
        <LaunchPoolSelect value={pool} onChange={setPool} />
        <Button
          variant="primary"
          icon="play"
          disabled={starting || !preview || preview.error !== null}
          onClick={() => void start(pool).then(setError)}
        >
          Start attack
        </Button>
      </div>
    </section>
  );
}

type SourceKind = FuzzPayloadSource["kind"];

function defaultSource(kind: SourceKind): FuzzPayloadSource {
  switch (kind) {
    case "list":
      return { kind, items: [] };
    case "numbers":
      return { kind, from: 1, to: 100, step: 1, minDigits: 0 };
    case "characters":
      return { kind, charset: "abcdefghijklmnopqrstuvwxyz0123456789", minLength: 1, maxLength: 2 };
    case "dates":
      return { kind, from: "2024-01-01", to: "2024-12-31", stepDays: 1, format: "yyyy-MM-dd" };
    case "builtin":
      return { kind, name: "special-characters" };
  }
}

function PayloadSetEditor({
  index,
  label,
  value,
  onChange,
}: {
  index: number;
  label: string;
  value: FuzzPayloadSet;
  onChange(v: FuzzPayloadSet): void;
}) {
  const lists = useFuzzerStore((s) => s.lists);
  const source = value.source;
  const setSource = (s: FuzzPayloadSource) => onChange({ ...value, source: s });

  async function loadFile(file: File) {
    const text = await file.text();
    const items = text.split(/\r?\n/).filter((l) => l !== "");
    setSource({ kind: "list", items });
  }

  return (
    <div className="fuzzer-set" aria-label={label}>
      <h3>{label}</h3>
      <select
        aria-label={`Source of payload set ${index + 1}`}
        value={source.kind}
        onChange={(e) => setSource(defaultSource(e.target.value as SourceKind))}
      >
        <option value="list">Your list</option>
        <option value="numbers">Numbers</option>
        <option value="characters">Character combinations</option>
        <option value="dates">Dates</option>
        <option value="builtin">Built-in list</option>
      </select>
      {source.kind === "list" && (
        <>
          <textarea
            rows={5}
            className="mono"
            aria-label="Payloads, one per line"
            placeholder="One payload per line"
            value={source.items.join("\n")}
            onChange={(e) => setSource({ kind: "list", items: e.target.value.split("\n") })}
          />
          <div className="dialog-row">
            <label className="btn btn-ghost btn-sm">
              Load file…
              <input
                type="file"
                accept=".txt,.lst,text/plain"
                hidden
                onChange={(e) => {
                  const f = e.target.files?.[0];
                  if (f) void loadFile(f);
                }}
              />
            </label>
            <span className="muted">
              {source.items.filter((i) => i !== "").length.toLocaleString()} payloads
            </span>
          </div>
        </>
      )}
      {source.kind === "numbers" && (
        <div className="fuzzer-numbers">
          <NumberInput
            label="From"
            value={source.from}
            min={-1e15}
            max={1e15}
            onChange={(from) => setSource({ ...source, from })}
          />
          <NumberInput
            label="To"
            value={source.to}
            min={-1e15}
            max={1e15}
            onChange={(to) => setSource({ ...source, to })}
          />
          <NumberInput
            label="Step"
            value={source.step}
            min={-1e15}
            max={1e15}
            onChange={(step) => setSource({ ...source, step })}
          />
          <NumberInput
            label="Min digits"
            value={source.minDigits}
            min={0}
            max={30}
            onChange={(minDigits) => setSource({ ...source, minDigits })}
          />
        </div>
      )}
      {source.kind === "characters" && (
        <div className="fuzzer-numbers">
          <label className="fuzzer-number fuzzer-wide">
            <span>Characters</span>
            <input
              className="mono"
              value={source.charset}
              onChange={(e) => setSource({ ...source, charset: e.target.value })}
            />
          </label>
          <NumberInput
            label="Min length"
            value={source.minLength}
            min={1}
            max={8}
            onChange={(minLength) => setSource({ ...source, minLength })}
          />
          <NumberInput
            label="Max length"
            value={source.maxLength}
            min={1}
            max={8}
            onChange={(maxLength) => setSource({ ...source, maxLength })}
          />
        </div>
      )}
      {source.kind === "dates" && (
        <div className="fuzzer-numbers">
          <label className="fuzzer-number">
            <span>From</span>
            <input
              type="date"
              value={source.from}
              onChange={(e) => setSource({ ...source, from: e.target.value })}
            />
          </label>
          <label className="fuzzer-number">
            <span>To</span>
            <input
              type="date"
              value={source.to}
              onChange={(e) => setSource({ ...source, to: e.target.value })}
            />
          </label>
          <NumberInput
            label="Every (days)"
            value={source.stepDays}
            min={1}
            max={3650}
            onChange={(stepDays) => setSource({ ...source, stepDays })}
          />
          <label
            className="fuzzer-number fuzzer-wide"
            title="yyyy, yy, MM, M, MMM, dd, d; anything else is copied"
          >
            <span>Format</span>
            <input
              className="mono"
              value={source.format}
              onChange={(e) => setSource({ ...source, format: e.target.value })}
            />
          </label>
        </div>
      )}
      {source.kind === "builtin" && (
        <>
          <select
            aria-label="Built-in list"
            value={source.name}
            onChange={(e) => setSource({ kind: "builtin", name: e.target.value })}
          >
            {lists.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name} ({l.items.length})
              </option>
            ))}
          </select>
          <p className="muted fuzzer-help">
            {lists.find((l) => l.id === source.name)?.description} No credential lists are included.
          </p>
        </>
      )}
      <ProcessingEditor
        value={value.processing}
        onChange={(processing) => onChange({ ...value, processing })}
      />
    </div>
  );
}

function ProcessingEditor({
  value,
  onChange,
}: {
  value: FuzzProcessing[];
  onChange(v: FuzzProcessing[]): void;
}) {
  const update = (i: number, rule: FuzzProcessing) =>
    onChange(value.map((r, j) => (j === i ? rule : r)));
  const library = useScriptsStore((s) => s.library);
  const payloadScripts = library?.config.scripts.filter((s) => s.kind === "payload") ?? [];
  function add(kind: string) {
    const rule: FuzzProcessing | null =
      kind === "prefix" || kind === "suffix"
        ? { kind, text: "" }
        : kind === "replace"
          ? { kind, from: "", to: "" }
          : kind === "lowercase" || kind === "uppercase"
            ? { kind }
            : kind === "encode"
              ? { kind, operation: "urlEncode" }
              : kind === "script"
                ? { kind, script: payloadScripts[0]?.id ?? "" }
                : null;
    if (rule) onChange([...value, rule]);
  }
  return (
    <div className="fuzzer-processing">
      {value.map((rule, i) => (
        <div key={i} className="fuzzer-rule">
          <span className="muted">{i + 1}.</span>
          <strong>{rule.kind}</strong>
          {(rule.kind === "prefix" || rule.kind === "suffix") && (
            <input
              className="mono"
              aria-label={`${rule.kind} text`}
              value={rule.text}
              onChange={(e) => update(i, { ...rule, text: e.target.value })}
            />
          )}
          {rule.kind === "replace" && (
            <>
              <input
                className="mono"
                aria-label="Find"
                placeholder="find"
                value={rule.from}
                onChange={(e) => update(i, { ...rule, from: e.target.value })}
              />
              <input
                className="mono"
                aria-label="Replace with"
                placeholder="replace with"
                value={rule.to}
                onChange={(e) => update(i, { ...rule, to: e.target.value })}
              />
            </>
          )}
          {rule.kind === "encode" && (
            <select
              aria-label="Encoding"
              value={rule.operation}
              onChange={(e) =>
                update(i, { kind: "encode", operation: e.target.value as Operation })
              }
            >
              {ENCODINGS.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                </option>
              ))}
            </select>
          )}
          {rule.kind === "script" && (
            <select
              aria-label="Payload script"
              value={rule.script}
              onChange={(e) => update(i, { kind: "script", script: e.target.value })}
            >
              <option value="">Choose a payload script…</option>
              {payloadScripts.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          )}
          <IconButton
            icon="close"
            label="Remove this rule"
            onClick={() => onChange(value.filter((_, j) => j !== i))}
          />
        </div>
      ))}
      <select
        aria-label="Add a processing rule"
        value=""
        onChange={(e) => {
          add(e.target.value);
          e.target.value = "";
        }}
      >
        <option value="">+ Processing (applied to each payload, in order)…</option>
        <option value="prefix">Add prefix</option>
        <option value="suffix">Add suffix</option>
        <option value="replace">Replace text</option>
        <option value="lowercase">Lower case</option>
        <option value="uppercase">Upper case</option>
        <option value="encode">Encode or hash</option>
        <option value="script" disabled={payloadScripts.length === 0}>
          Payload script{payloadScripts.length === 0 ? " (create one in Tools → Scripts)" : ""}
        </option>
      </select>
    </div>
  );
}

const SORTS: { id: FuzzSort; label: string }[] = [
  { id: "index", label: "#" },
  { id: "status", label: "Status" },
  { id: "length", label: "Length" },
  { id: "duration", label: "Time" },
];

function Results() {
  const attacks = useFuzzerStore((s) => s.attacks);
  const attackId = useFuzzerStore((s) => s.attackId);
  const results = useFuzzerStore((s) => s.results);
  const error = useFuzzerStore((s) => s.resultsError);
  const query = useFuzzerStore((s) => s.query);
  const selected = useFuzzerStore((s) => s.selected);
  const detail = useFuzzerStore((s) => s.detail);
  const hiddenColumns = useFuzzerStore((s) => s.hiddenColumns);
  const hide = useMemo(() => new Set<ResultColumn>(hiddenColumns), [hiddenColumns]);
  const { selectAttack, setQuery, selectResult, loadResults, loadAttacks, toggleColumn } =
    useFuzzerStore.getState();
  const tasks = useTasksStore.getState();
  useEffect(() => {
    void loadAttacks().then(loadResults);
  }, [loadAttacks, loadResults]);
  const attack = attacks.find((a) => a.id === attackId);
  if (attacks.length === 0) {
    return (
      <EmptyState title="No attacks yet">
        Set up an attack in “Attack setup” and press Start attack. Its results appear here.
      </EmptyState>
    );
  }
  const config = attack ? (JSON.parse(attack.config) as FuzzConfig) : null;
  const shows = (c: ResultColumn) => !hide.has(c);
  const matchTerms = shows("matches") ? (config?.grepMatch ?? []).filter((m) => m !== "") : [];
  const extracts = shows("extracts") ? (config?.grepExtract ?? []).filter((m) => m !== "") : [];

  function header(sort: FuzzSort, label: string) {
    const active = query.sort === sort;
    return (
      <th
        aria-sort={active ? (query.descending ? "descending" : "ascending") : undefined}
        className="sortable"
        onClick={() =>
          setQuery({ sort, descending: active ? !query.descending : sort !== "index" })
        }
      >
        {label}
        {active ? (query.descending ? " ↓" : " ↑") : ""}
      </th>
    );
  }

  return (
    <div className="fuzzer-body">
      <div className="fuzzer-bar">
        <select
          aria-label="Attack"
          value={attackId ?? ""}
          onChange={(e) => selectAttack(Number(e.target.value))}
        >
          {attacks.map((a) => (
            <option key={a.id} value={a.id}>
              #{a.id} {a.name} — {a.status}
            </option>
          ))}
        </select>
        {attack && (
          <>
            <Badge tone={attack.status === "running" ? "accent" : "neutral"}>{attack.status}</Badge>
            <span className="muted">
              {attack.progress} · {attack.errors} without response · {attack.discovered} matched
            </span>
            {(attack.status === "running" || attack.status === "queued") && (
              <>
                <IconButton
                  icon="pause"
                  label="Pause"
                  onClick={() => void tasks.pause(attack.id).then(loadAttacks)}
                />
                <IconButton
                  icon="stop"
                  label="Stop"
                  onClick={() => void tasks.cancel(attack.id).then(loadAttacks)}
                />
              </>
            )}
            {attack.status === "paused" && (
              <>
                <IconButton
                  icon="play"
                  label="Resume"
                  onClick={() => void tasks.resume(attack.id).then(loadAttacks)}
                />
                <IconButton
                  icon="stop"
                  label="Stop"
                  onClick={() => void tasks.cancel(attack.id).then(loadAttacks)}
                />
              </>
            )}
          </>
        )}
        <span className="spacer" />
        <Switch
          label="Matched only"
          checked={query.matchedOnly}
          onChange={(matchedOnly) => setQuery({ matchedOnly })}
        />
        <Switch
          label="Status differs from baseline"
          checked={query.differentStatusOnly}
          onChange={(differentStatusOnly) => setQuery({ differentStatusOnly })}
        />
        <span className="muted">{results ? `${results.total.toLocaleString()} rows` : ""}</span>
        <Button
          icon="menu"
          aria-haspopup="menu"
          onClick={(e) =>
            openMenuBelow(e.currentTarget, "Columns", [
              { heading: "Columns" },
              ...OPTIONAL_COLUMNS.map((c) => ({
                label: c.label,
                checked: !hide.has(c.id),
                onSelect: () => toggleColumn(c.id),
              })),
            ])
          }
        >
          Columns
        </Button>
        <Button
          icon="file"
          disabled={!attack || (results?.total ?? 0) === 0}
          title="Save every result of this attack as CSV"
          onClick={() => void useFuzzerStore.getState().exportResults()}
        >
          Export CSV
        </Button>
      </div>
      <SplitPane
        direction="vertical"
        initial={0.55}
        first={
          error ? (
            <ErrorState
              title="Could not load results"
              message={error}
              onRetry={() => void loadResults()}
            />
          ) : !results ? (
            <LoadingState />
          ) : (
            <div className="endpoint-table-wrap fuzzer-results">
              <table className="endpoint-table organizer-table">
                <thead>
                  <tr>
                    {header("index", "#")}
                    {shows("payload") && <th>Payload</th>}
                    {SORTS.slice(1)
                      .filter((s) => shows(s.id as ResultColumn))
                      .map((s) => header(s.id, s.label))}
                    {shows("error") && <th>Error</th>}
                    {matchTerms.map((m) => (
                      <th key={m} title={`Contains ${m}`}>
                        {m.length > 12 ? `${m.slice(0, 12)}…` : m}
                      </th>
                    ))}
                    {extracts.map((x) => (
                      <th key={x} title={x}>
                        extract
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {results.rows.map((r) => (
                    <ResultRow
                      key={r.id}
                      row={r}
                      selected={selected?.id === r.id}
                      onSelect={selectResult}
                      hide={hide}
                    />
                  ))}
                </tbody>
              </table>
              {results.total > results.rows.length && (
                <p className="muted">
                  Showing the first {results.rows.length.toLocaleString()} rows; sort or filter to
                  see others.
                </p>
              )}
            </div>
          )
        }
        second={
          !selected ? (
            <EmptyState title="Select a request">Its request and response appear here.</EmptyState>
          ) : (
            <SplitPane
              direction="horizontal"
              first={
                <MessageViewer
                  title={`Request ${selected.idx}`}
                  message={detail?.request ?? null}
                  emptyText="Loading…"
                  compareLabel={`Fuzz request ${selected.idx}`}
                />
              }
              second={
                <MessageViewer
                  title="Response"
                  message={detail?.response ?? null}
                  emptyText={selected.error ? `No response: ${selected.error}` : "Loading…"}
                  compareLabel={`Fuzz response ${selected.idx}`}
                />
              }
            />
          )
        }
      />
    </div>
  );
}

/**
 * One result. Recorded results never change, so a row re-renders only when
 * its selection does: the table reloads about once a second during an
 * attack and may hold 2,000 rows.
 */
const ResultRow = memo(
  function ResultRow({
    row,
    selected,
    onSelect,
    hide,
  }: {
    row: FuzzResultRow;
    selected: boolean;
    onSelect(row: FuzzResultRow): void;
    hide: ReadonlySet<ResultColumn>;
  }) {
    return (
      <tr
        className={`table-row${selected ? " selected" : ""}${row.color ? ` hl-${row.color}` : ""}`}
        aria-selected={selected}
        onClick={() => onSelect(row)}
        onContextMenu={(e) => {
          onSelect(row);
          openContextMenu(e, "Result actions", resultMenu(row));
        }}
      >
        <td className="num">{row.idx === 0 ? "base" : row.idx}</td>
        {!hide.has("payload") && (
          <td className="mono" title={row.payloads.join(" | ")}>
            {row.position !== null && <span className="muted">§{row.position + 1} </span>}
            {row.payloads.join(" | ")}
          </td>
        )}
        {!hide.has("status") && (
          <td>
            <span className={`status status-${statusTone(row.status)}`}>{row.status ?? "—"}</span>
          </td>
        )}
        {!hide.has("length") && <td className="num">{formatBytes(row.length)}</td>}
        {!hide.has("duration") && <td className="num">{formatDuration(row.durationMs)}</td>}
        {!hide.has("error") && (
          <td className="muted" title={row.error ?? undefined}>
            {row.error ?? ""}
          </td>
        )}
        {!hide.has("matches") &&
          row.matches.map((m, i) => (
            <td key={`m${i}`}>{m ? <Badge tone="accent">✓</Badge> : ""}</td>
          ))}
        {!hide.has("extracts") &&
          row.extracts.map((x, i) => (
            <td key={`x${i}`} className="mono" title={x}>
              {x}
            </td>
          ))}
      </tr>
    );
  },
  (a, b) =>
    a.row.id === b.row.id &&
    a.row.color === b.row.color &&
    a.hide === b.hide &&
    a.selected === b.selected &&
    a.onSelect === b.onSelect,
);

/** Context menu of a result: highlight colours. */
function resultMenu(row: FuzzResultRow): MenuEntry[] {
  const { setColor } = useFuzzerStore.getState();
  const entries: MenuEntry[] = [{ heading: `Request ${row.idx === 0 ? "base" : row.idx}` }];
  entries.push({ heading: "Highlight" });
  for (const color of HIGHLIGHTS) {
    entries.push({
      label: color[0]?.toUpperCase() + color.slice(1),
      checked: row.color === color,
      onSelect: () => void setColor(row.id, row.color === color ? null : color),
    });
  }
  if (row.color !== null) {
    entries.push(SEPARATOR, { label: "No highlight", onSelect: () => void setColor(row.id, null) });
  }
  return entries;
}
