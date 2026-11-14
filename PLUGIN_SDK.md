# ChosProxy Plugin SDK (APIs v1 and v2)

A plugin adds checks to ChosProxy. It is a separate program, written in any
language, that talks to ChosProxy with one JSON object per line on its
standard input and output. ChosProxy decides what the plugin may do through
it, permission by permission; nothing is granted automatically.

> **Security.** A plugin runs on your computer with your user's rights.
> Permissions control what it can do *through ChosProxy* (receive traffic,
> add findings, annotate the history) and every message is checked — but
> they do not sandbox the program itself. Install only plugins you trust.
> ChosProxy fingerprints a plugin's files when you enable it; if they change,
> it stops running until you approve it again.

## Layout

Plugins live in the plugins folder (shown in **Plugins**; by default
`<data dir>/plugins/`). Each plugin is a folder named after its id:

```
plugins/
  hello-plugin/
    plugin.toml
    hello.py
```

### `plugin.toml`

```toml
id = "hello-plugin"          # 2–64 lower-case letters, digits, dashes; = folder name
name = "HelloPlugin"
version = "1.0.0"
description = "What it does."
author = "You"
api = 1                      # plugin API version
command = ["python3", "hello.py"]   # run in the plugin folder
permissions = ["network.read", "scanner.passive", "project.write"]
```

