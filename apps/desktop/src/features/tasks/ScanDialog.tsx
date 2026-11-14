import { useEffect, useState } from "react";
import { LaunchPoolSelect } from "../../components/LaunchPoolSelect";
import type { CrawlConfig } from "../../bindings/CrawlConfig";
import type { PointKind } from "../../bindings/PointKind";
import type { ScanConfig } from "../../bindings/ScanConfig";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useTasksStore } from "../../state/tasks";
import { CheckPicker } from "./CheckPicker";
import { ScheduleControls } from "./ScheduleControls";

type CrawlNumber = "maxDepth" | "maxRequests" | "requestsPerSecond" | "maxSimilar";

const CRAWL_NUMBERS: { key: CrawlNumber; label: string; min: number; max: number }[] = [
  { key: "maxDepth", label: "Depth", min: 0, max: 50 },
  { key: "maxRequests", label: "Requests", min: 1, max: 100000 },
  { key: "requestsPerSecond", label: "Per second", min: 0, max: 200 },
  { key: "maxSimilar", label: "Similar URLs", min: 0, max: 10000 },
];

const POINT_KINDS: { kind: PointKind; label: string }[] = [
  { kind: "query", label: "Query" },
  { kind: "form", label: "Form" },
  { kind: "json", label: "JSON" },
  { kind: "cookie", label: "Cookie" },
  { kind: "header", label: "Headers" },
  { kind: "pathSegment", label: "Path" },
  { kind: "multipart", label: "Multipart" },
  { kind: "xml", label: "XML" },
];

/** "New scan" (M25 / M13): a configuration from the library, start URLs, and the
 * phases crawl → passive audit → JavaScript analysis → optional active audit. */
