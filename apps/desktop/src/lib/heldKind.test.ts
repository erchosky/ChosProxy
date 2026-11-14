import { describe, expect, it } from "vitest";
import { heldKindLabel, isWebSocketKind } from "./heldKind";

describe("held kinds", () => {
  it("labels WebSocket directions", () => {
    expect(heldKindLabel("wsToServer")).toBe("WS → server");
    expect(heldKindLabel("request")).toBe("request");
    expect(isWebSocketKind("wsToClient")).toBe(true);
    expect(isWebSocketKind("response")).toBe(false);
  });
});
