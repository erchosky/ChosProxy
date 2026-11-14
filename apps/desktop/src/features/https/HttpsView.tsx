import { useEffect } from "react";
import { useAppStore } from "../../state/app";
import { BrowserCard } from "./BrowserCard";
import { CaCard } from "./CaCard";
import { PolicyCard } from "./PolicyCard";
import { TrustHelp } from "./TrustHelp";

export function HttpsView() {
  const refreshCa = useAppStore((s) => s.refreshCa);
  const refreshBrowsers = useAppStore((s) => s.refreshBrowsers);
  useEffect(() => {
    void refreshCa();
    void refreshBrowsers();
  }, [refreshCa, refreshBrowsers]);

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1>HTTPS &amp; lab browser</h1>
          <p className="page-sub">
            Inspect encrypted traffic of systems you are authorized to test.
          </p>
        </div>
      </div>
      <BrowserCard />
      <CaCard />
      <PolicyCard />
      <TrustHelp />
    </div>
  );
}
