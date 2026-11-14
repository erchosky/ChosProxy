import { useState } from "react";
import { LaunchPoolSelect } from "../../components/LaunchPoolSelect";
import { ConfigLibraryBar } from "../../components/ConfigLibraryBar";
import type { DiscoveryConfig } from "../../bindings/DiscoveryConfig";
import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { useTasksStore } from "../../state/tasks";
import { ScheduleControls } from "./ScheduleControls";

type NumberField = {
  [K in keyof DiscoveryConfig]: DiscoveryConfig[K] extends number ? K : never;
}[keyof DiscoveryConfig];

const NUMBERS: { key: NumberField; label: string; min: number; max: number; help: string }[] = [
  {
    key: "maxDepth",
    label: "Depth",
    min: 0,
    max: 5,
    help: "Folder levels searched below the base",
  },
  { key: "maxRequests", label: "Requests", min: 1, max: 100000, help: "At most, in total" },
  { key: "concurrency", label: "At a time", min: 1, max: 32, help: "Requests in flight" },
  {
    key: "requestsPerSecond",
    label: "Per second",
    min: 0,
    max: 1000,
    help: "0 = no limit; be gentle with shared targets",
  },
  { key: "timeoutSecs", label: "Timeout (s)", min: 1, max: 300, help: "Per response" },
];

/** "Discover content": base folder, names, limits. */
export function DiscoveryDialog() {
  const discovery = useTasksStore((s) => s.discovery);
  if (!discovery) return null;
  return <Dialog config={discovery} />;
}

function Dialog({ config }: { config: DiscoveryConfig }) {
  const { editDiscovery, closeDiscovery, startDiscovery } = useTasksStore.getState();
  const [words, setWords] = useState(config.customWords.join("\n"));
  const [extensions, setExtensions] = useState(config.extensions.join(", "));
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [pool, setPool] = useState("");
  const invalid = NUMBERS.find(({ key, min, max }) => {
    const v = config[key];
    return !Number.isInteger(v) || v < min || v > max;
  });

  async function start() {
    editDiscovery({
      customWords: words
        .split(/\s+/)
        .map((w) => w.trim())
        .filter(Boolean),
      extensions: extensions
        .split(/[\s,]+/)
        .map((e) => e.trim().replace(/^\./, ""))
        .filter(Boolean),
    });
    setStarting(true);
    setError(await startDiscovery(pool));
    setStarting(false);
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={closeDiscovery}>
      <div
        className="dialog crawl-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Discover content"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && closeDiscovery()}
      >
        <header className="dialog-head">
          <strong>Discover content</strong>
        </header>
        <div className="dialog-form">
          <ConfigLibraryBar
            tool="discovery"
            current={config}
            onLoad={(patch) => {
              editDiscovery(patch);
              if (Array.isArray(patch.customWords)) setWords(patch.customWords.join("\n"));
              if (Array.isArray(patch.extensions)) setExtensions(patch.extensions.join(", "));
            }}
          />
          <label>
            Base folder
            <input
              autoFocus
              className="mono"
              spellCheck={false}
              placeholder="https://target.example/"
              value={config.baseUrl}
              onChange={(e) => editDiscovery({ baseUrl: e.target.value })}
            />
          </label>
          <div className="dialog-row">
            <Switch
              label="Built-in names (common folders and files)"
              checked={config.builtinWords}
              onChange={(builtinWords) => editDiscovery({ builtinWords })}
            />
            <Switch
              label="Names from the site map"
              checked={config.siteWords}
              onChange={(siteWords) => editDiscovery({ siteWords })}
            />
            <Switch
              label="Well-known files (robots.txt, .git/HEAD…)"
              checked={config.wellKnownFiles}
              onChange={(wellKnownFiles) => editDiscovery({ wellKnownFiles })}
            />
          </div>
          <label className="crawl-urls">
            Your own names (one per line)
            <textarea
              rows={3}
              spellCheck={false}
              className="mono"
              value={words}
              onChange={(e) => setWords(e.target.value)}
            />
          </label>
          <label>
            Extensions tried after each name
            <input
              className="mono"
              spellCheck={false}
              value={extensions}
              onChange={(e) => setExtensions(e.target.value)}
            />
          </label>
          <div className="crawl-limits">
            {NUMBERS.map(({ key, label, min, max, help }) => (
              <label key={key} title={help}>
                <span>{label}</span>
                <input
                  type="number"
                  min={min}
                  max={max}
                  value={config[key]}
                  aria-invalid={invalid?.key === key}
                  onChange={(e) => editDiscovery({ [key]: Number(e.target.value) })}
                />
                <small className="muted">{help}</small>
              </label>
            ))}
          </div>
          <div className="dialog-row">
            <Switch
              label="Search folders that are found"
              checked={config.recursive}
              onChange={(recursive) => editDiscovery({ recursive })}
            />
            <Switch
              label="Use the cookie jar"
              checked={config.useCookieJar}
              onChange={(useCookieJar) => editDiscovery({ useCookieJar })}
            />
          </div>
          <p className="muted crawl-note">
            Sends GET requests for likely names under the base folder, after learning what the site
            returns for names that cannot exist (so &quot;not found&quot; pages that answer 200 are
            not reported). It stays in scope — or, while no scope is defined, on the base URL&apos;s
            site. Only the hits are stored, as &quot;discovery&quot; traffic. Run it only against
            targets you are authorized to test, with limits their owners agree to.
          </p>
          {invalid && (
            <div className="field-error">
              {invalid.label} must be {invalid.min}–{invalid.max}
            </div>
          )}
          {error && <div className="field-error">{error}</div>}
        </div>
        <footer className="dialog-row crawl-actions">
          <ScheduleControls
            kind="discovery"
            name={`Discover content on ${config.baseUrl}`}
            config={() => ({
              ...config,
              customWords: words
                .split(/\s+/)
                .map((w) => w.trim())
                .filter(Boolean),
              extensions: extensions
                .split(/[\s,]+/)
                .map((e) => e.trim().replace(/^\./, ""))
                .filter(Boolean),
            })}
            onScheduled={closeDiscovery}
          />
          <span className="spacer" />
          <LaunchPoolSelect value={pool} onChange={setPool} />
          <Button variant="ghost" onClick={closeDiscovery}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon="search"
            disabled={starting || invalid !== undefined || config.baseUrl.trim() === ""}
            onClick={() => void start()}
          >
            Start discovery
          </Button>
        </footer>
      </div>
    </div>
  );
}
