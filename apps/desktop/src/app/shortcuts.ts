import type { KeyBinding } from "../bindings/KeyBinding";
import type { ViewId } from "../state/app";
import { NAV } from "./nav";

/** Views reachable with Mod+1…9 by default, in that order. */
export const QUICK_VIEWS: readonly ViewId[] = [
  "dashboard",
  "intercept",
  "history",
  "sitemap",
  "findings",
  "repeater",
  "fuzzer",
  "tasks",
  "decoder",
];

export type ShortcutAction =
  | { kind: "palette" }
  | { kind: "help" }
  | { kind: "view"; view: ViewId }
  | { kind: "step"; delta: 1 | -1 }
  | { kind: "intercept" };

export interface KeyInput {
  key: string;
  code?: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** An action that can have a shortcut. */
export interface BindableAction {
  id: string;
  label: string;
  group: "General" | "Navigate";
  defaults: string[];
  action: ShortcutAction;
}

/** Every action that can be bound, with its default keys. */
export const BINDABLE: readonly BindableAction[] = [
  {
    id: "palette",
    label: "Command palette",
    group: "General",
    defaults: ["Mod+K"],
    action: { kind: "palette" },
  },
  {
    id: "help",
    label: "This list of shortcuts",
    group: "General",
    defaults: ["?", "F1"],
    action: { kind: "help" },
  },
  {
    id: "intercept",
    label: "Turn Intercept on or off",
    group: "General",
    defaults: ["Mod+Shift+I"],
    action: { kind: "intercept" },
  },
  {
    id: "step:next",
    label: "Next view in the sidebar",
    group: "Navigate",
    defaults: ["Alt+ArrowDown"],
    action: { kind: "step", delta: 1 },
  },
  {
    id: "step:previous",
    label: "Previous view in the sidebar",
    group: "Navigate",
    defaults: ["Alt+ArrowUp"],
    action: { kind: "step", delta: -1 },
  },
  ...NAV.map((n): BindableAction => {
    const i = QUICK_VIEWS.indexOf(n.id);
    return {
      id: `view:${n.id}`,
      label: n.label,
      group: "Navigate",
      defaults: i >= 0 ? [`Mod+${i + 1}`] : [],
      action: { kind: "view", view: n.id },
    };
  }),
];

const MODIFIER_KEYS = new Set(["Control", "Meta", "Alt", "Shift", "AltGraph", "CapsLock"]);

/**
 * The combo a key press stands for (`Mod+Shift+I`, `Alt+ArrowDown`, `?`),
 * or null for a lone modifier. Shift is implied by symbol keys (`?`).
 */
export function comboOf(e: KeyInput): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Mod");
  if (e.altKey) parts.push("Alt");
  const single = e.key.length === 1;
  const symbol = single && !/[a-z0-9]/i.test(e.key);
  if (e.shiftKey && !symbol) parts.push("Shift");
  parts.push(single ? e.key.toUpperCase() : e.key);
  return parts.join("+");
}

/** The keys an action has: the user's choice, else its defaults. */
export function keysFor(action: BindableAction, overrides: readonly KeyBinding[]): string[] {
  const o = overrides.find((b) => b.action === action.id);
  if (!o) return action.defaults;
  return o.keys === "" ? [] : [o.keys];
}

/**
 * The global shortcut a key press means, if any. `editing` is true while
 * focus is in a text field or editor: keys a text field would use (`?`,
 * Alt+arrows) are left to it there; shortcuts with Ctrl/Cmd still apply.
 */
export function matchShortcut(
  e: KeyInput,
  editing: boolean,
  overrides: readonly KeyBinding[] = [],
): ShortcutAction | null {
  const combo = comboOf(e);
  if (!combo) return null;
  if (editing && !combo.startsWith("Mod+")) return null;
  const found = BINDABLE.find((a) => keysFor(a, overrides).includes(combo));
  return found ? found.action : null;
}

/** The view `delta` places away from `from` in the sidebar, wrapping. */
export function stepView(from: ViewId, delta: 1 | -1): ViewId {
  const i = NAV.findIndex((n) => n.id === from);
  const next = (Math.max(i, 0) + delta + NAV.length) % NAV.length;
  return NAV[next]?.id ?? from;
}

/** Whether focus is somewhere that types text. */
export function isEditing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag === "INPUT") {
    const type = (target as HTMLInputElement).type;
    return !["checkbox", "radio", "button", "submit", "range", "color"].includes(type);
  }
  return target.closest(".cm-editor") !== null;
}

/** "⌘" on macOS, "Ctrl" elsewhere. */
export function modLabel(): string {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform)
    ? "⌘"
    : "Ctrl";
}

/** A combo for display (`Mod` as ⌘ or Ctrl, arrows as symbols). */
export function displayKeys(combo: string): string {
  return combo
    .replace(/^Mod\+/, `${modLabel()}+`)
    .replace("ArrowDown", "↓")
    .replace("ArrowUp", "↑")
    .replace("ArrowLeft", "←")
    .replace("ArrowRight", "→");
}

/** The first shortcut of a view, for display, if it has one. */
export function viewShortcut(view: ViewId, overrides: readonly KeyBinding[] = []): string | null {
  const a = BINDABLE.find((b) => b.id === `view:${view}`);
  const first = a ? keysFor(a, overrides)[0] : undefined;
  return first ? displayKeys(first) : null;
}

/** Everything listed in the shortcuts overlay. */
export function shortcutList(
  overrides: readonly KeyBinding[] = [],
): { keys: string; what: string; group: string }[] {
  const m = modLabel();
  const bound = BINDABLE.flatMap((a) => {
    const keys = keysFor(a, overrides);
    return keys.length === 0
      ? []
      : [{ group: a.group, keys: keys.map(displayKeys).join(" / "), what: a.label }];
  });
  return [
    ...bound,
    { group: "General", keys: "Esc", what: "Close a dialog, menu or the palette" },
    { group: "Editors", keys: `${m}+Enter`, what: "Send the Repeater request" },
    { group: "Editors", keys: `${m}+F`, what: "Find in the message" },
    { group: "Lists", keys: "↑ ↓ PgUp PgDn Home End", what: "Move through the history" },
    { group: "Lists", keys: "← →", what: "Switch tabs in a focused tab bar" },
    {
      group: "Lists",
      keys: "Shift+F10",
      what: "Menu of the focused row, or of the selection in an editor",
    },
  ];
}
