import { useVirtualizer } from "@tanstack/react-virtual";
import { type MouseEvent, useRef, useState } from "react";
import { selectionForDecoder, toHex } from "../../lib/bytes";
import { copyText } from "../../services/clipboard";
import { useAppStore } from "../../state/app";
import { sendToDecoder } from "../../state/tools";
import { Button } from "../Button";

const ROW_HEIGHT = 19;
const WIDTH = 16;

interface Range {
  anchor: number;
  start: number;
  end: number;
}

/**
 * Virtualized hex dump; handles megabytes without rendering every row.
 * Click a byte, then Shift+click another, to select a range for the Decoder.
 */
export function HexView({ bytes }: { bytes: Uint8Array }) {
  const parent = useRef<HTMLDivElement>(null);
  const [range, setRange] = useState<Range | null>(null);
  const rows = Math.ceil(bytes.length / WIDTH);
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => parent.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 20,
  });
  const pick = (i: number, e: MouseEvent) => {
    e.preventDefault();
    setRange((r) =>
      e.shiftKey && r
        ? { anchor: r.anchor, start: Math.min(r.anchor, i), end: Math.max(r.anchor, i) }
        : { anchor: i, start: i, end: i },
    );
  };
  const inRange = (i: number) => range !== null && i >= range.start && i <= range.end;
  const selected = range ? bytes.subarray(range.start, range.end + 1) : null;

  return (
    <div className="hex-wrap">
      {range && selected && (
        <div className="hex-toolbar">
          <span className="muted">
            {selected.length.toLocaleString()} byte{selected.length === 1 ? "" : "s"} selected (0x
            {range.start.toString(16)}–0x{range.end.toString(16)})
          </span>
          <Button
            size="sm"
            variant="ghost"
            icon="decoder"
            onClick={() => {
              const { text, asHex } = selectionForDecoder(selected);
              sendToDecoder(text);
              if (asHex)
                useAppStore
                  .getState()
                  .notify(
                    "info",
                    "Binary bytes sent as hex; add a Hex decode step to get them back",
                  );
            }}
          >
            Decode selection
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon="copy"
            onClick={() => void copyText(toHex(selected))}
          >
            Copy hex
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setRange(null)}>
            Clear
          </Button>
        </div>
      )}
      <div className="hex-view mono" ref={parent} aria-label="Hex dump">
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((item) => {
            const start = item.index * WIDTH;
            const cells = Array.from({ length: WIDTH }, (_, k) => start + k);
            return (
              <div
                key={item.key}
                className="hex-row"
                style={{ transform: `translateY(${item.start}px)`, height: ROW_HEIGHT }}
              >
                <span className="hex-offset">{start.toString(16).padStart(8, "0")}</span>
                <span className="hex-bytes">
                  {cells.map((i, k) => {
                    const b = bytes[i];
                    return (
                      <span
                        key={k}
                        className={`hex-byte${k === WIDTH / 2 ? " hex-mid" : ""}${inRange(i) ? " hex-sel" : ""}`}
                        onMouseDown={b === undefined ? undefined : (e) => pick(i, e)}
                      >
                        {b === undefined ? "  " : b.toString(16).padStart(2, "0")}
                      </span>
                    );
                  })}
                </span>
                <span className="hex-ascii">
                  {cells.map((i, k) => {
                    const b = bytes[i];
                    if (b === undefined) return null;
                    return (
                      <span
                        key={k}
                        className={inRange(i) ? "hex-sel" : undefined}
                        onMouseDown={(e) => pick(i, e)}
                      >
                        {b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "."}
                      </span>
                    );
                  })}
                </span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
