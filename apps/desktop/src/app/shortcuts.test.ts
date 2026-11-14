import { describe, expect, it } from "vitest";
import { NAV } from "./nav";
import {
  type KeyInput,
  QUICK_VIEWS,
  comboOf,
  matchShortcut,
  shortcutList,
  stepView,
  viewShortcut,
} from "./shortcuts";

const key = (k: string, mods: Partial<KeyInput> = {}): KeyInput => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("matchShortcut", () => {
  it("opens the palette and jumps to views with Ctrl or Cmd", () => {
    expect(matchShortcut(key("k", { metaKey: true }), false)).toEqual({ kind: "palette" });
    expect(matchShortcut(key("K", { ctrlKey: true }), true)).toEqual({ kind: "palette" });
    expect(matchShortcut(key("3", { ctrlKey: true }), true)).toEqual({
      kind: "view",
      view: "history",
    });
    expect(matchShortcut(key("I", { metaKey: true, shiftKey: true }), true)).toEqual({
      kind: "intercept",
    });
    expect(matchShortcut(key("0", { ctrlKey: true }), false)).toBeNull();
  });

  it("leaves plain keys to text fields", () => {
    expect(matchShortcut(key("?", { shiftKey: true }), false)).toEqual({ kind: "help" });
    expect(matchShortcut(key("?", { shiftKey: true }), true)).toBeNull();
    expect(matchShortcut(key("ArrowDown", { altKey: true }), false)).toEqual({
      kind: "step",
      delta: 1,
    });
    expect(matchShortcut(key("ArrowUp", { altKey: true }), true)).toBeNull();
    expect(matchShortcut(key("a"), false)).toBeNull();
  });
});

describe("views", () => {
  it("quick views exist in the sidebar and stepping wraps", () => {
    for (const v of QUICK_VIEWS) expect(NAV.some((n) => n.id === v)).toBe(true);
    const first = NAV[0]?.id;
    const last = NAV[NAV.length - 1]?.id;
    expect(first && last && stepView(last, 1)).toBe(first);
    expect(first && last && stepView(first, -1)).toBe(last);
  });

  it("lists every quick view in the overlay", () => {
    const list = shortcutList();
    expect(list.filter((s) => s.group === "Navigate")).toHaveLength(QUICK_VIEWS.length + 2);
    expect(new Set(list.map((s) => s.keys)).size).toBe(list.length);
  });
});

describe("user key bindings", () => {
  it("reads combos from key presses", () => {
    expect(comboOf(key("i", { metaKey: true, shiftKey: true }))).toBe("Mod+Shift+I");
    expect(comboOf(key("?", { shiftKey: true }))).toBe("?");
    expect(comboOf(key("ArrowDown", { altKey: true }))).toBe("Alt+ArrowDown");
    expect(comboOf(key("Shift", { shiftKey: true }))).toBeNull();
  });

  it("overrides, adds and removes shortcuts", () => {
    const overrides = [
      { action: "palette", keys: "Mod+P" },
      { action: "view:oast", keys: "Mod+Shift+O" },
      { action: "help", keys: "" },
    ];
    expect(matchShortcut(key("k", { ctrlKey: true }), false, overrides)).toBeNull();
    expect(matchShortcut(key("p", { ctrlKey: true }), false, overrides)).toEqual({
      kind: "palette",
    });
    expect(matchShortcut(key("O", { metaKey: true, shiftKey: true }), true, overrides)).toEqual({
      kind: "view",
      view: "oast",
    });
    expect(matchShortcut(key("?", { shiftKey: true }), false, overrides)).toBeNull();
    expect(viewShortcut("oast", overrides)).toMatch(/\+Shift\+O$/);
    expect(shortcutList(overrides).some((s) => s.what === "This list of shortcuts")).toBe(false);
  });
});
