import type { HeldKind } from "../bindings/HeldKind";

/** Short label for a paused message's kind. */
export function heldKindLabel(kind: HeldKind): string {
  switch (kind) {
    case "request":
      return "request";
    case "response":
      return "response";
    case "wsToServer":
      return "WS → server";
    case "wsToClient":
      return "WS → client";
  }
}

export function isWebSocketKind(kind: HeldKind): boolean {
  return kind === "wsToServer" || kind === "wsToClient";
}
