import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CrawlConfig } from "../bindings/CrawlConfig";
import type { TaskRow } from "../bindings/TaskRow";

const api = {
  tasks: vi.fn(),
  taskDetail: vi.fn(),
  taskPause: vi.fn(),
  crawlDefaults: vi.fn(),
  crawlStart: vi.fn(),
  historyPage: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) =>
    typeof e === "object" && e !== null && "message" in e
      ? { kind: "invalid", message: String((e as { message: unknown }).message) }
      : { kind: "unknown", message: String(e) },
}));

const { useTasksStore } = await import("./tasks");
const { useAppStore } = await import("./app");

const defaults: CrawlConfig = {
  startUrls: [],
  maxDepth: 5,
  maxRequests: 500,
  concurrency: 4,
  requestsPerSecond: 10,
  timeoutSecs: 15,
  maxVariantsPerPath: 10,
  maxSimilar: 20,
  submitGetForms: true,
  useCookieJar: true,
  avoid: ["logout"],
  userAgent: "UA",
};

const task = (patch: Partial<TaskRow> = {}): TaskRow => ({
  id: 1,
  kind: "crawl",
  name: "Crawl lab.test",
  status: "running",
  config: JSON.stringify(defaults),
  createdAt: 0,
  startedAt: 0,
  finishedAt: null,
  requests: 3,
  errors: 0,
  discovered: 9,
  queued: 6,
  progress: "depth 1",
  lastError: null,
  ...patch,
});

describe("tasks store", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    useTasksStore.getState().reset();
    api.crawlDefaults.mockResolvedValue(defaults);
    api.tasks.mockResolvedValue([task()]);
    api.taskDetail.mockResolvedValue({ row: task(), logs: [] });
  });

  it("opens the crawl dialog with start URLs and starts the crawl", async () => {
    await useTasksStore.getState().openCrawl(["http://lab.test/app"]);
    expect(useTasksStore.getState().crawl?.startUrls).toEqual(["http://lab.test/app"]);
    useTasksStore.getState().editCrawl({ maxDepth: 2 });

    api.crawlStart.mockRejectedValueOnce({ message: "add at least one start URL" });
    expect(await useTasksStore.getState().startCrawl()).toBe("add at least one start URL");
    expect(useTasksStore.getState().crawl).not.toBeNull();

    api.crawlStart.mockResolvedValue(task());
    expect(await useTasksStore.getState().startCrawl("careful")).toBeNull();
    expect(api.crawlStart).toHaveBeenLastCalledWith(
      expect.objectContaining({ maxDepth: 2, startUrls: ["http://lab.test/app"] }),
      "careful",
    );
    const s = useTasksStore.getState();
    expect(s.crawl).toBeNull();
    expect(s.selectedId).toBe(1);
    expect(s.rows).toHaveLength(1);
    expect(useAppStore.getState().view).toBe("tasks");
  });

  it("suggests the sites of recent traffic", async () => {
    const row = (scheme: string, host: string, port: number, inScope: boolean) => ({
      scheme,
      host,
      port,
      inScope,
    });
    api.historyPage.mockResolvedValue({
      rows: [
        row("http", "127.0.0.1", 9090, false),
        row("https", "shop.example", 443, true),
        row("https", "shop.example", 443, true),
        row("http", "api.example", 8080, true),
      ],
    });
    await useTasksStore.getState().openCrawl();
    expect(useTasksStore.getState().crawl?.startUrls).toEqual([
      "https://shop.example/",
      "http://api.example:8080/",
    ]);
    api.historyPage.mockRejectedValue(new Error("no project"));
    await useTasksStore.getState().openCrawl();
    expect(useTasksStore.getState().crawl?.startUrls).toEqual([]);
  });

  it("reports failed controls", async () => {
    const notify = vi.spyOn(useAppStore.getState(), "notify");
    api.taskPause.mockRejectedValue({ message: "the task is not running" });
    await useTasksStore.getState().pause(1);
    expect(notify).toHaveBeenCalledWith("error", "Could not pause: the task is not running");
  });
});
