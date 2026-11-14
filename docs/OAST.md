# ChosOAST — out-of-band interaction testing

Some problems only show up when the target contacts another system: a URL
parameter the server fetches, an XML document whose external entities are
resolved, a value that ends up in a DNS lookup. ChosOAST gives you unique
addresses to place in requests and tells you when, how and from where
something contacted them, linked to the request that carried them.

Only use it on systems you are authorized to test. The target will contact
a server you control; nothing is sent to ChosProxy's authors.

## How it works

1. **OAST → New payload** creates an identifier such as
   `r27n5qy6b42pmtsyndtw` and copies its URL. In Self-hosted mode (or Local
   with DNS) there is also a DNS name, `<id>.<zone>`.
2. Put the URL or name in a request: Repeater, Fuzzer, the lab browser,
   anywhere. ChosProxy finds the identifier in the stored traffic by itself.
3. When the target looks the name up or fetches the URL, the interaction
   appears in the OAST view with the raw DNS question or HTTP request, and
   an **External service interaction** finding is created for the request
   that carried it.

## Active Audit OAST probes (M13)

When the Active Audit `active.oast-probe` check is explicitly enabled in a
scan, ChosProxy allocates a fresh ChosOAST URL per insertion point (for
example query/body/header values matching URL-like or general parameters) and
polls ChosOAST within a bounded correlation window. An out-of-band finding
is recorded only when ChosOAST reports an interaction whose unique identifier
matches the active probe sent for that point.

If ChosOAST is not enabled or configured in the project, `active.oast-probe`
is skipped cleanly without emitting speculative findings, and never queries
cloud metadata endpoints or third-party destinations.

Requests you send to the OAST server yourself (opening a payload URL) are
not counted as carrying requests, so they never create findings.

## Local mode

The interaction server runs inside ChosProxy while a project is open.

| Setting | Default | Notes |
|---|---|---|
| Listen on | `127.0.0.1` | Use a lab network address (e.g. `192.168.56.1`) if the target runs in a VM; ChosProxy warns that the server is then reachable from that network. |
| HTTP port | `8899` | |
| Address in payloads | `127.0.0.1` | How the target reaches this machine. |
| Answer DNS | off | UDP port `5353`, zone `oast.chosproxy.test`. DNS payloads only work if the lab's resolver forwards that zone to this machine. |

## Self-hosted mode (recommended for real targets)

Run `chosoast` on a machine you control with a public address, and delegate
a DNS zone to it.

1. **DNS.** In your domain's DNS, add (example for `example.com`):

   ```
   oast.example.com.      NS   ns-oast.example.com.
   ns-oast.example.com.   A    203.0.113.5
   ```

2. **Start the server** (ports 53, 80 and 443 need privileges, e.g.
   `sudo setcap 'cap_net_bind_service=+ep' ./chosoast` on Linux):

   ```bash
   ./chosoast --zone oast.example.com --ip 203.0.113.5
   ```

   On first start it creates `chosoast-data/` (mode `0700`) with a
   certificate and a token, and prints:

   ```
   certificate SHA-256: 3f:a1:…
   access token (shown once, stored in chosoast-data/token): 9c41…
   ```

   Options: `--ipv6`, `--dns`, `--http`, `--https`, `--api` (default
   `0.0.0.0:8443`), `--data`, `--keep`, `--no-dns`, `--no-http`,
   `--no-https`. `chosoast --help` lists them.

3. **In ChosProxy → OAST → Settings → Self-hosted**, enter the zone, the
   API host and port, the token and the certificate SHA-256, then **Test
   connection**. ChosProxy polls the server every few seconds over TLS and
   accepts only that certificate.

The server keeps interactions in memory only (100 000 by default) and
answers DNS only for its zone, never recursively, with a per-source rate
limit. One server can serve several projects: each project stores only the
interactions of its own payloads.

Build it from source with `cargo build --release -p chosproxy-oast`
(the binary is `target/release/chosoast`).

## Limits

* HTTPS interactions on a self-hosted server use its self-signed
  certificate; clients that verify certificates abort before sending a
  request. DNS and HTTP are the reliable channels.
* SMTP and other protocols, and external OAST providers through plugins,
  are not available yet (see ADR-0026).

## Mail (SMTP, M31b)

Local mode: turn on **Accept mail (SMTP)** and choose a port (2525 by
default). Self-hosted: start `chosoast` with `--smtp <addr:port>`. Mail
that carries a payload identifier, for example sent to
`<id>@<your zone>`, is recorded as an SMTP interaction with its envelope
and body (up to 64 KB) and can become an "External service interaction
(SMTP)" finding. The listener is a sink: it never delivers or relays
mail, and it offers no AUTH or STARTTLS. Receiving mail from the Internet
needs port 25 and an MX record for the zone, which are yours to set up.
