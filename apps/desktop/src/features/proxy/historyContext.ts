import { createContext, useContext } from "react";
import { type HistoryStore, useHistoryStore } from "../../state/history";

/** Which traffic list (HTTP history or Logger) the table and panels show. */
export const HistoryStoreContext = createContext<HistoryStore>(useHistoryStore);

export function useHistoryCtx(): HistoryStore {
  return useContext(HistoryStoreContext);
}
