import type { KeyboardEvent } from "react";

export interface TabItem<T extends string> {
  id: T;
  label: string;
  disabled?: boolean;
  title?: string;
}

interface TabsProps<T extends string> {
  items: readonly TabItem<T>[];
  value: T;
  onChange(value: T): void;
  compact?: boolean;
  label: string;
}

export function Tabs<T extends string>({ items, value, onChange, compact, label }: TabsProps<T>) {
  const enabled = items.filter((i) => !i.disabled);
  function onKeyDown(e: KeyboardEvent) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const idx = enabled.findIndex((i) => i.id === value);
    const next =
      enabled[(idx + (e.key === "ArrowRight" ? 1 : enabled.length - 1)) % enabled.length];
    if (next) onChange(next.id);
    e.preventDefault();
  }
  return (
    <div
      className={`tabs${compact ? " tabs-compact" : ""}`}
      role="tablist"
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {items.map((item) => (
        <button
          key={item.id}
          role="tab"
          type="button"
          className="tab"
          aria-selected={item.id === value}
          tabIndex={item.id === value ? 0 : -1}
          disabled={item.disabled}
          title={item.title}
          onClick={() => onChange(item.id)}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}

interface SegmentedProps<T extends string> {
  items: readonly TabItem<T>[];
  value: T;
  onChange(value: T): void;
  label: string;
}

/** Small toggle-button group for view modes. */
export function Segmented<T extends string>({ items, value, onChange, label }: SegmentedProps<T>) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          aria-pressed={item.id === value}
          disabled={item.disabled}
          title={item.title}
          onClick={() => onChange(item.id)}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
