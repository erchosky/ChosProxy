import { useState } from "react";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { useAppStore } from "../../state/app";

/** Detects browsers and launches/closes the isolated lab browser. */
export function BrowserCard() {
  const browsers = useAppStore((s) => s.browsers);
  const browser = useAppStore((s) => s.browser);
  const listeners = useAppStore((s) => s.listeners);
  const busy = useAppStore((s) => s.busy);
  const launch = useAppStore((s) => s.launchBrowser);
  const close = useAppStore((s) => s.closeBrowser);
  const refresh = useAppStore((s) => s.refreshBrowsers);
  const [selected, setSelected] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const listening = listeners.some((l) => l.state.state === "running");
  const chosen = selected ?? browsers[0]?.id ?? null;
  const urlInvalid = url.trim() !== "" && !/^https?:\/\//i.test(url.trim());

  return (
    <section className="card">
      <h2>Lab browser</h2>
      <p className="muted">
        Opens an installed browser with a temporary, isolated profile that sends all traffic through
        the proxy. Your normal browser profile and the system certificate store are not touched; the
        profile is deleted when the browser closes.
      </p>
      {browser?.running ? (
        <div className="browser-running">
          <Badge tone="ok">running</Badge>
          <span>
            {browser.browser?.name} via <code>{browser.proxy}</code>
          </span>
          <span className="spacer" />
          <Button icon="stop" disabled={busy !== null} onClick={() => void close()}>
            Close browser
          </Button>
        </div>
      ) : browsers.length === 0 ? (
        <p className="callout callout-warn">
          No supported browser found. On Kali: <code>sudo apt install chromium</code> (or Firefox),
          then{" "}
          <button type="button" className="link-button" onClick={() => void refresh()}>
            search again
          </button>
          .
        </p>
      ) : (
        <>
          <div className="browser-choices" role="radiogroup" aria-label="Browser">
            {browsers.map((b) => (
              <label key={b.id} className="browser-choice">
                <input
                  type="radio"
                  name="lab-browser"
                  checked={chosen === b.id}
                  onChange={() => setSelected(b.id)}
                />
                <span>{b.name}</span>
                <span className="muted mono">{b.id}</span>
                {b.kind === "firefox" && (
                  <Badge title="The CA is imported into the temporary profile when NSS certutil is installed">
                    CA import via certutil
                  </Badge>
                )}
              </label>
            ))}
          </div>
          <div className="form-actions">
            <input
              type="text"
              className="grow"
              placeholder="Start page (optional), e.g. http://localhost:3000"
              aria-label="Start page"
              aria-invalid={urlInvalid}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
            />
            <Button
              variant="primary"
              icon="play"
              disabled={!listening || urlInvalid || busy !== null}
              title={listening ? undefined : "Start a proxy listener first"}
              onClick={() => void launch(chosen, url.trim() || null)}
            >
              Open lab browser
            </Button>
          </div>
          {urlInvalid && <div className="field-error">Start with http:// or https://</div>}
          {!listening && <p className="field-error">Start a proxy listener first.</p>}
        </>
      )}
      {browser?.warning && <p className="callout callout-warn">{browser.warning}</p>}
    </section>
  );
}
