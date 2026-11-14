// One context menu for the whole app: views describe their entries, the
// shell shows them (components/ContextMenu.tsx).
import type { MouseEvent as ReactMouseEvent } from "react";
import { create } from "zustand";
import type { IconName } from "../components/Icon";

export interface MenuItem {
  label: string;
  onSelect(): void;
  icon?: IconName;
  disabled?: boolean;
  /** Shown as a check box (menuitemcheckbox). */
  checked?: boolean;
  danger?: boolean;
  /** Extra text on the right, e.g. a shortcut. */
  hint?: string;
}

export type MenuEntry = MenuItem | { separator: true } | { heading: string };

export const SEPARATOR: MenuEntry = { separator: true };

export interface OpenMenu {
  x: number;
  y: number;
  label: string;
  entries: MenuEntry[];
  /** Element focused before the menu opened (focus returns there). */
  returnFocus: HTMLElement | null;
}

interface ContextMenuState {
  menu: OpenMenu | null;
  open(x: number, y: number, label: string, entries: MenuEntry[]): void;
  close(): void;
}

export const useContextMenuStore = create<ContextMenuState>((set) => ({
  menu: null,
  open(x, y, label, entries) {
    if (entries.length === 0) return;
    const active = document.activeElement;
    set({
      menu: {
        x,
        y,
        label,
        entries,
        returnFocus: active instanceof HTMLElement ? active : null,
      },
    });
  },
  close() {
    set({ menu: null });
  },
}));

/** Opens a menu where the pointer is (an `onContextMenu` handler). */
export function openContextMenu(e: ReactMouseEvent, label: string, entries: MenuEntry[]) {
  e.preventDefault();
  e.stopPropagation();
  useContextMenuStore.getState().open(e.clientX, e.clientY, label, entries);
}

/** Opens a menu under an element (a menu button, or Shift+F10). */
export function openMenuBelow(el: Element, label: string, entries: MenuEntry[]) {
  const r = el.getBoundingClientRect();
  useContextMenuStore.getState().open(r.left, r.bottom + 2, label, entries);
}

/** True for the keys that open a context menu from the keyboard. */
export function isMenuKey(e: { key: string; shiftKey: boolean }) {
  return e.key === "ContextMenu" || (e.shiftKey && e.key === "F10");
}

export function isItem(entry: MenuEntry): entry is MenuItem {
  return "label" in entry;
}
