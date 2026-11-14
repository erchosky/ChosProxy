import type { Operation } from "../../bindings/Operation";

/** Operations grouped for the step picker, with readable labels. */
export const OPERATION_GROUPS: { group: string; ops: { op: Operation; label: string }[] }[] = [
  {
    group: "Decode",
    ops: [
      { op: "urlDecode", label: "URL decode" },
      { op: "base64Decode", label: "Base64 decode" },
      { op: "base64UrlDecode", label: "Base64 URL decode" },
      { op: "hexDecode", label: "Hex decode" },
      { op: "htmlDecode", label: "HTML entities decode" },
      { op: "unicodeUnescape", label: "Unicode unescape" },
      { op: "gzipDecompress", label: "Gzip decompress" },
      { op: "deflateDecompress", label: "Deflate/zlib decompress" },
    ],
  },
  {
    group: "Encode",
    ops: [
      { op: "urlEncode", label: "URL encode" },
      { op: "urlEncodeAll", label: "URL encode all bytes" },
      { op: "base64Encode", label: "Base64 encode" },
      { op: "base64UrlEncode", label: "Base64 URL encode" },
      { op: "hexEncode", label: "Hex encode" },
      { op: "htmlEncode", label: "HTML entities encode" },
      { op: "unicodeEscape", label: "Unicode escape" },
      { op: "gzipCompress", label: "Gzip compress" },
      { op: "deflateCompress", label: "Zlib compress" },
    ],
  },
  {
    group: "Inspect",
    ops: [
      { op: "jsonPretty", label: "Pretty JSON" },
      { op: "jwtDecode", label: "JWT decode (no verification)" },
    ],
  },
  {
    group: "Hash",
    ops: [
      { op: "md5", label: "MD5" },
      { op: "sha1", label: "SHA-1" },
      { op: "sha256", label: "SHA-256" },
      { op: "sha512", label: "SHA-512" },
    ],
  },
];

const LABELS = new Map(OPERATION_GROUPS.flatMap((g) => g.ops.map((o) => [o.op, o.label])));

export function operationLabel(op: Operation): string {
  return LABELS.get(op) ?? op;
}
