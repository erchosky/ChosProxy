import type { ReactNode } from "react";

interface BadgeProps {
  tone?: "neutral" | "ok" | "warn" | "danger" | "accent";
  title?: string;
  children: ReactNode;
}

export function Badge({ tone = "neutral", title, children }: BadgeProps) {
  return (
    <span className={`badge${tone === "neutral" ? "" : ` badge-${tone}`}`} title={title}>
      {children}
    </span>
  );
}
