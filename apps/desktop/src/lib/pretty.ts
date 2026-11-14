export type ContentLanguage = "json" | "xml" | "html" | "javascript" | "css" | "text";

/** Picks a highlighting language from a Content-Type value and the body. */
export function languageOf(contentType: string | null | undefined, body: string): ContentLanguage {
  const ct = (contentType ?? "").split(";")[0]?.trim().toLowerCase() ?? "";
  if (ct.includes("json")) return "json";
  if (ct === "text/html" || ct === "application/xhtml+xml") return "html";
  if (ct.includes("xml") || ct === "image/svg+xml") return "xml";
  if (ct.includes("javascript") || ct.includes("ecmascript")) return "javascript";
  if (ct === "text/css") return "css";
  const head = body.trimStart().slice(0, 64).toLowerCase();
  if (head.startsWith("<!doctype html") || head.startsWith("<html")) return "html";
  if (head.startsWith("<?xml")) return "xml";
  if ((head.startsWith("{") || head.startsWith("[")) && isJson(body)) return "json";
  return "text";
}

function isJson(text: string): boolean {
  try {
    JSON.parse(text);
    return true;
  } catch {
    return false;
  }
}

/** Pretty-prints a body for display. Only JSON is reformatted; other
 * languages are shown as-is (reformatting markup can hide what was sent). */
export function prettyBody(body: string, language: ContentLanguage): string {
  if (language !== "json") return body;
  try {
    return JSON.stringify(JSON.parse(body), null, 2);
  } catch {
    return body;
  }
}