`command[0]` is looked up in `PATH`, or relative to the plugin folder when
it contains a `/`. Paths with `..` and absolute paths are refused. The
process gets a minimal environment (`PATH`, `HOME`, `LANG`, temporary
folders and `CHOSPROXY_PLUGIN_API`, the manifest's API version): no variables of yours are passed on.

## Permissions (API v1)

| Permission | Allows |
|---|---|
| `network.read` | Receive every request and response stored in the open project. |
| `scanner.passive` | Add findings (source "plugin"), merged by key, with the exchange as evidence. |
| `project.write` | Highlight history entries and set their notes. |

API v2 (`api = 2` in the manifest) adds four more:

| Permission | Allows |
|---|---|
| `network.send` | Send requests through ChosProxy (`send`): stored in the history (source "plugin"), refused out of scope when a scope is defined, at most 4 at once and 300 per minute. |
| `project.read` | Read the project on request (`query`): one exchange, recent history rows, findings. |
| `ui.extend` | Add actions to every request menu (`registerAction`); running one sends the request to the plugin (`action`) and shows its `output`. |
| `proxy.intercept` | See in-scope requests going through the proxy before they are sent (`registerHook`, `hook`) and let them go, change them, block them or answer them (`hookResult`). Only while a scope is defined. |

A v1 manifest that asks for them is refused; v1 plugins keep working
unchanged. The tester can grant a
subset of what a plugin asks for; messages that need a permission that was
not granted are refused, logged in the plugin's log and recorded in the
project audit log.

## Protocol

ChosProxy starts enabled plugins when a project opens and stops them when it
closes. Every message is one line of JSON (UTF-8), at most 1 MiB.

### From ChosProxy

```json
{"type":"hello","api":1,"plugin":"hello-plugin","granted":["network.read","scanner.passive"]}
```
The first message. Answer with `{"type":"ready"}` within 10 seconds.

```json
{"type":"exchange","id":42,"source":"proxy","method":"GET","url":"https://lab.test/","host":"lab.test",
 "status":200,"requestHeaders":[["Host","lab.test"]],"requestBody":"",
 "responseHeaders":[["Content-Type","text/html"]],"responseBody":"<html>…"}
```
Sent for every exchange stored after the plugin started (only with
`network.read`). Bodies are text (the response decoded, both lossy UTF-8)
cut at 256 KiB. Read your input promptly: a plugin that stops reading for 5
seconds is stopped.

```json
{"type":"shutdown"}
```
Exit now. A plugin that does not exit within 2 seconds is killed.

### From the plugin

```json
{"type":"ready"}
{"type":"log","level":"info","message":"text"}
{"type":"finding","exchangeId":42,"key":"lab.test","kind":"Debug token exposed",
 "title":"Debug token header on lab.test","severity":"info","confidence":"certain",
 "description":"…","remediation":"…","evidence":"X-Debug-Token: a1b2c3"}
{"type":"annotate","exchangeId":42,"highlight":"yellow","note":"text"}
```

* `log` levels: `info`, `warning`, `error`. Lines are kept in the plugin's
  log (Plugins → Log), 500 per plugin.
* `finding` needs `scanner.passive`. Severity: `info`, `low`, `medium`,
  `high`; confidence: `tentative`, `firm`, `certain`. Findings with the same
  `key` are one finding (up to five pieces of evidence); findings the tester
  marked as false positive or accepted risk stay quiet. At most 120 per
  minute.
* `annotate` needs `project.write`. Highlights: `red`, `orange`, `yellow`,
  `green`, `cyan`, `blue`, `purple`, `gray`. Omitted fields keep their
  value.

Anything written to standard error appears in the plugin's log. After 20
invalid messages the plugin is stopped.

## API v2 calls

Calls carry an `id` you choose; ChosProxy answers each with a `result`:

```json
{"type":"send","id":1,"scheme":"https","host":"lab.test","port":443,"request":"GET /api HTTP/1.1\nHost: lab.test\n\n"}
{"type":"result","id":1,"ok":true,"data":{"exchangeId":57,"status":200,"response":"HTTP/1.1 200 OK\r\n…"}}

{"type":"query","id":2,"query":{"what":"history","search":"/api","limit":20}}
{"type":"query","id":3,"query":{"what":"exchange","exchangeId":57}}
{"type":"query","id":4,"query":{"what":"findings"}}
{"type":"result","id":4,"ok":false,"error":"project.read not granted"}
```

Menu actions:

```json
{"type":"registerAction","action":"decode-jwt","label":"Decode the JWT"}
{"type":"action","call":9,"action":"decode-jwt","exchange":{…same shape as "exchange"…}}
{"type":"output","call":9,"text":"header: {…}\npayload: {…}"}
```

Answer an action within 30 seconds. Requests a plugin sends are not fed
back to it as `exchange` messages, so a plugin cannot loop on its own
traffic.

### Request hooks (`proxy.intercept`, M26)

```json
{"type":"registerHook","hosts":["lab.test","*.lab.test"]}
{"type":"hook","call":5,"request":{"scheme":"https","host":"lab.test","port":443,
 "url":"https://lab.test/api","raw":"GET /api HTTP/1.1\r\nHost: lab.test\r\n\r\n"}}
{"type":"hookResult","call":5,"action":"continue"}
{"type":"hookResult","call":5,"action":"replace","request":"GET /api?debug=0 HTTP/1.1\nHost: lab.test\n\n"}
{"type":"hookResult","call":5,"action":"block","reason":"not during the test window"}
{"type":"hookResult","call":5,"action":"respond","response":"HTTP/1.1 200 OK\nContent-Type: text/plain\n\nstub"}
```

* `hosts` is optional (empty: every in-scope host); at most 20 patterns,
  `example.com` (exact) or `*.example.com` (subdomains).
* Hooks see proxy requests of HTTP/1.1 clients only, **only while a scope is
  defined and only for in-scope requests**, after Match & Replace and before
  Intercept. WebSocket handshakes, binary requests and requests over 1 MiB
  are not shown to hooks.
* Answer within **2 seconds**. Without an answer (or with one that cannot be
  used: a request or response that does not parse, an unknown action) the
  request goes on unchanged — hooks fail open.
* With several plugins, they are asked in plugin id order; a `replace` is
  what the next one sees, and the first `block` or `respond` ends it.
* `replace`: the request line must use a path (`GET /x HTTP/1.1`); line
  breaks in the head become CRLF and Content-Length is set. `block`: the
  client gets a 403 with the reason. `respond`: the client gets the response
  (Content-Length set) and the connection is closed; the request is not sent.
* Every change, block and answer is written in the history entry's notes
  (and blocks and answers in the plugin's log), so the tester can always see
  what a plugin did.

## Signing and distribution (ChosHub)

Sign a plugin so testers can check who made it and that nothing changed:

```bash
chosplugin keygen my-key.pk8                       # keep it private
chosplugin sign path/to/plugin my-key.pk8 "Your name"
chosplugin verify path/to/plugin
```

This writes `plugin.sig`: an Ed25519 signature of the plugin's id, version
and the fingerprint of all its files. In Plugins:
- **Install from folder…** copies a plugin folder (for example from a
  shared catalog directory). It arrives disabled, with nothing granted, and
  never replaces an installed plugin.
- Each plugin shows **Not signed**, **Signed by … key not trusted**,
  **Signed by … (trusted)** or **Signature does not match the files**.
- **Trust this publisher** after comparing the key fingerprint with the one
  the publisher gave you. **Only run plugins signed by a trusted
  publisher** blocks every other plugin.

A signature never grants permissions: you still enable the plugin and
grant each permission.

## HelloPlugin

The example in `examples/plugins/hello-plugin/` (also installed from
**Plugins → Install the HelloPlugin example**) receives traffic, looks for an
`X-Debug-Token` response header and reports an informational finding; with
`project.write` it also highlights the request. Try it with the lab server's
`/debug-token` page.

## Response hooks and HTTP/2 (M31b)

`{"type": "registerHook", "hosts": [...], "responses": true}` also sends
the responses of those requests: a `hook` message that carries a
`response` (the whole response as text, a chunked body in one piece) next
to the `request`. Answer with `hookResult`: `continue`, `replace` (with
`response`) or `block` (the client gets a 502 with your `reason`).
Requests of HTTP/2 clients now reach hooks too, in the text form
`GET /path HTTP/2`; a replacement keeps that form. Permission, scope rule,
the 2 s limit and failing open are the same as for request hooks.
