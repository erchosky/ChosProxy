import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FuzzConfig } from "../bindings/FuzzConfig";

const api = {
  fuzzBuiltinLists: vi.fn(),
  fuzzDefaults: vi.fn(),
  fuzzPreview: vi.fn(),
  fuzzAutoMark: vi.fn(),
  fuzzStart: vi.fn(),
  fuzzResults: vi.fn(),
  tasks: vi.fn(),
  exchangeDetail: vi.fn(),
  repeaterSnapshot: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) =>
    typeof e === "object" && e !== null && "message" in e
      ? { kind: "invalid", message: String((e as { message: unknown }).message) }
      : { kind: "unknown", message: String(e) },
}));

const { addMark, countPositions, parseStatusList, setsNeeded, useFuzzerStore } =
  await import("./fuzzer");

const defaults: FuzzConfig = {
  target: { scheme: "http", host: "localhost", port: 80 },
  template: "",
  headCrlf: true,
  bodyCrlf: true,
  mode: "sniper",
  payloadSets: [],
  updateContentLength: true,
  useCookieJar: false,
  concurrency: 4,
  requestsPerSecond: 10,
  timeoutSecs: 15,
  retries: 1,
  maxRequests: 10000,
  grepMatch: [],
  grepRegex: false,
  grepCaseSensitive: false,
  grepExtract: [],
  stop: { afterMatches: 0, afterErrors: 0, onStatus: [] },
};

describe("fuzzer helpers", () => {
  it("marks, counts and sizes payload sets", () => {
    expect(addMark("id=42", 3, 5)).toBe("id=§42§");
    expect(addMark("id=42", 5, 3)).toBe("id=§42§");
    expect(addMark("ab", 1, 1)).toBe("a§§b");
    expect(countPositions("a=§1§&b=§2§")).toBe(2);
    expect(countPositions("a=§1")).toBe(0);
    expect(setsNeeded("sniper", 3)).toBe(1);
    expect(setsNeeded("clusterBomb", 3)).toBe(3);
    expect(setsNeeded("pitchfork", 0)).toBe(1);
  });
});

describe("fuzzer store", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    useFuzzerStore.setState({ config: null, lists: [], attackId: null, attacks: [] });
    api.fuzzBuiltinLists.mockResolvedValue([]);
    api.fuzzDefaults.mockResolvedValue(defaults);
    api.fuzzPreview.mockResolvedValue({ positions: 0, requests: 0, samples: [], error: "x" });
    api.tasks.mockResolvedValue([]);
  });

  it("adds a payload set per position for cluster bomb", async () => {
    await useFuzzerStore.getState().init();
    useFuzzerStore.getState().edit({ template: "§a§:§b§", mode: "clusterBomb" });
    expect(useFuzzerStore.getState().config?.payloadSets).toHaveLength(2);
    expect(useFuzzerStore.getState().targetText).toBe("http://localhost");
  });

  it("opens a stored request and starts an attack", async () => {
    api.repeaterSnapshot.mockResolvedValue({
      detail: { row: { scheme: "https", host: "shop.example", port: 443 } },
      requestText: "GET /?q=§x§ HTTP/1.1\n\n",
    });
    await useFuzzerStore.getState().openExchange(7);
    const s = useFuzzerStore.getState();
    // Old markers are removed.
    expect(s.config?.template).toBe("GET /?q=x HTTP/1.1\n\n");
    expect(s.targetText).toBe("https://shop.example");

    useFuzzerStore.getState().edit({ template: "GET /?q=§x§ HTTP/1.1\n\n" });
    useFuzzerStore.getState().setSet(0, {
      source: { kind: "list", items: ["a"] },
      processing: [],
    });
    api.fuzzStart.mockResolvedValue({ id: 3, name: "Fuzz shop.example/" });
    api.tasks.mockResolvedValue([{ id: 3, kind: "fuzz" }]);
    api.fuzzResults.mockResolvedValue({ rows: [], total: 0 });
    expect(await useFuzzerStore.getState().start()).toBeNull();
    expect(api.fuzzStart).toHaveBeenCalledWith(
      expect.objectContaining({
        target: { scheme: "https", host: "shop.example", port: 443 },
        payloadSets: [{ source: { kind: "list", items: ["a"] }, processing: [] }],
      }),
      null,
    );
    expect(useFuzzerStore.getState().tab).toBe("results");
    expect(useFuzzerStore.getState().attackId).toBe(3);

    api.fuzzStart.mockRejectedValue({ message: "mark at least one payload position" });
    expect(await useFuzzerStore.getState().start()).toBe("mark at least one payload position");
  });
});

describe("stop status codes", () => {
  it("keeps valid, distinct HTTP status codes", () => {
    expect(parseStatusList("429, 503;503 42 abc 600 200")).toEqual([429, 503, 200]);
    expect(parseStatusList("")).toEqual([]);
  });
});

describe("results columns", () => {
  it("hides and shows columns and remembers the choice", () => {
    const { toggleColumn } = useFuzzerStore.getState();
    toggleColumn("error");
    toggleColumn("length");
    expect(useFuzzerStore.getState().hiddenColumns).toEqual(["error", "length"]);
    expect(JSON.parse(localStorage.getItem("chosproxy.fuzzer.hiddenColumns") ?? "[]")).toEqual([
      "error",
      "length",
    ]);
    toggleColumn("error");
    expect(useFuzzerStore.getState().hiddenColumns).toEqual(["length"]);
    toggleColumn("length");
  });
});
