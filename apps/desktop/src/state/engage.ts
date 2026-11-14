// Which engagement-tool dialog is open, and for which stored request.
import { create } from "zustand";
import type { SiteFilter } from "../bindings/SiteFilter";

interface EngageState {
  csrfFor: number | null;
  frameFor: number | null;
  analyzeFor: SiteFilter | null;
  openCsrf(exchangeId: number): void;
  openFrame(exchangeId: number): void;
  openAnalyzer(site: SiteFilter): void;
  close(): void;
}

export const useEngageStore = create<EngageState>((set) => ({
  csrfFor: null,
  frameFor: null,
  analyzeFor: null,
  openCsrf: (csrfFor) => set({ csrfFor, frameFor: null, analyzeFor: null }),
  openFrame: (frameFor) => set({ frameFor, csrfFor: null, analyzeFor: null }),
  openAnalyzer: (analyzeFor) => set({ analyzeFor, csrfFor: null, frameFor: null }),
  close: () => set({ csrfFor: null, frameFor: null, analyzeFor: null }),
}));
