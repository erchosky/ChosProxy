import { useEffect, useState } from "react";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { SplitPane } from "../../components/SplitPane";
import { Switch } from "../../components/Switch";
import { useInterceptStore } from "../../state/intercept";
import { HeldEditor } from "./HeldEditor";
import { HeldList } from "./HeldList";
import { InterceptRulesCard } from "./InterceptRulesCard";
import { MatchReplaceCard } from "./MatchReplaceCard";
import { useAppStore } from "../../state/app";

export function InterceptView() {
  const state = useInterceptStore((s) => s.state);
  const { refresh, setEnabled, forwardAll } = useInterceptStore.getState();
  const [panel, setPanel] = useState<"rules" | "replace" | null>(null);
  const replaceRules = useAppStore(
    (s) => s.proxyConfig?.match_replace.filter((r) => r.enabled).length ?? 0,
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="intercept-view">
      <div className="intercept-bar">
        <Switch
          label={state.enabled ? "Intercept is on" : "Intercept is off"}
          checked={state.enabled}
          onChange={(v) => void setEnabled(v)}
        />
        {state.pending.length > 0 && <Badge tone="warn">{state.pending.length} waiting</Badge>}
        <span className="muted intercept-hint">
          {state.enabled
            ? "Matching messages pause until you forward or drop them. Turning Intercept off forwards everything that is waiting."
            : "Messages flow without pausing."}
        </span>
        <span className="spacer" />
        <Button size="sm" disabled={state.pending.length === 0} onClick={() => void forwardAll()}>
          Forward all
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="settings"
          aria-expanded={panel === "rules"}
          onClick={() => setPanel((p) => (p === "rules" ? null : "rules"))}
        >
          Rules
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="replace"
          aria-expanded={panel === "replace"}
          onClick={() => setPanel((p) => (p === "replace" ? null : "replace"))}
        >
          Match &amp; Replace
          {replaceRules > 0 && <Badge tone="accent">{replaceRules}</Badge>}
        </Button>
      </div>
      {panel && (
        <div className="intercept-rules-wrap">
          {panel === "rules" ? <InterceptRulesCard /> : <MatchReplaceCard />}
        </div>
      )}
      <div className="intercept-split">
        <SplitPane
          direction="horizontal"
          initial={0.3}
          first={<HeldList />}
          second={<HeldEditor />}
        />
      </div>
    </div>
  );
}
