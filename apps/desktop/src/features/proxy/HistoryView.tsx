import { SplitPane } from "../../components/SplitPane";
import { ExchangePanel } from "./ExchangePanel";
import { HistoryFilters } from "./HistoryFilters";
import { HistoryTable } from "./HistoryTable";

export function HistoryView() {
  return (
    <div className="history-view">
      <HistoryFilters />
      <div className="history-split">
        <SplitPane
          direction="vertical"
          initial={0.45}
          first={<HistoryTable />}
          second={<ExchangePanel />}
        />
      </div>
    </div>
  );
}
