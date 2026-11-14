import { useState } from "react";
import { Icon } from "../components/Icon";
import { formatClock } from "../lib/format";
import { useAppStore } from "../state/app";

export function StatusBar() {
  const stats = useAppStore((s) => s.stats);
  const notices = useAppStore((s) => s.notices);
  const dismiss = useAppStore((s) => s.dismissNotice);
  const busy = useAppStore((s) => s.busy);
  const setView = useAppStore((s) => s.setView);
  const [open, setOpen] = useState(false);
  const problems = notices.filter((n) => n.level !== "info");
  const latest = notices[0];

  return (
    <footer className="statusbar">
      <span>{stats ? `${stats.project.exchanges.toLocaleString()} requests` : "—"}</span>
      <span>{stats ? `${stats.proxy.activeConnections} open connections` : ""}</span>
      {stats && stats.proxy.tunnels > 0 && (
        <span title="Connections relayed without decryption (passthrough hosts, interception off, or non-HTTP protocols)">
          {stats.proxy.tunnels} tunnelled without decryption
        </span>
      )}
      {stats && stats.proxy.tlsClientFailures > 0 && (
        <span
          className="status-warn"
          title="A client rejected the interception certificate: it does not trust the local CA"
        >
          {stats.proxy.tlsClientFailures} HTTPS handshakes rejected
        </span>
      )}
      {stats && stats.proxy.authFailures > 0 && (
        <span className="status-warn" title="Requests rejected for wrong proxy credentials">
          {stats.proxy.authFailures} proxy logins rejected
        </span>
      )}
      {stats && stats.capturesDropped > 0 && (
        <span className="status-warn">{stats.capturesDropped} captures dropped</span>
      )}
      {busy && <span>{busy}…</span>}
      <span className="spacer" />
      {latest && (
        <span className={`status-latest status-${latest.level}`} title={latest.message}>
          {latest.message}
        </span>
      )}
      <button
        type="button"
        className={`status-notices${problems.length ? " has-problems" : ""}`}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <Icon name="warning" size={13} />
        {problems.length}
      </button>
      {open && (
        <div className="notice-panel" role="dialog" aria-label="Notifications">
          <header>
            <strong>Notifications</strong>
            <span className="spacer" />
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setOpen(false);
                setView("eventlog");
              }}
            >
              Event log
            </button>
            <button
              type="button"
              className="icon-btn"
              aria-label="Close"
              onClick={() => setOpen(false)}
            >
              <Icon name="close" size={14} />
            </button>
          </header>
          {notices.length === 0 ? (
            <p className="notice-empty">No notifications.</p>
          ) : (
            <ul>
              {notices.map((n) => (
                <li key={n.id} className={`notice notice-${n.level}`}>
                  <span className="notice-time mono">{formatClock(n.at)}</span>
                  <span className="notice-text selectable">{n.message}</span>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="Dismiss"
                    onClick={() => dismiss(n.id)}
                  >
                    <Icon name="close" size={12} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </footer>
  );
}
