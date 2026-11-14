# ChosScripts

Small scripts for things the built-in tools do not cover. Write them in
Tools → Scripts. They are kept with your settings (`scripts.toml`) and work
in every project. The language is [Rhai](https://rhai.rs), which looks like
a mix of JavaScript and Rust ([ADR-0032](adr/0032-chosscripts.md)).

Scripts run in a sandbox. They cannot read or write files, use the network
or start programs. Each run is limited in time and memory, and a script
only sees the message it is given.

## Kinds

| Kind | Write | Where it runs |
|---|---|---|
| Check | `fn check(msg)` (passive) and/or `fn active_probes(value)` + `fn active_check(baseline, probe, payload)` (active) | Passive analysis & active audit: findings |
| Filter | `fn filter(msg)` returning `true` / `false` | HTTP history / Logger → Script |
| Column | `fn column(msg)` returning a value | HTTP history / Logger → Script |
| Action | `fn action(msg)` returning text | Right-click a request → Run script |
| Rewrite | `fn request(text)` and/or `fn response(text)` returning the new text or `()` | Intercept → Match & Replace → Request / Response script |
| Payload | `fn payload(p)` returning the changed payload or `()` | Fuzzer → payload processing → Payload script |

Action scripts also run from Repeater's "Run script" button, on the request
and response shown.

## Sharing

Use **Export all…** and **Import…** in Tools → Scripts. Imported scripts
arrive switched off, so read them before you enable them. If an id is
already in use, the imported script gets a suffix.

## Conditions

HTTP history / Logger → **Conditions** builds a filter from conditions on
the URL, method, status, MIME type, headers, bodies or length. It runs as a
filter script in the same sandbox. **Save as filter script** keeps it in
the library, where you can edit it further.

Start from the template of each kind. Use **Test** with an exchange number
from the history to run it once without saving; the history's selected
request is filled in for you.

## The message (`msg`)

**Properties**
- `method`, `url`, `scheme`, `host`, `port`, `path`, `http_version`
- `status` (0 when there is no response), `mime`, `has_response`
- `request`, `response`: the raw messages as text.
- `request_body`, `response_body`: the bodies with chunking and
  compression removed.

**Functions**
- `request_header(name)`, `response_header(name)`: the first value of a
  header, or `""` if it is missing.
- `response_headers(name)`: all values, as an array.

**Helpers**
- `regex_match(text, re)`, `regex_find(text, re)`,
  `regex_captures(text, re)`, `regex_replace(text, re, with)`
- `base64_encode`, `base64_decode`
- `contains_ci(text, needle)`: a case-insensitive "contains".
- `print(…)`: its output is shown in test runs and actions.

HTTP messages use `\r\n` line breaks. In a regular expression, write
`(?mR)` so that `^` and `$` match at the start and end of each line.

## Check findings

A map with:
- `title` (required);
- `severity`: `info`, `low`, `medium` or `high`;
- `confidence`: `tentative`, `firm` or `certain`;
- `evidence`, `detail`, `remediation`;
- `key`: what makes two detections the same finding. The default is the
  origin and the title.

Findings appear in Findings with the script's name as their type. Use
Findings → Analyze again to run new checks on existing traffic.

## Active check scripts (M13)

A **Check** script can also participate in an active audit scan (when active
audit is enabled and authorized in the Scan launcher) by defining:

```rhai
fn active_probes(value) {
    // Return up to 16 probe values to substitute at each compatible insertion point.
    [value + "chos_custom_probe"]
}

fn active_check(baseline, probe, payload) {
    // Compare the baseline exchange and the probe exchange (`msg` objects).
    if probe.status == 200 && probe.response_body.contains("chos_custom_probe") {
        #{
            title: "Custom reflection detected",
            severity: "medium",
            confidence: "tentative",
            evidence: "Reflected payload: " + payload
        }
    }
}
```

Active check scripts run inside the same Rhai sandbox, share the scan's
per-point and total active request budgets, rate limiter, Resource Pool, and
Session Rules, re-check target scope before every probe, and attach both the
baseline exchange and the proving probe exchange as finding evidence.

## WebSocket messages (M31b)

A rewrite script may also define `fn websocket(text, to_server)`. Add a
Match & Replace rule with the target **WebSocket script** and choose the
script. It runs on text messages in both directions (`to_server` is true
for messages from the client), in rule order with the literal and regex
WebSocket rules. Return the new text, or `()` to leave the message alone.
Binary messages are not given to scripts, and a script that fails leaves
the message unchanged.
