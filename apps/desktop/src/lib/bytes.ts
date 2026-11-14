/** Decodes base64 into bytes. */
export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

const utf8 = new TextDecoder("utf-8", { fatal: false });

/** Lossy UTF-8 decoding (invalid sequences become U+FFFD). */
export function bytesToText(bytes: Uint8Array): string {
  return utf8.decode(bytes);
}

export interface HexRow {
  offset: string;
  hex: string;
  ascii: string;
}

/** Formats one 16-byte row of a hex dump. */
export function hexRow(bytes: Uint8Array, row: number, width = 16): HexRow {
  const start = row * width;
  const slice = bytes.subarray(start, Math.min(start + width, bytes.length));
  const hexParts: string[] = [];
  let ascii = "";
  for (let i = 0; i < width; i++) {
    const b = slice[i];
    if (b === undefined) {
      hexParts.push("  ");
      continue;
    }
    hexParts.push(b.toString(16).padStart(2, "0"));
    ascii += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".";
  }
  const hex = `${hexParts.slice(0, width / 2).join(" ")}  ${hexParts.slice(width / 2).join(" ")}`;
  return { offset: start.toString(16).padStart(8, "0"), hex, ascii };
}

/** Number of hex-dump rows for a byte length. */
export function hexRowCount(length: number, width = 16): number {
  return Math.ceil(length / width);
}

/** Lowercase hex digits of bytes, no separators. */
export function toHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) out += b.toString(16).padStart(2, "0");
  return out;
}

const strictUtf8 = new TextDecoder("utf-8", { fatal: true });

/**
 * Text for the Decoder: the bytes themselves when they are valid UTF-8
 * without control characters (besides tab and newlines), else hex digits.
 */
export function selectionForDecoder(bytes: Uint8Array): { text: string; asHex: boolean } {
  try {
    const text = strictUtf8.decode(bytes);
    // eslint-disable-next-line no-control-regex
    if (!/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(text)) return { text, asHex: false };
  } catch {
    // Not UTF-8: hex below.
  }
  return { text: toHex(bytes), asHex: true };
}
