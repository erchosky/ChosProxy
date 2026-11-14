import type { Confidence } from "../../bindings/Confidence";
import type { FindingSeverity } from "../../bindings/FindingSeverity";
import type { FindingStatus } from "../../bindings/FindingStatus";

export const SEVERITIES: FindingSeverity[] = ["critical", "high", "medium", "low", "info"];
export const CONFIDENCES: Confidence[] = ["certain", "firm", "tentative"];
export const STATUSES: { value: FindingStatus; label: string }[] = [
  { value: "open", label: "Open" },
  { value: "confirmed", label: "Confirmed" },
  { value: "fixed", label: "Fixed" },
  { value: "acceptedRisk", label: "Accepted risk" },
  { value: "falsePositive", label: "False positive" },
];

export const statusLabel = (s: FindingStatus) => STATUSES.find((x) => x.value === s)?.label ?? s;
