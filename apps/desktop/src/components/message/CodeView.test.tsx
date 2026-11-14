import { EditorView } from "@codemirror/view";
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CodeView } from "./CodeView";

vi.mock("../../services/ipc", () => ({ api: {}, dialogs: {}, toIpcError: (e: unknown) => e }));
vi.mock("../../state/tools", () => ({ sendToDecoder: vi.fn(), searchProjectFor: vi.fn() }));

const TEXT = "HTTP/1.1 200 OK\nContent-Type: text/plain\n\nfirst token, second token\n";

function editorOf(container: HTMLElement) {
  const dom = container.querySelector(".cm-editor");
  if (!(dom instanceof HTMLElement)) throw new Error("no editor");
  const view = EditorView.findFromDOM(dom);
  if (!view) throw new Error("no editor view");
  return { dom, view };
}

describe("CodeView search", () => {
  it("opens with Ctrl/Cmd+F in a read-only view and moves through the matches", () => {
    const { container } = render(
      <CodeView text={TEXT} language="http" wrap={false} label="Response" />,
    );
    const { dom, view } = editorOf(container);
    const content = dom.querySelector(".cm-content");
    if (!(content instanceof HTMLElement)) throw new Error("no content");

    const mac = /Mac|iP(hone|ad)/.test(navigator.platform);
    fireEvent.keyDown(content, {
      key: "f",
      code: "KeyF",
      keyCode: 70,
      [mac ? "metaKey" : "ctrlKey"]: true,
    });
    const input = dom.querySelector<HTMLInputElement>(".cm-search input[name=search]");
    expect(input).not.toBeNull();
    if (!input) return;

    fireEvent.input(input, { target: { value: "token" } });
    fireEvent.change(input, { target: { value: "token" } });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    const first = view.state.selection.main;
    expect(view.state.sliceDoc(first.from, first.to)).toBe("token");

    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    const second = view.state.selection.main;
    expect(view.state.sliceDoc(second.from, second.to)).toBe("token");
    expect(second.from).toBeGreaterThan(first.from);

    // Searching never edits a read-only view.
    expect(view.state.doc.toString()).toBe(TEXT);
  });
});
