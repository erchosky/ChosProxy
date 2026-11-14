// Menu entries that copy a request as curl, Python or fetch.
import type { MenuEntry } from "../state/contextMenu";
import { copyText } from "../services/clipboard";
import { useAppStore } from "../state/app";
import { toFetch, toPython } from "./codegen";
import { toCurl } from "./curl";

function copy(text: string, what: string) {
  void copyText(text).then((ok) =>
    useAppStore.getState().notify(ok ? "info" : "error", ok ? `${what} copied` : "Could not copy"),
  );
}

/** "Copy as …" entries for the request `raw` sent to `url`. */
export function copyAsEntries(url: string, raw: () => Uint8Array): MenuEntry[] {
  return [
    {
      label: "Copy as curl command",
      icon: "copy",
      onSelect: () => copy(toCurl(url, raw()), "curl command"),
    },
    {
      label: "Copy as Python (requests)",
      icon: "copy",
      onSelect: () => copy(toPython(url, raw()), "Python code"),
    },
    {
      label: "Copy as JavaScript (fetch)",
      icon: "copy",
      onSelect: () => copy(toFetch(url, raw()), "fetch code"),
    },
  ];
}
