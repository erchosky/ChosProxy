import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { LoadingState } from "../../components/States";
import { formatDateTime } from "../../lib/format";
import { useAppStore } from "../../state/app";

/** Status, explanation and actions for the local certificate authority. */
export function CaCard() {
  const ca = useAppStore((s) => s.ca);
  const busy = useAppStore((s) => s.busy);
  const exportCa = useAppStore((s) => s.exportCa);
  const regenerateCa = useAppStore((s) => s.regenerateCa);
  const importCa = useAppStore((s) => s.importCa);

  return (
    <section className="card">
      <h2>Local certificate authority</h2>
      {!ca ? (
        <LoadingState />
      ) : (
        <>
          <div className="ca-status">
            {ca.available ? (
              <Badge tone="ok">ready — HTTPS can be inspected</Badge>
            ) : (
              <Badge tone="danger">unavailable — HTTPS is tunnelled without inspection</Badge>
            )}
            {ca.generatedThisSession && ca.available && (
              <Badge tone="accent">generated on this machine during this session</Badge>
            )}
          </div>
          {ca.error && <p className="callout callout-danger selectable">{ca.error}</p>}
          <p className="muted">
            To read HTTPS traffic, ChosProxy presents its own certificate for each site, signed by
            this certificate authority (CA). It was created on this machine; its private key never
            leaves it and is never exported. A browser accepts these certificates only if it trusts
            this CA — do that only in a browser profile used for testing, never in your everyday
            browser.
          </p>
          {ca.info && (
            <dl className="paths selectable">
              <dt>Name</dt>
              <dd>{ca.info.subject}</dd>
              <dt>SHA-256 fingerprint</dt>
              <dd className="mono fingerprint">{ca.info.fingerprintSha256}</dd>
              <dt>Valid</dt>
              <dd>
                {formatDateTime(ca.info.notBefore)} → {formatDateTime(ca.info.notAfter)}
              </dd>
              <dt>Certificate</dt>
              <dd className="mono">{ca.info.certPath}</dd>
              <dt>Private key</dt>
              <dd className="mono">{ca.info.keyPath} (keep private)</dd>
              <dt>Download through the proxy</dt>
              <dd className="mono">{ca.downloadUrl}</dd>
            </dl>
          )}
          <div className="form-actions">
            <Button
              icon="file"
              disabled={!ca.available || busy !== null}
              onClick={() => void exportCa("pem")}
            >
              Export certificate (PEM)…
            </Button>
            <Button
              icon="file"
              disabled={!ca.available || busy !== null}
              onClick={() => void exportCa("der")}
            >
              Export certificate (DER .crt)…
            </Button>
            <span className="spacer" />
            <Button icon="folder" disabled={busy !== null} onClick={() => void importCa()}>
              Import CA…
            </Button>
            <Button
              variant="danger"
              icon="restart"
              disabled={busy !== null}
              onClick={() => void regenerateCa()}
            >
              Regenerate CA…
            </Button>
          </div>
        </>
      )}
    </section>
  );
}