export function ScanDialog({
  initialUrls,
  initialConfigName,
  onClose,
}: {
  initialUrls?: string[];
  initialConfigName?: string;
  onClose(): void;
}) {
  const [configs, setConfigs] = useState<ScanConfig[]>([]);
  const [config, setConfig] = useState<ScanConfig | null>(null);
  const [urls, setUrls] = useState(() => (initialUrls?.length ? initialUrls.join("\n") : ""));
  const [saveName, setSaveName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pool, setPool] = useState("");

  function use(list: ScanConfig[], name?: string) {
    setConfigs(list);
    const chosen = list.find((c) => c.name === name) ?? list[0] ?? null;
    setConfig(chosen ? structuredClone(chosen) : null);
  }

  useEffect(() => {
    api
      .scanConfigs()
      .then((list) => use(list, initialConfigName))
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, [initialConfigName]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  if (!config) {
    return null;
  }
  const edit = (patch: Partial<ScanConfig>) => setConfig({ ...config, ...patch });
  const editCrawl = (patch: Partial<CrawlConfig>) =>
    edit({ crawlConfig: { ...config.crawlConfig, ...patch } });
  const stored = configs.find((c) => c.name === config.name);
  const changed = JSON.stringify(stored) !== JSON.stringify(config);

  const start = () =>
    run(async () => {
      const task = await api.scanStart(
        {
          startUrls: urls
            .split(/\s+/)
            .map((u) => u.trim())
            .filter(Boolean),
          config,
        },
        pool || null,
      );
      useAppStore.getState().notify("info", `${task.name} started`);
      await useTasksStore.getState().refresh();
      useTasksStore.getState().select(task.id);
      onClose();
    });

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-form dialog-wide"
        role="dialog"
        aria-modal="true"
        aria-label="New scan"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>New scan</strong>
        </header>
        <p className="muted">
          A scan crawls the start URLs (GET requests from links only, in scope, rate-limited) and
          then audits each structural pattern once with the passive checks and the JavaScript
          analysis. It sends no attack payloads. The task log says why each item was audited or
          skipped.
        </p>
        <label className="field-stack">
          Start URLs (one per line)
          <textarea
            className="mono"
            rows={3}
            placeholder="https://target.example/"
            value={urls}
            onChange={(e) => setUrls(e.target.value)}
          />
        </label>
        <div className="dialog-row">
          <label className="inline-field">
            Configuration
            <select
              value={stored ? config.name : ""}
              onChange={(e) => use(configs, e.target.value)}
            >
              {!stored && <option value="">(edited copy)</option>}
              {configs.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name}
                  {c.builtin ? " (built-in)" : ""}
                </option>
              ))}
            </select>
          </label>
          {changed && <Badge tone="accent">changed</Badge>}
        </div>
        {config.description && <p className="muted">{config.description}</p>}
        <div className="dialog-row">
          <Switch checked={config.crawl} onChange={(crawl) => edit({ crawl })} label="Crawl" />
          <Switch
            checked={config.passive}
            onChange={(passive) => edit({ passive })}
            label="Passive audit"
          />
          <Switch
            checked={config.javascript}
            onChange={(javascript) => edit({ javascript })}
            label="JavaScript analysis"
          />
          <Switch
            checked={config.active}
            onChange={(active) => edit({ active, activeAuthorized: false })}
            label="Active audit"
          />
        </div>
        <div className="dialog-row">
          {config.crawl &&
            CRAWL_NUMBERS.map((f) => (
              <label key={f.key} className="inline-field">
                {f.label}
                <input
                  type="number"
                  min={f.min}
                  max={f.max}
                  value={config.crawlConfig[f.key]}
                  onChange={(e) => editCrawl({ [f.key]: Number(e.target.value) })}
                />
              </label>
            ))}
          <label className="inline-field">
            Unique items audited
            <input
              type="number"
              min={1}
              max={100000}
              value={config.maxItems}
              onChange={(e) => edit({ maxItems: Number(e.target.value) })}
            />
          </label>
        </div>
        {config.active && (
          <fieldset className="field-stack">
            <legend>Active audit limits &amp; authorization</legend>
            <div className="dialog-row">
              <label className="inline-field">
                Max active requests
                <input
                  type="number"
                  min={1}
                  max={50000}
                  value={config.maxActiveRequests}
                  onChange={(e) => edit({ maxActiveRequests: Number(e.target.value) })}
                />
              </label>
              <label className="inline-field">
                Per insertion point
                <input
                  type="number"
                  min={1}
                  max={50}
                  value={config.maxRequestsPerPoint}
                  onChange={(e) => edit({ maxRequestsPerPoint: Number(e.target.value) })}
                />
              </label>
              <label className="inline-field">
                Concurrency
                <input
                  type="number"
                  min={1}
                  max={16}
                  value={config.activeConcurrency}
                  onChange={(e) => edit({ activeConcurrency: Number(e.target.value) })}
                />
              </label>
              <label className="inline-field">
                Active req/s
                <input
                  type="number"
                  min={1}
                  max={100}
                  value={config.activeRequestsPerSecond}
                  onChange={(e) => edit({ activeRequestsPerSecond: Number(e.target.value) })}
                />
              </label>
              <label className="inline-field">
                Timeout (s)
                <input
                  type="number"
                  min={1}
                  max={120}
                  value={config.activeTimeoutSecs}
                  onChange={(e) => edit({ activeTimeoutSecs: Number(e.target.value) })}
                />
              </label>
            </div>
            <div className="dialog-row">
              <span className="muted">Insertion points:</span>
              {POINT_KINDS.map((p) => {
                const checked = config.activePointKinds.includes(p.kind);
                return (
                  <label key={p.kind} className="inline-field">
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={(e) => {
                        const next = e.target.checked
                          ? [...config.activePointKinds, p.kind]
                          : config.activePointKinds.filter((k) => k !== p.kind);
                        edit({ activePointKinds: next });
                      }}
                    />
                    {p.label}
                  </label>
                );
              })}
            </div>
            <div className="dialog-row">
              <Switch
                checked={config.activeAllowUnsafeMethods}
                onChange={(activeAllowUnsafeMethods) => edit({ activeAllowUnsafeMethods })}
                label="Also probe non-idempotent methods (POST, PUT, PATCH, DELETE)"
              />
              <span className="muted">
                Estimated cap: up to{" "}
                {Math.min(
                  config.maxActiveRequests,
                  config.maxItems * config.maxRequestsPerPoint,
                ).toLocaleString()}{" "}
                active probe requests
              </span>
            </div>
            <label className="inline-field callout">
              <input
                type="checkbox"
                checked={config.activeAuthorized}
                onChange={(e) => edit({ activeAuthorized: e.target.checked })}
              />
              <span>
                I confirm I am explicitly authorized to send active verification probes to the
                in-scope target(s).
              </span>
            </label>
          </fieldset>
        )}
        {(config.passive || config.javascript || config.active) && (
          <CheckPicker
            disabled={config.disabledChecks}
            includeActive={config.active}
            onChange={(disabledChecks) => edit({ disabledChecks })}
          />
        )}
        <div className="dialog-row">
          <input
            aria-label="Name for the saved configuration"
            placeholder="Save as…"
            value={saveName}
            onChange={(e) => setSaveName(e.target.value)}
          />
          <Button
            size="sm"
            disabled={busy || !saveName.trim()}
            onClick={() =>
              void run(async () => {
                const name = saveName.trim();
                use(await api.saveScanConfig({ ...config, name, builtin: false }), name);
                setSaveName("");
              })
            }
          >
            Save
          </Button>
          {stored && !stored.builtin && (
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              onClick={() => void run(async () => use(await api.deleteScanConfig(config.name)))}
            >
              Delete
            </Button>
          )}
          <span className="spacer" />
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const path = await dialogs.chooseJsonFile();
                if (path) use(await api.importScanConfigs(path), config.name);
              })
            }
          >
            Import…
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || !stored}
            title="Export the selected configuration as JSON"
            onClick={() =>
              void run(async () => {
                const path = await dialogs.chooseJsonDestination("scan-configuration.json");
                if (path) await api.exportScanConfigs(path, [config.name]);
              })
            }
          >
            Export…
          </Button>
        </div>
        {error && <p className="callout callout-danger">{error}</p>}
        <div className="form-actions">
          {urls.trim() && (
            <ScheduleControls
              kind="scan"
              name={`Scan ${urls.split(/\s+/).find(Boolean) ?? ""} (${config.name})`}
              config={() => ({
                startUrls: urls
                  .split(/\s+/)
                  .map((u) => u.trim())
                  .filter(Boolean),
                config,
              })}
              onScheduled={onClose}
            />
          )}
          <span className="spacer" />
          <LaunchPoolSelect value={pool} onChange={setPool} />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon="spider"
            disabled={busy || !urls.trim() || (config.active && !config.activeAuthorized)}
            onClick={() => void start()}
          >
            Start scan
          </Button>
        </div>
      </div>
    </div>
  );
}
