import { beforeEach, describe, expect, it, vi } from "vitest";

const api = { search: vi.fn() };
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) => ({ kind: "unknown", message: String(e) }),
}));

const { findReferences, folderOf } = await import("./tools");
const { useSearchStore } = await import("./search");
const { useAppStore } = await import("./app");

describe("request tools", () => {
  beforeEach(() => {
    api.search.mockReset();
    api.search.mockResolvedValue({ hits: [], truncated: false, scanned: 0 });
  });

  it("finds references to a URL in stored responses only", () => {
    findReferences("https://lab.test/api/users?id=1");
    const q = useSearchStore.getState().query;
    expect(q).toMatchObject({
      text: "/api/users",
      regex: false,
      inUrls: false,
      inRequestHeaders: false,
      inRequestBodies: false,
      inResponseHeaders: true,
      inResponseBodies: true,
    });
    expect(useAppStore.getState().view).toBe("search");
    expect(api.search).toHaveBeenCalledTimes(1);
    // The root of a site is searched by its host.
    findReferences("https://lab.test:8443/");
    expect(useSearchStore.getState().query.text).toBe("lab.test:8443");
  });

  it("gives the folder of a URL", () => {
    expect(folderOf("https://a.test/b/c?x=1")).toBe("https://a.test/b/");
    expect(folderOf("https://a.test")).toBe("https://a.test/");
  });
});
