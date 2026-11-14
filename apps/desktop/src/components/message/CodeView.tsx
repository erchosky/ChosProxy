import { copyText } from "../../services/clipboard";
import {
  isMenuKey,
  type MenuEntry,
  openContextMenu,
  openMenuBelow,
  SEPARATOR,
} from "../../state/contextMenu";
import { searchProjectFor, sendToDecoder } from "../../state/tools";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import {
  HighlightStyle,
  syntaxHighlighting,
  type LanguageSupport,
  type StreamLanguage,
} from "@codemirror/language";
import { Compartment, EditorState, type Extension, Prec } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { tags } from "@lezer/highlight";
import { useEffect, useRef } from "react";
import type { ContentLanguage } from "../../lib/pretty";
import { httpLanguage } from "./httpLanguage";

export type ViewLanguage = ContentLanguage | "http";

const languages: Record<ViewLanguage, () => LanguageSupport | StreamLanguage<unknown> | null> = {
  json: () => json(),
  xml: () => xml(),
  // Highlighting only; nothing is rendered or executed.
  html: () => html(),
  javascript: () => javascript(),
  css: () => css(),
  http: () => httpLanguage as StreamLanguage<unknown>,
  text: () => null,
};

const highlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.operatorKeyword], color: "var(--syn-keyword)", fontWeight: "600" },
  { tag: [tags.propertyName, tags.attributeName], color: "var(--syn-key)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--syn-string)" },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: "var(--syn-number)" },
  { tag: [tags.tagName, tags.typeName], color: "var(--syn-tag)" },
  { tag: [tags.comment, tags.meta], color: "var(--syn-comment)" },
  { tag: tags.punctuation, color: "var(--text-faint)" },
]);

const baseTheme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--code-bg)", color: "var(--text)" },
  ".cm-scroller": {
    fontFamily: '"JetBrains Mono", "Fira Code", "DejaVu Sans Mono", ui-monospace, monospace',
    fontSize: "12px",
    lineHeight: "1.55",
  },
  ".cm-gutters": {
    backgroundColor: "var(--code-bg)",
    color: "var(--text-faint)",
    borderRight: "1px solid var(--border)",
  },
  ".cm-content": { caretColor: "var(--accent)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--selection) !important",
  },
  ".cm-selectionMatch": { backgroundColor: "var(--selection)" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--warn) 35%, transparent)" },
  ".cm-panels": { backgroundColor: "var(--bg-elev-2)", color: "var(--text)" },
  ".cm-panels input, .cm-panels button": { fontSize: "12px" },
  "&.cm-focused": { outline: "none" },
});

/** Longest selection reported to `onSelection`. */
const MAX_SELECTION = 1024 * 1024;

interface CodeViewProps {
  text: string;
  language: ViewLanguage;
  wrap: boolean;
  label: string;
  /** Allow editing (undo/redo included). Read-only by default. */
  editable?: boolean;
  /** Called with the full text after every edit. */
  onChange?(text: string): void;
  /** Ctrl/Cmd+Enter inside the editor. */
  onSubmit?(): void;
  /** Called with the selected text whenever the selection changes. */
  onSelection?(text: string): void;
  /** Extra entries for the right-click / Shift+F10 menu. */
  menu?(): MenuEntry[];
}

/** Code view: read-only by default (selection, copy and Ctrl/Cmd+F search
 * work), or an editor when `editable` is set. */
export function CodeView({
  text,
  language,
  wrap,
  label,
  editable = false,
  onChange,
  onSubmit,
  onSelection,
  menu,
}: CodeViewProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const langSlot = useRef(new Compartment());
  const wrapSlot = useRef(new Compartment());
  // Latest callbacks without re-creating the editor.
  const callbacks = useRef({ onChange, onSubmit, onSelection });
  useEffect(() => {
    callbacks.current = { onChange, onSubmit, onSelection };
  }, [onChange, onSubmit, onSelection]);

  useEffect(() => {
    if (!host.current) return;
    const lang = languages[language]();
    const editing: Extension[] = editable
      ? [
          history(),
          Prec.highest(
            keymap.of([
              {
                key: "Mod-Enter",
                run: () => {
                  callbacks.current.onSubmit?.();
                  return true;
                },
              },
            ]),
          ),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) callbacks.current.onChange?.(u.state.doc.toString());
          }),
        ]
      : [EditorState.readOnly.of(true)];
    const extensions: Extension[] = [
      lineNumbers(),
      ...editing,
      EditorView.updateListener.of((u) => {
        if (!u.selectionSet || !callbacks.current.onSelection) return;
        const r = u.state.selection.main;
        callbacks.current.onSelection(
          u.state.sliceDoc(r.from, Math.min(r.to, r.from + MAX_SELECTION)),
        );
      }),
      search({ top: true }),
      highlightSelectionMatches(),
      keymap.of(searchKeymap),
      syntaxHighlighting(highlight),
      baseTheme,
      EditorView.contentAttributes.of({ "aria-label": label }),
      langSlot.current.of(lang ?? []),
      wrapSlot.current.of(wrap ? EditorView.lineWrapping : []),
    ];
    view.current = new EditorView({
      parent: host.current,
      state: EditorState.create({ doc: text, extensions }),
    });
    return () => {
      view.current?.destroy();
      view.current = null;
    };
    // Created once; later prop changes are applied by the effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (!v || v.state.doc.toString() === text) return;
    v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text } });
  }, [text]);

  useEffect(() => {
    view.current?.dispatch({ effects: langSlot.current.reconfigure(languages[language]() ?? []) });
  }, [language]);

  useEffect(() => {
    view.current?.dispatch({
      effects: wrapSlot.current.reconfigure(wrap ? EditorView.lineWrapping : []),
    });
  }, [wrap]);

  const openMenu = (open: (entries: MenuEntry[]) => void) => {
    const v = view.current;
    const r = v?.state.selection.main;
    const selected = v && r ? v.state.sliceDoc(r.from, Math.min(r.to, r.from + MAX_SELECTION)) : "";
    const entries: MenuEntry[] = [];
    if (selected) {
      entries.push(
        { heading: "Selection" },
        {
          label: "Copy",
          icon: "copy",
          onSelect: () => void copyText(selected),
        },
        { label: "Decode", icon: "decoder", onSelect: () => sendToDecoder(selected) },
        {
          label: "Search the project for it",
          icon: "search",
          onSelect: () => searchProjectFor(selected),
        },
      );
    }
    const extra = menu?.() ?? [];
    if (extra.length > 0) entries.push(...(entries.length > 0 ? [SEPARATOR] : []), ...extra);
    if (entries.length > 0) open(entries);
  };

  return (
    <div
      className="code-view selectable"
      ref={host}
      onContextMenu={(e) => openMenu((entries) => openContextMenu(e, "Editor actions", entries))}
      onKeyDown={(e) => {
        if (!isMenuKey(e) || !host.current) return;
        e.preventDefault();
        const el = host.current;
        openMenu((entries) => openMenuBelow(el, "Editor actions", entries));
      }}
    />
  );
}
