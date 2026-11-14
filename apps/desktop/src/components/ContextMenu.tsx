import { type KeyboardEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { isItem, type OpenMenu, useContextMenuStore } from "../state/contextMenu";
import { Icon } from "./Icon";

/** The app's context menu (mounted once by the shell). */
export function ContextMenu() {
  const menu = useContextMenuStore((s) => s.menu);
  if (!menu) return null;
  return <Menu menu={menu} />;
}

function Menu({ menu }: { menu: OpenMenu }) {
  const close = useContextMenuStore((s) => s.close);
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: menu.x, top: menu.y });

  // Keep the menu on screen, then focus its first item.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const margin = 6;
    setPosition({
      left: Math.max(margin, Math.min(menu.x, window.innerWidth - width - margin)),
      top: Math.max(margin, Math.min(menu.y, window.innerHeight - height - margin)),
    });
    items()[0]?.focus();
  }, [menu]);

  useEffect(() => {
    const dismiss = (e: Event) => {
      if (e.target instanceof Node && ref.current?.contains(e.target)) return;
      close();
    };
    const onBlur = () => close();
    // Not closed on scroll: opening a menu often selects a row, and the
    // views that reload for it scroll (the menu is fixed, so it stays right).
    window.addEventListener("mousedown", dismiss, true);
    window.addEventListener("resize", onBlur);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("mousedown", dismiss, true);
      window.removeEventListener("resize", onBlur);
      window.removeEventListener("blur", onBlur);
    };
  }, [close]);

  function items(): HTMLButtonElement[] {
    return Array.from(
      ref.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
    );
  }

  function dismiss() {
    close();
    menu.returnFocus?.focus();
  }

  function onKeyDown(e: KeyboardEvent) {
    const list = items();
    const at = list.findIndex((b) => b === document.activeElement);
    const go = (i: number) => list[(i + list.length) % list.length]?.focus();
    switch (e.key) {
      case "ArrowDown":
        go(at + 1);
        break;
      case "ArrowUp":
        go(at - 1);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(list.length - 1);
        break;
      case "Escape":
      case "Tab":
        dismiss();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  return (
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      aria-label={menu.label}
      style={position}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {menu.entries.map((entry, i) => {
        if (!isItem(entry)) {
          return "heading" in entry ? (
            <div key={i} className="context-menu-heading" role="presentation">
              {entry.heading}
            </div>
          ) : (
            <div key={i} className="context-menu-separator" role="separator" />
          );
        }
        const checkable = entry.checked !== undefined;
        return (
          <button
            key={i}
            type="button"
            role={checkable ? "menuitemcheckbox" : "menuitem"}
            aria-checked={checkable ? entry.checked : undefined}
            className={`context-menu-item${entry.danger ? " danger" : ""}`}
            disabled={entry.disabled}
            onClick={() => {
              dismiss();
              entry.onSelect();
            }}
          >
            <span className="context-menu-icon" aria-hidden="true">
              {checkable
                ? entry.checked && <Icon name="check" size={13} />
                : entry.icon && <Icon name={entry.icon} size={13} />}
            </span>
            <span className="context-menu-label">{entry.label}</span>
            {entry.hint && <span className="context-menu-hint">{entry.hint}</span>}
          </button>
        );
      })}
    </div>
  );
}
