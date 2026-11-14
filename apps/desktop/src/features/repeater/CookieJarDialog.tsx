import { useCallback, useEffect, useMemo, useState } from "react";
import type { JarCookie } from "../../bindings/JarCookie";
import { Button, IconButton } from "../../components/Button";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { formatDateTime } from "../../lib/format";
import { api, dialogs, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

/** The project cookie jar: cookies set by proxy and Repeater responses,
 * which Repeater tabs can add to their requests. */
export function CookieJarDialog({ onClose }: { onClose(): void }) {
  const notify = useAppStore((s) => s.notify);
  const [cookies, setCookies] = useState<JarCookie[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  const load = useCallback(() => {
    api
      .cookieJar()
      .then((c) => {
        setCookies(c);
        setError(null);
      })
      .catch((e: unknown) => setError(toIpcError(e).message));
  }, []);
  useEffect(load, [load]);

  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!cookies || !q) return cookies ?? [];
    return cookies.filter(
      (c) =>
        c.domain.includes(q) ||
        c.name.toLowerCase().includes(q) ||
        c.value.toLowerCase().includes(q),
    );
  }, [cookies, filter]);

  async function remove(ids: number[]) {
    try {
      await api.cookieJarDelete(ids);
      load();
    } catch (e) {
      notify("error", `Could not delete: ${toIpcError(e).message}`);
    }
  }

  async function clear() {
    const ok = await dialogs.confirm(
      "Remove every cookie from the project cookie jar?",
      "Clear the cookie jar?",
      "Clear",
    );
    if (!ok) return;
    try {
      const n = await api.cookieJarClear();
      notify("info", `${n} cookie${n === 1 ? "" : "s"} removed`);
      load();
    } catch (e) {
      notify("error", `Could not clear the jar: ${toIpcError(e).message}`);
    }
  }

  async function setValue(cookie: JarCookie, value: string) {
    if (value === cookie.value) return;
    try {
      await api.cookieJarSetValue(cookie.id, value);
      load();
    } catch (e) {
      notify("error", `Could not change ${cookie.name}: ${toIpcError(e).message}`);
    }
  }

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog cookie-jar"
        role="dialog"
        aria-modal="true"
        aria-label="Cookie jar"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Cookie jar</strong>
          <input
            type="search"
            aria-label="Filter cookies"
            placeholder="Filter by domain, name or value"
            value={filter}
            autoFocus
            onChange={(e) => setFilter(e.target.value)}
          />
          <span className="spacer" />
          <Button
            size="sm"
            variant="ghost"
            icon="trash"
            disabled={!cookies?.length}
            onClick={() => void clear()}
          >
            Clear
          </Button>
          <IconButton icon="close" label="Close" onClick={onClose} />
        </header>
        <p className="muted cookie-jar-help">
          Filled from the Set-Cookie headers of proxy and Repeater responses. Tabs with “Use cookie
          jar” add the matching cookies (domain, path, Secure) to their Cookie header when sending;
          the editor text is not changed.
        </p>
        <div className="cookie-jar-body">
          {error ? (
            <ErrorState title="Could not load the cookie jar" message={error} onRetry={load} />
          ) : cookies === null ? (
            <LoadingState />
          ) : shown.length === 0 ? (
            <EmptyState title={cookies.length ? "No matching cookies" : "The jar is empty"}>
              Browse the target through the proxy, or send a request that sets cookies from
              Repeater.
            </EmptyState>
          ) : (
            <table className="endpoint-table cookie-jar-table">
              <thead>
                <tr>
                  <th>Domain</th>
                  <th>Path</th>
                  <th>Name</th>
                  <th>Value</th>
                  <th>Expires</th>
                  <th>Flags</th>
                  <th>From</th>
                  <th aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {shown.map((c) => (
                  <tr key={c.id}>
                    <td className="mono" title={c.hostOnly ? "This host only" : "And subdomains"}>
                      {c.hostOnly ? c.domain : `.${c.domain}`}
                    </td>
                    <td className="mono">{c.path}</td>
                    <td className="mono">{c.name}</td>
                    <td>
                      <input
                        key={`${c.id}-${c.updatedAt}`}
                        className="mono cookie-value"
                        aria-label={`Value of ${c.name}`}
                        defaultValue={c.value}
                        spellCheck={false}
                        onKeyDown={(e) => {
                          e.stopPropagation();
                          if (e.key === "Enter") e.currentTarget.blur();
                          if (e.key === "Escape") {
                            e.currentTarget.value = c.value;
                            e.currentTarget.blur();
                          }
                        }}
                        onBlur={(e) => void setValue(c, e.currentTarget.value)}
                      />
                    </td>
                    <td className="muted">
                      {c.expiresAt === null ? "session" : formatDateTime(c.expiresAt)}
                    </td>
                    <td className="muted">
                      {[c.secure && "Secure", c.httpOnly && "HttpOnly", c.sameSite]
                        .filter(Boolean)
                        .join(" · ")}
                    </td>
                    <td className="muted">{c.source}</td>
                    <td>
                      <IconButton
                        icon="trash"
                        label={`Delete ${c.name}`}
                        onClick={() => void remove([c.id])}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <footer className="muted">
          {cookies ? `${cookies.length} cookie${cookies.length === 1 ? "" : "s"}` : ""}
        </footer>
      </div>
    </div>
  );
}
