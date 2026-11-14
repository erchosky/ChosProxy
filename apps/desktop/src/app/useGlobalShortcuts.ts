import { useEffect } from "react";
import { useAppStore } from "../state/app";
import { useInterceptStore } from "../state/intercept";
import { isEditing, matchShortcut, stepView } from "./shortcuts";

/** Application-wide keyboard shortcuts (listed in the `?` overlay). */
export function useGlobalShortcuts() {
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.defaultPrevented || e.isComposing) return;
      // Escape closes the shortcuts overlay wherever focus is.
      if (e.key === "Escape" && useAppStore.getState().shortcutsOpen) {
        e.preventDefault();
        useAppStore.getState().setShortcutsOpen(false);
        return;
      }
      const bindings = useAppStore.getState().settings?.ui.keyBindings ?? [];
      const action = matchShortcut(e, isEditing(e.target), bindings);
      if (!action) return;
      const s = useAppStore.getState();
      // While a dialog is open only its own toggle applies.
      if (s.paletteOpen && action.kind !== "palette") return;
      e.preventDefault();
      switch (action.kind) {
        case "palette":
          s.setShortcutsOpen(false);
          s.setPaletteOpen(!s.paletteOpen);
          break;
        case "help":
          s.setShortcutsOpen(!s.shortcutsOpen);
          break;
        case "view":
          s.setShortcutsOpen(false);
          s.setView(action.view);
          break;
        case "step":
          s.setView(stepView(s.view, action.delta));
          break;
        case "intercept": {
          if (!s.project) return;
          const i = useInterceptStore.getState();
          void i.setEnabled(!i.state.enabled);
          break;
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
