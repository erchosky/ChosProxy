import { beforeEach, describe, expect, it, vi } from "vitest";

const api = {
  organizerList: vi.fn(),
  organizerUpdate: vi.fn(),
  organizerDetail: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  toIpcError: (e: unknown) => ({ kind: "unknown", message: String(e) }),
}));

const { useOrganizerStore } = await import("./organizer");

const item = (id: number, collection: string) => ({
  id,
  exchangeId: id,
  name: `item ${id}`,
  notes: "n",
  tags: ["t"],
  status: "todo" as const,
  severity: "none" as const,
  collection,
  method: "GET",
  url: "http://lab.test/",
  statusCode: 200,
  createdAt: 0,
  updatedAt: 0,
});

describe("organizer store", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    useOrganizerStore.getState().reset();
  });

  it("keeps the newest list when loads overlap", async () => {
    let older: (items: unknown[]) => void = () => undefined;
    api.organizerList
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            older = resolve;
          }),
      )
      .mockResolvedValueOnce([item(1, "New")]);
    const first = useOrganizerStore.getState().load();
    await useOrganizerStore.getState().load();
    older([item(1, "Old")]);
    await first;
    expect(useOrganizerStore.getState().items[0]?.collection).toBe("New");
  });

  it("moves an item to a collection with its other fields untouched", async () => {
    api.organizerList.mockResolvedValue([item(1, "")]);
    await useOrganizerStore.getState().load();
    api.organizerUpdate.mockResolvedValue(null);
    api.organizerList.mockResolvedValue([item(1, "Auth/Login")]);
    expect(await useOrganizerStore.getState().moveTo(1, "Auth/Login")).toBeNull();
    expect(api.organizerUpdate).toHaveBeenCalledWith(1, {
      name: "item 1",
      notes: "n",
      tags: ["t"],
      status: "todo",
      severity: "none",
      collection: "Auth/Login",
    });
    // Already there: nothing to do. Unknown item: said so.
    api.organizerUpdate.mockClear();
    expect(await useOrganizerStore.getState().moveTo(1, "Auth/Login")).toBeNull();
    expect(api.organizerUpdate).not.toHaveBeenCalled();
    expect(await useOrganizerStore.getState().moveTo(9, "x")).toBe("the item is gone");
  });
});
