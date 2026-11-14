import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureConfig } from "../bindings/CaptureConfig";

const api = {
  sequencerDefaults: vi.fn(),
  sequencerPrepare: vi.fn(),
  sequencerStart: vi.fn(),
  sequencerAnalysis: vi.fn(),
  sequencerAnalyze: vi.fn(),
  sequencerTokens: vi.fn(),
  tasks: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) =>
    typeof e === "object" && e !== null && "message" in e
      ? { kind: "invalid", message: String((e as { message: unknown }).message) }
      : { kind: "unknown", message: String(e) },
}));

const { useSequencerStore } = await import("./sequencer");
const { useAppStore } = await import("./app");

const defaults: CaptureConfig = {
  scheme: "http",
  host: "localhost",
  port: 80,
  request: "",
  headCrlf: true,
  bodyCrlf: true,
  location: { kind: "cookie", name: "" },
  count: 500,
  concurrency: 4,
  requestsPerSecond: 20,
  timeoutSecs: 15,
};

describe("sequencer store", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    useSequencerStore.setState({ config: null, captures: [], source: null, analysis: null });
    api.sequencerDefaults.mockResolvedValue(defaults);
    api.tasks.mockResolvedValue([
      { id: 2, kind: "sequencer" },
      { id: 3, kind: "crawl" },
    ]);
  });

  it("prepares a capture from a stored request and starts it", async () => {
    api.sequencerPrepare.mockResolvedValue({
      config: {
        ...defaults,
        scheme: "https",
        host: "shop.example",
        port: 443,
        request: "GET / HTTP/1.1\n\n",
        location: { kind: "cookie", name: "SID" },
      },
      candidates: [{ location: { kind: "cookie", name: "SID" }, sample: "abc" }],
    });
    await useSequencerStore.getState().openExchange(9);
    let s = useSequencerStore.getState();
    expect(s.targetText).toBe("https://shop.example");
    expect(s.candidates).toHaveLength(1);
    expect(s.captures.map((c) => c.id)).toEqual([2]);
    expect(useAppStore.getState().view).toBe("sequencer");

    api.sequencerStart.mockResolvedValue({ id: 5, name: "Tokens from shop.example/" });
    api.sequencerTokens.mockResolvedValue(["a"]);
    expect(await useSequencerStore.getState().start()).toBeNull();
    s = useSequencerStore.getState();
    expect(s.source).toEqual({ kind: "capture", id: 5 });
    expect(s.analysisError).toMatch(/at least two/);
    expect(api.sequencerAnalysis).not.toHaveBeenCalled();

    // When the capture finishes, its tokens are analyzed again.
    api.tasks.mockResolvedValue([{ id: 5, kind: "sequencer", status: "completed" }]);
    api.sequencerTokens.mockResolvedValue(["a", "b"]);
    api.sequencerAnalysis.mockResolvedValue({ samples: 2 });
    useSequencerStore.setState({
      captures: [{ id: 5, kind: "sequencer", status: "running" } as never],
    });
    await useSequencerStore.getState().loadCaptures();
    expect(api.sequencerAnalysis).toHaveBeenCalledWith(5);
    expect(useSequencerStore.getState().analysis).toEqual({ samples: 2 });
  });

  it("analyzes pasted tokens, one per line", async () => {
    await useSequencerStore.getState().init();
    api.sequencerAnalyze.mockResolvedValue({ samples: 2 });
    useSequencerStore.getState().setPasted(" a1 \n\nb2\n");
    await useSequencerStore.getState().analyzePasted();
    expect(api.sequencerAnalyze).toHaveBeenCalledWith(["a1", "b2"]);
    expect(useSequencerStore.getState().sample).toEqual(["a1", "b2"]);

    api.sequencerAnalyze.mockRejectedValue({ message: "at least two tokens are needed" });
    await useSequencerStore.getState().analyzePasted();
    expect(useSequencerStore.getState().analysisError).toBe("at least two tokens are needed");
  });
});
