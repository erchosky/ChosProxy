import { describe, expect, it } from "vitest";
import {
  base64ToBytes,
  bytesToText,
  hexRow,
  hexRowCount,
  selectionForDecoder,
  toHex,
} from "./bytes";

describe("bytes", () => {
  it("decodes base64 including binary", () => {
    expect(Array.from(base64ToBytes("AAH/"))).toEqual([0, 1, 255]);
  });

  it("decodes UTF-8 lossily", () => {
    expect(bytesToText(new Uint8Array([0x68, 0x69, 0xff]))).toBe("hi�");
  });

  it("formats hex rows with padding and ascii", () => {
    const bytes = new TextEncoder().encode("GET / HTTP/1.1\r\nHost");
    expect(hexRowCount(bytes.length)).toBe(2);
    const row0 = hexRow(bytes, 0);
    expect(row0.offset).toBe("00000000");
    expect(row0.hex.startsWith("47 45 54 20")).toBe(true);
    expect(row0.ascii).toBe("GET / HTTP/1.1..");
    const row1 = hexRow(bytes, 1);
    expect(row1.offset).toBe("00000010");
    expect(row1.ascii).toBe("Host");
    expect(row1.hex.length).toBe(row0.hex.length);
  });
});

describe("selectionForDecoder", () => {
  it("sends text as text and binary as hex", () => {
    expect(selectionForDecoder(new TextEncoder().encode("a=1\nb"))).toEqual({
      text: "a=1\nb",
      asHex: false,
    });
    expect(selectionForDecoder(new Uint8Array([0x1f, 0x8b, 0x08]))).toEqual({
      text: "1f8b08",
      asHex: true,
    });
    expect(selectionForDecoder(new Uint8Array([0x41, 0x00]))).toEqual({
      text: "4100",
      asHex: true,
    });
    expect(toHex(new Uint8Array([0, 255]))).toBe("00ff");
  });
});
