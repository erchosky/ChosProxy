import { useEffect } from "react";
import { ErrorState, LoadingState } from "../components/States";
import { CommandPalette } from "../features/palette/CommandPalette";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { WelcomeScreen } from "../features/welcome/WelcomeScreen";
import { PasswordDialog } from "../components/PasswordDialog";
import { RecoveryDialog } from "../components/RecoveryDialog";
import { runningInDesktop } from "../services/ipc";
import { useAppStore } from "../state/app";
import { useScopeStore } from "../state/scope";
import { Shell } from "./Shell";
import { useTheme } from "./theme";
import { useEngineEvents } from "./useEngineEvents";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

export function App() {
  if (!runningInDesktop()) {
    return (
      <ErrorState
        title="Desktop runtime not found"
        message="This interface must run inside the ChosProxy desktop application (pnpm dev)."
      />
    );
  }
  return <DesktopApp />;
}

function DesktopApp() {
  const bootstrapped = useAppStore((s) => s.bootstrapped);
  const bootError = useAppStore((s) => s.bootError);
  const project = useAppStore((s) => s.project);
  const theme = useAppStore((s) => s.settings?.ui.theme);
  const bootstrap = useAppStore((s) => s.bootstrap);

  useTheme(theme);
  useEngineEvents();
  useGlobalShortcuts();
  useEffect(() => {
    void bootstrap();
    void useScopeStore.getState().load();
  }, [bootstrap]);

  if (!bootstrapped) return <LoadingState label="Starting…" />;
  if (bootError)
    return (
      <ErrorState title="Could not start" message={bootError} onRetry={() => void bootstrap()} />
    );
  return (
    <>
      {project ? <Shell /> : <WelcomeScreen />}
      <CommandPalette />
      <RecoveryDialog />
      <PasswordDialog />
      <ShortcutsDialog />
    </>
  );
}
