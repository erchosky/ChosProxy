# Automation API and `chosproxy-cli`

ChosProxy can be driven by scripts through a small JSON API on
`127.0.0.1`. The API is used:

* by the desktop app, when you turn it on;
* by `chosproxy-cli serve`, which runs ChosProxy without a window;
* by the `chosproxy-cli` client commands.

The API can do what the window can do, with the same limits:

* scope checks;
* safe defaults for crawls, scans and API sends;
* optional Active Audit in `POST /v1/scan` (`config.active`) only when
  `activeAuthorized: true`, non-empty project scope, and bounded budgets are
  provided; no persistence, secret extraction, evasion, or load tests;
* read-only projects reject mutating routes cleanly.

Only drive systems you own or are authorized to test.

## Turning it on

* **Desktop app:** go to Settings → Automation API, turn on "Listen on
  127.0.0.1", and choose a port (8790 by default). The setting is kept in
  `api.toml`.
* **Without a window:**

  ```bash
  chosproxy-cli serve                      # temporary project
  chosproxy-cli serve --project lab.chosproxy
  chosproxy-cli serve --new engagement.chosproxy --port 8791
  ```

  The first line printed is `API listening on http://127.0.0.1:PORT`.
  Proxy listeners set to start with a project start too. The command runs
  until Ctrl-C.

## The token

Every call needs `Authorization: Bearer <token>`.

* **What it is:** 32 random bytes in hex.
* **Where it is:** the file `api.token` in the data directory, a private
  file. The desktop shows it under Settings → Automation API and can make
  a new one. Anyone who can read the file can use the API, just as anyone
  who can read your project files can read the traffic.
* **How clients find it:** `--token`, then `CHOSPROXY_API_TOKEN`, then the
  token file.

## What is refused

| Request | Answer |
|---|---|
| Missing or wrong token | `401` |
| With an `Origin` header (web pages, including through DNS rebinding) | `403` |
| With a `Host` other than `127.0.0.1:PORT`, `localhost:PORT` or `[::1]:PORT` | `403` |
| From another machine (the server binds to loopback only) | no connection |
| Body over 10 MB | `413` |
| No project open (for project calls) | `409` |
| Invalid input, or out of scope | `400`, with `{"error": "…"}` |

Calls that change something are written to the project's audit log as
`api.*`: scope, crawls, scans, listeners and token changes.

## Endpoints (`/v1`)

| Method and path | Body / query | Does |
|---|---|---|
| `GET /v1/status` | | Version, open project, listeners |
| `POST /v1/project/temporary` | | New temporary project |
| `POST /v1/project/open` / `create` | `{"path", "password"?}` | Open or create a project file (`password` for an encrypted one, M31) |
| `POST /v1/project/close` | | Close it |
| `GET /v1/listeners` | | Proxy listeners |
| `POST /v1/listeners/{id}/start` / `stop` | | Start or stop one |
| `GET /v1/history` | `?limit&offset&search&inScope=true` | A page of history |
| `POST /v1/history/query` | a `HistoryQuery` | Same, with every filter |
| `GET /v1/history/{id}` | | One exchange, request and response |
| `GET /v1/findings`, `POST /v1/findings/query` | a `FindingQuery` | Findings |
| `GET /v1/scope`, `PUT /v1/scope` | a `ScopeConfig` | Read or replace the scope |
| `POST /v1/scope/add` | `{"url", "exclude"?, "path"?}` | Add a URL (or exclude it) |
| `POST /v1/send` | `{"target": {scheme, host, port}, "request": "GET / HTTP/1.1\n…"}` | One request, like Repeater (scope-checked, stored) |
| `POST /v1/crawl` | a `CrawlConfig` (only `startUrls` is needed) | Start a crawl task |
| `GET /v1/scan/configs` | | The scan configuration library |
| `POST /v1/scan` | `{"startUrls", "config"}` | Start a scan task |
| `GET /v1/api-definitions` | | Imported API definitions |
| `POST /v1/api-definitions` | `{"name", "text", "baseUrl"?}` | Import one (OpenAPI, Postman, WSDL) |
| `GET /v1/api-definitions/{id}` | | Its endpoints |
| `GET /v1/tasks`, `GET /v1/tasks/{id}` | | Tasks, or one with its log |
| `POST /v1/tasks/{id}/pause` / `resume` / `cancel` | | Control a task |
| `POST /v1/report` | `{"format": "markdown"/"html"/"json", "title"?, "includeEvidence"?}` | A report of every finding (secrets redacted) |

The JSON shapes are the TypeScript bindings in
`apps/desktop/src/bindings/` (for example `HistoryQuery.ts`,
`CrawlConfig.ts`, `ScanLaunch.ts`).

## `chosproxy-cli` client

```bash
chosproxy-cli status
chosproxy-cli scope add https://staging.example.test/
chosproxy-cli crawl https://staging.example.test/
chosproxy-cli wait 3                       # until task 3 ends
chosproxy-cli scan https://staging.example.test/ --config "Crawl and audit (light)"
chosproxy-cli history --search login --limit 20
chosproxy-cli send "https://staging.example.test/api/me" --file request.txt
chosproxy-cli findings
chosproxy-cli report --format html --output report.html
chosproxy-cli import-api openapi.yaml --base-url https://staging.example.test
```

* **Options:** every client command takes `--port` and `--token`.
* **Exit code:** non-zero when a call fails, and when `wait` sees the task
  fail.

### With curl

```bash
export CHOSPROXY_API_TOKEN=$(cat "$HOME/.local/share/chosproxy/api.token")
curl -s -H "Authorization: Bearer $CHOSPROXY_API_TOKEN" http://127.0.0.1:8790/v1/status
```

## Description and events (M31b)

- `GET /v1/openapi.json` returns an OpenAPI 3.1 description of every route.
  It is generated from the same table the tests check against the router.
  `chosproxy-cli openapi` prints it.
- `GET /v1/events` is a Server-Sent Events stream of engine events: each
  event is `event: <type>` and `data: <JSON>`. It needs the token like
  every route. At most 8 streams are open at once; a client that stops
  reading is disconnected; `event: lagged` says events were missed.
  `chosproxy-cli events` prints one JSON object per line until Ctrl-C.
