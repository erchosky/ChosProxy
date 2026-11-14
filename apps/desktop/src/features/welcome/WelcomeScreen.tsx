import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { useAppStore } from "../../state/app";

export function WelcomeScreen() {
  const info = useAppStore((s) => s.info);
  const busy = useAppStore((s) => s.busy);
  const newTemporaryProject = useAppStore((s) => s.newTemporaryProject);
  const createProject = useAppStore((s) => s.createProject);
  const openProject = useAppStore((s) => s.openProject);
  const openProjectReadOnly = useAppStore((s) => s.openProjectReadOnly);
  const listener = useAppStore((s) => s.proxyConfig?.listeners[0]);

  return (
    <div className="welcome">
      <div className="welcome-card">
        <div className="welcome-logo" aria-hidden="true">
          ‹•›
        </div>
        <h1>{info?.productName}</h1>
        <p className="welcome-tagline">{info?.tagline}</p>

        <div className="callout callout-warn welcome-auth">
          <Icon name="shield" />
          <span>
            Only test systems you own or are explicitly authorized to test — local labs such as
            OWASP Juice Shop, DVWA or WebGoat, CTFs, or targets covered by written permission.
          </span>
        </div>

        <div className="welcome-actions">
          <Button
            variant="primary"
            size="lg"
            icon="play"
            disabled={busy !== null}
            onClick={() => void newTemporaryProject()}
          >
            Start temporary project
          </Button>
          <div className="welcome-secondary">
            <Button icon="plus" disabled={busy !== null} onClick={() => void createProject()}>
              New project file…
            </Button>
            <Button icon="folder" disabled={busy !== null} onClick={() => void openProject()}>
              Open project file…
            </Button>
            <Button
              icon="folder"
              disabled={busy !== null}
              title="Inspect a project file without modifying it or starting listeners"
              onClick={() => void openProjectReadOnly()}
            >
              Open read-only…
            </Button>
            <Button
              icon="shield"
              disabled={busy !== null}
              title="Encrypted with a random key that exists only while it is open"
              onClick={() => void newTemporaryProject(true)}
            >
              Encrypted temporary project
            </Button>
          </div>
          <p className="welcome-hint">
            Temporary projects are deleted when closed. Project files keep history across sessions.
          </p>
        </div>

        <ol className="welcome-steps">
          <li>
            <strong>Create a project.</strong> The proxy starts on{" "}
            <code>{listener ? `${listener.bind_address}:${listener.port}` : "127.0.0.1:8080"}</code>
            .
          </li>
          <li>
            <strong>Open the lab browser</strong> — an isolated profile already routed through the
            proxy and able to inspect HTTPS.
          </li>
          <li>
            <strong>Browse your target.</strong> HTTP and HTTPS requests appear in Proxy → HTTP
            history.
          </li>
        </ol>
        <p className="welcome-hint">
          A local certificate authority is generated on this machine for HTTPS inspection; its
          private key never leaves it.
        </p>
      </div>
    </div>
  );
}
