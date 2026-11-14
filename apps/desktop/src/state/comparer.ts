import { create } from "zustand";

/** Something to compare: a request, a response or pasted text. */
export interface CompareItem {
  id: number;
  label: string;
  text: string;
}

const MAX_ITEMS = 50;
let nextId = 1;

interface ComparerState {
  items: CompareItem[];
  a: number | null;
  b: number | null;

  /** Adds an item; it fills the first free slot (A, then B). */
  add(label: string, text: string): void;
  remove(id: number): void;
  setSlot(slot: "a" | "b", id: number): void;
  swap(): void;
}

export const useComparerStore = create<ComparerState>((set) => ({
  items: [],
  a: null,
  b: null,

  add(label, text) {
    const item = { id: nextId++, label, text };
    set((s) => {
      const items = [item, ...s.items].slice(0, MAX_ITEMS);
      const alive = (id: number | null) => (items.some((i) => i.id === id) ? id : null);
      let a = alive(s.a);
      let b = alive(s.b);
      if (a === null) a = item.id;
      else if (b === null) b = item.id;
      else {
        // Both slots taken: compare the previous B with the new item.
        a = b;
        b = item.id;
      }
      return { items, a, b };
    });
  },

  remove: (id) =>
    set((s) => ({
      items: s.items.filter((i) => i.id !== id),
      a: s.a === id ? null : s.a,
      b: s.b === id ? null : s.b,
    })),

  setSlot: (slot, id) => set({ [slot]: id }),

  swap: () => set((s) => ({ a: s.b, b: s.a })),
}));
