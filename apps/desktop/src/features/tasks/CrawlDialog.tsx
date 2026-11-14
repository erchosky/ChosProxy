import { useState } from "react";
import { LaunchPoolSelect } from "../../components/LaunchPoolSelect";
import { ConfigLibraryBar } from "../../components/ConfigLibraryBar";
import type { CrawlConfig } from "../../bindings/CrawlConfig";
import { Button } from "../../components/Button";
import { Switch } from "../../components/Switch";
import { useTasksStore } from "../../state/tasks";
import { ScheduleControls } from "./ScheduleControls";

type NumberField = {
  [K in keyof CrawlConfig]: CrawlConfig[K] extends number ? K : never;
}[keyof CrawlConfig];

const NUMBERS: { key: NumberField; label: string; min: number; max: number; help: string }[] = [
  { key: "maxDepth", label: "Depth", min: 0, max: 50, help: "Links followed from a start URL" },
  { key: "maxRequests", label: "Requests", min: 1, max: 100000, help: "At most, in total" },
  { key: "concurrency", label: "At a time", min: 1, max: 32, help: "Requests in flight" },
  {
    key: "requestsPerSecond",
    label: "Per second",
    min: 0,
    max: 200,
    help: "0 = no limit; be gentle with shared targets",
  },
  { key: "timeoutSecs", label: "Timeout (s)", min: 1, max: 300, help: "Per response" },
  {
    key: "maxVariantsPerPath",
    label: "Variants per path",
    min: 1,
    max: 1000,
    help: "Query strings fetched per path (/item?id=…)",
  },
  {
    key: "maxSimilar",
    label: "Similar URLs",
    min: 0,
    max: 10000,
    help: "Per pattern: /item/1 and /item/2 are alike (0 = no limit)",
  },
];

/** "New crawl": start URLs, limits and safety options. */
export function CrawlDialog() {
  const crawl = useTasksStore((s) => s.crawl);
  if (!crawl) return null;
  return <Dialog crawl={crawl} />;
}

function Dialog({ crawl }: { crawl: CrawlConfig }) {
  const { editCrawl, closeCrawl, startCrawl } = useTasksStore.getState();
  const [urls, setUrls] = useState(crawl.startUrls.join("\n"));
  const [avoid, setAvoid] = useState(crawl.avoid.join(", "));
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [pool, setPool] = useState("");

  async function start() {
    editCrawl({
      startUrls: urls
        .split(/\s+/)
        .map((u) => u.trim())
        .filter(Boolean),
      avoid: avoid
        .split(",")
        .map((w) => w.trim())
        .filter(Boolean),
    });
    setStarting(true);
    setError(await startCrawl(pool));
    setStarting(false);
  }

  const invalid = NUMBERS.find(({ key, min, max }) => {
    const v = crawl[key];
    return !Number.isInteger(v) || v < min || v > max;
  });

  return (
    <div className="palette-backdrop" role="presentation" onClick={closeCrawl}>
      <div
        className="dialog crawl-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="New crawl"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && closeCrawl()}
      >
        <header className="dialog-head">
          <strong>New crawl</strong>
        </header>
        <div className="dialog-form">
          <ConfigLibraryBar
            tool="crawler"
            current={crawl}
            onLoad={(patch) => {
              editCrawl(patch);
              if (Array.isArray(patch.avoid)) setAvoid(patch.avoid.join(", "));
            }}
          />
          <label className="crawl-urls">
            Start URLs (one per line)
            <textarea
              rows={3}
              autoFocus
              spellCheck={false}
              className="mono"
              placeholder="https://target.example/"
              value={urls}
              onChange={(e) => setUrls(e.target.value)}
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
                  value={crawl[key]}
                  aria-invalid={invalid?.key === key}
                  onChange={(e) => editCrawl({ [key]: Number(e.target.value) })}
                />
                <small className="muted">{help}</small>
              </label>
            ))}
          </div>
          <div className="dialog-row">
            <Switch
              label="Submit GET forms (default values)"
              checked={crawl.submitGetForms}
              onChange={(submitGetForms) => editCrawl({ submitGetForms })}
            />
            <Switch
              label="Use the cookie jar (crawl as the logged-in user)"
              checked={crawl.useCookieJar}
              onChange={(useCookieJar) => editCrawl({ useCookieJar })}
            />
          </div>
          <label>
            Do not follow links containing
            <input
              className="mono"
              spellCheck={false}
              value={avoid}
              onChange={(e) => setAvoid(e.target.value)}
            />
          </label>
          <p className="muted crawl-note">
            The crawler only sends GET requests and never submits POST forms (they are listed in the
            task log). It stays in scope — or, while no scope is defined, on the start URLs’ sites —
            and skips links that look like they log out or delete things. Its traffic appears in the
            Logger as “crawler” and goes through passive analysis. Crawl only targets you are
            authorized to test.
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
            kind="crawl"
            name={`Crawl ${urls.split(/\s+/).find(Boolean) ?? ""}`}
            config={() => ({
              ...crawl,
              startUrls: urls
                .split(/\s+/)
                .map((u) => u.trim())
                .filter(Boolean),
              avoid: avoid
                .split(",")
                .map((w) => w.trim())
                .filter(Boolean),
            })}
            onScheduled={closeCrawl}
          />
          <span className="spacer" />
          <LaunchPoolSelect value={pool} onChange={setPool} />
          <Button variant="ghost" onClick={closeCrawl}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon="play"
            disabled={starting || invalid !== undefined || urls.trim() === ""}
            onClick={() => void start()}
          >
            Start crawl
          </Button>
        </footer>
      </div>
    </div>
  );
}
