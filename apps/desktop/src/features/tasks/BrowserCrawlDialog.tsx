import { useState } from "react";
import { Button } from "../../components/Button";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useTasksStore } from "../../state/tasks";

/** Starts a browser-powered crawl (M24): the lab browser renders each page
 * through the proxy, so links made by scripts are found too. */
export function BrowserCrawlDialog({ onClose }: { onClose(): void }) {
  const [url, setUrl] = useState("");
  const [pages, setPages] = useState(50);
  const [timeout, setTimeoutSecs] = useState(20);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function start() {
    setBusy(true);
    setError(null);
    try {
      const task = await api.browserCrawlStart({
        startUrl: url.trim(),
        maxPages: pages,
        pageTimeoutSecs: timeout,
        browser: null,
      });
      useAppStore.getState().notify("info", `${task.name} started`);
      await useTasksStore.getState().refresh();
      useTasksStore.getState().select(task.id);
      onClose();
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Browser crawl"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Browser crawl</strong>
        </header>
        <p className="muted">
          Opens each page in the lab browser (Chromium, Chrome, Edge or Brave) through the proxy, so
          scripts run and the links they add are followed. Stays in scope, or on the start origin
          when no scope is defined. Log in in the lab browser first, or add a session rule, to crawl
          behind a login. Every request appears in HTTP history.
        </p>
        <label className="field-stack">
          Start URL
          <input
            type="url"
            placeholder="https://target.example/"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
        </label>
        <div className="dialog-row">
          <label className="inline-field">
            Pages at most
            <input
              type="number"
              min={1}
              max={500}
              value={pages}
              onChange={(e) => setPages(Number(e.target.value))}
            />
          </label>
          <label className="inline-field">
            Wait per page (s)
            <input
              type="number"
              min={1}
              max={120}
              value={timeout}
              onChange={(e) => setTimeoutSecs(Number(e.target.value))}
            />
          </label>
        </div>
        {error && <p className="callout callout-danger">{error}</p>}
        <div className="form-actions">
          <span className="spacer" />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon="spider"
            disabled={busy || !/^https?:\/\//.test(url.trim())}
            onClick={() => void start()}
          >
            Start
          </Button>
        </div>
      </div>
    </div>
  );
}
