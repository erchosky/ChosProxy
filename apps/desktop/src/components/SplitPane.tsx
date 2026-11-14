import { type PointerEvent, type ReactNode, useRef, useState } from "react";

interface SplitPaneProps {
  direction: "vertical" | "horizontal";
  /** Initial size of the first pane as a fraction (0-1). */
  initial?: number;
  min?: number;
  first: ReactNode;
  second: ReactNode;
}

/** Two panes with a draggable divider. */
export function SplitPane({ direction, initial = 0.5, min = 0.15, first, second }: SplitPaneProps) {
  const [fraction, setFraction] = useState(initial);
  const [dragging, setDragging] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const vertical = direction === "vertical";

  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
  }
  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    if (!dragging || !container.current) return;
    const rect = container.current.getBoundingClientRect();
    const pos = vertical
      ? (e.clientY - rect.top) / rect.height
      : (e.clientX - rect.left) / rect.width;
    setFraction(Math.min(1 - min, Math.max(min, pos)));
  }
  function onPointerUp(e: PointerEvent<HTMLDivElement>) {
    e.currentTarget.releasePointerCapture(e.pointerId);
    setDragging(false);
  }

  return (
    <div ref={container} className={`split split-${direction}`}>
      <div className="split-pane" style={{ flex: `0 0 ${fraction * 100}%` }}>
        {first}
      </div>
      <div
        className="split-handle"
        role="separator"
        aria-orientation={vertical ? "horizontal" : "vertical"}
        data-dragging={dragging}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      />
      <div className="split-pane" style={{ flex: "1 1 0" }}>
        {second}
      </div>
    </div>
  );
}
