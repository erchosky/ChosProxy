import { SplitPane } from "../../components/SplitPane";
import { useLoggerStore } from "../../state/history";
import { ExchangePanel } from "../proxy/ExchangePanel";
import { HistoryFilters } from "../proxy/HistoryFilters";
import { HistoryTable } from "../proxy/HistoryTable";
import { HistoryStoreContext } from "../proxy/historyContext";

/** Every request sent by any tool (proxy, Repeater, scanner, …), in order. */
export function LoggerView() {
  return (
    <HistoryStoreContext.Provider value={useLoggerStore}>
      <div className="history-view">
        <HistoryFilters />
        <div className="history-split">
          <SplitPane
            direction="vertical"
            initial={0.5}
            first={<HistoryTable />}
            second={<ExchangePanel />}
          />
        </div>
      </div>
    </HistoryStoreContext.Provider>
  );
}
