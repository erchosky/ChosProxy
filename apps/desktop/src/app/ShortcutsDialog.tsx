import { useAppStore } from "../state/app";
import { shortcutList } from "./shortcuts";

/** The `?` overlay: every keyboard shortcut. */
export function ShortcutsDialog() {
  const open = useAppStore((s) => s.shortcutsOpen);
  return open ? <ShortcutsList /> : null;
}

function ShortcutsList() {
  const close = () => useAppStore.getState().setShortcutsOpen(false);
  const bindings = useAppStore((s) => s.settings?.ui.keyBindings) ?? [];
  const list = shortcutList(bindings);
  const groups = [...new Set(list.map((s) => s.group))];
  return (
    <div className="palette-backdrop" role="presentation" onClick={close}>
      <div
        className="dialog shortcuts-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard shortcuts"
        tabIndex={-1}
        ref={(el) => el?.focus()}
        onClick={(e) => e.stopPropagation()}
      >
        <header className="dialog-head">
          <strong>Keyboard shortcuts</strong>
          <button type="button" className="link-button" onClick={close}>
            Close
          </button>
        </header>
        <div className="shortcuts-groups">
          {groups.map((g) => (
            <section key={g} aria-label={g}>
              <h3>{g}</h3>
              <dl>
                {list
                  .filter((s) => s.group === g)
                  .map((s) => (
                    <div key={s.keys} className="shortcut-row">
                      <dt>
                        <kbd>{s.keys}</kbd>
                      </dt>
                      <dd>{s.what}</dd>
                    </div>
                  ))}
              </dl>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
