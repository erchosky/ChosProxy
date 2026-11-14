import type { ScriptKind } from "../bindings/ScriptKind";

export const SCRIPT_KINDS: { id: ScriptKind; label: string; help: string }[] = [
  {
    id: "check",
    label: "Check",
    help: "Runs during passive analysis on every analyzed response; what it returns becomes findings.",
  },
  {
    id: "filter",
    label: "Filter",
    help: "HTTP history → Script: shows only the rows for which it returns true.",
  },
  {
    id: "column",
    label: "Column",
    help: "HTTP history → Script: adds a column with the value it returns.",
  },
  {
    id: "action",
    label: "Action",
    help: "Right-click a request → Run script: shows the text it returns.",
  },
  {
    id: "payload",
    label: "Payload",
    help: "Fuzzer → payload processing → Payload script: returns each payload changed.",
  },
  {
    id: "rewrite",
    label: "Rewrite",
    help: "Intercept → Match & Replace → Request/Response script: returns the new message text.",
  },
];

/** A starting point for each kind; every one works on the lab server. */
export const TEMPLATES: Record<ScriptKind, string> = {
  check: `// Reports a finding when a response discloses a server version.
// Return (), a map, or an array of maps with: title (required), severity
// (info|low|medium|high), confidence (tentative|firm|certain), evidence,
// detail, remediation, key (what makes two detections the same finding).
fn check(msg) {
    let server = msg.response_header("server");
    if regex_match(server, "/[0-9]") {
        #{
            title: "Server version disclosed",
            severity: "low",
            confidence: "certain",
            evidence: "Server: " + server,
            detail: "The Server header names the software version.",
            remediation: "Remove the version from the Server header.",
            key: msg.host + " " + server,
        }
    }
}
`,
  filter: `// Keeps responses that set a cookie.
fn filter(msg) {
    msg.response_header("set-cookie") != ""
}
`,
  column: `// Shows the value of the Content-Security-Policy header (or "none").
fn column(msg) {
    let csp = msg.response_header("content-security-policy");
    if csp == "" { "none" } else { csp }
}
`,
  action: `// Lists the parameters of the request.
fn action(msg) {
    let out = [];
    let query = regex_find(msg.path, "\\\\?.*");
    if query != () {
        for pair in query.sub_string(1).split("&") {
            out.push(pair);
        }
    }
    if msg.request_body != "" {
        out.push("body: " + msg.request_body);
    }
    if out.is_empty() {
        return "No parameters.";
    }
    let text = out.reduce(|sum, x| sum + "\\n" + x, "");
    text.trim();
    text
}
`,
  payload: `// Changes each Fuzzer payload; return () to keep it.
// Here: a JSON string with the payload, Base64-encoded.
fn payload(p) {
    base64_encode("{\\"user\\":\\"" + p + "\\"}")
}
`,
  rewrite: `// Changes requests and/or responses; return () to leave one unchanged.
// HTTP messages use \\r\\n line breaks: in regular expressions use (?mR) so
// ^ and $ work per line.
fn request(text) {
    regex_replace(text, "(?mR)^User-Agent: .*$", "User-Agent: ChosProxy")
}

fn response(text) {
    ()
}

// Text WebSocket messages (rule target "WebSocket script"); to_server is
// true for messages from the client.
fn websocket(text, to_server) {
    ()
}
`,
};

/** A new id from a name: lower case, digits and dashes, unique. */
export function scriptId(name: string, taken: readonly string[]): string {
  const base =
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 40) || "script";
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}
