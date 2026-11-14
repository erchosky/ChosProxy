import { useState } from "react";

export interface Bar {
  /** Category label (x axis). */
  label: string;
  value: number;
  /** Shown on hover and in the table. */
  detail: string;
  /** Drawn in the "flagged" color (explained by `flaggedLabel`). */
  flagged?: boolean;
}

interface BarChartProps {
  /** What is plotted (the chart's accessible name). */
  title: string;
  bars: Bar[];
  /** Top of the value axis. */
  max: number;
  /** Value axis ticks. */
  ticks: number[];
  formatTick?(v: number): string;
  /** Shaded range on the value axis (e.g. the expected band). */
  band?: { low: number; high: number; label: string };
  /** Legend entry for flagged bars (shown when any bar is flagged). */
  flaggedLabel?: string;
  /** Column header of the values in the table view. */
  valueLabel: string;
  height?: number;
}

const PAD = { top: 8, right: 8, bottom: 22, left: 36 };
const WIDTH = 640;

/** Top-rounded bar from the baseline (square at the base, 4px data end). */
function barPath(x: number, y: number, w: number, base: number): string {
  const r = Math.min(4, w / 2, Math.max(0, base - y));
  return `M${x},${base}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${base}Z`;
}

/** A single-series column chart with hover readout and a table view. */
export function BarChart({
  title,
  bars,
  max,
  ticks,
  formatTick = (v) => String(v),
  band,
  flaggedLabel,
  valueLabel,
  height = 160,
}: BarChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const plotW = WIDTH - PAD.left - PAD.right;
  const plotH = height - PAD.top - PAD.bottom;
  const base = PAD.top + plotH;
  const y = (v: number) => PAD.top + plotH - (Math.max(0, Math.min(v, max)) / max) * plotH;
  const slot = bars.length > 0 ? plotW / bars.length : plotW;
  // Thin marks: at most 24px, always a 2px gap between neighbours.
  const barW = Math.max(1, Math.min(24, slot - 2));
  const labelEvery = Math.max(1, Math.ceil(bars.length / 16));
  const hovered = hover === null ? undefined : bars[hover];
  const anyFlagged = bars.some((b) => b.flagged);

  return (
    <figure className="bar-chart">
      <div className="bar-chart-head">
        <figcaption>{title}</figcaption>
        {anyFlagged && flaggedLabel && (
          <span className="bar-chart-legend">
            <span className="bar-chart-swatch" aria-hidden="true" />
            {flaggedLabel}
          </span>
        )}
        <span className="bar-chart-readout" aria-live="polite">
          {hovered ? `${hovered.label}: ${hovered.detail}` : ""}
        </span>
      </div>
      <svg
        viewBox={`0 0 ${WIDTH} ${height}`}
        role="img"
        aria-label={title}
        className="bar-chart-svg"
        onMouseLeave={() => setHover(null)}
      >
        {band && (
          <rect
            className="bar-chart-band"
            x={PAD.left}
            width={plotW}
            y={y(band.high)}
            height={Math.max(1, y(band.low) - y(band.high))}
          >
            <title>{band.label}</title>
          </rect>
        )}
        {ticks.map((t) => (
          <g key={t}>
            <line
              className="bar-chart-grid"
              x1={PAD.left}
              x2={WIDTH - PAD.right}
              y1={y(t)}
              y2={y(t)}
            />
            <text className="bar-chart-tick" x={PAD.left - 4} y={y(t) + 3} textAnchor="end">
              {formatTick(t)}
            </text>
          </g>
        ))}
        {bars.map((b, i) => {
          const x = PAD.left + i * slot + (slot - barW) / 2;
          // A flagged zero still shows: a 2px stub on the baseline.
          const top = b.value > 0 ? y(b.value) : base - 2;
          return (
            <g key={i}>
              {(b.value > 0 || b.flagged) && (
                <path
                  className={`bar-chart-bar${b.flagged ? " flagged" : ""}${hover === i ? " active" : ""}`}
                  d={barPath(x, top, barW, base)}
                />
              )}
              {/* Hit target: the whole slot, taller than the mark. */}
              <rect
                className="bar-chart-hit"
                x={PAD.left + i * slot}
                width={slot}
                y={PAD.top}
                height={plotH}
                onMouseEnter={() => setHover(i)}
              >
                <title>{`${b.label}: ${b.detail}`}</title>
              </rect>
              {i % labelEvery === 0 && (
                <text
                  className="bar-chart-tick"
                  x={x + barW / 2}
                  y={height - 6}
                  textAnchor="middle"
                >
                  {b.label}
                </text>
              )}
            </g>
          );
        })}
        <line className="bar-chart-axis" x1={PAD.left} x2={WIDTH - PAD.right} y1={base} y2={base} />
      </svg>
      <details className="bar-chart-table">
        <summary>Show as table</summary>
        <table className="endpoint-table">
          <thead>
            <tr>
              <th>Item</th>
              <th>{valueLabel}</th>
            </tr>
          </thead>
          <tbody>
            {bars.map((b, i) => (
              <tr key={i}>
                <td>{b.label}</td>
                <td>
                  {b.detail}
                  {b.flagged && flaggedLabel ? ` (${flaggedLabel})` : ""}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </figure>
  );
}
