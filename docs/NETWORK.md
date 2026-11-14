# Network settings, invisible listeners and Match & Replace

Only send traffic to systems you own or are authorized to test. Upstream
proxies and host name overrides change where requests go, so check them
before a session.

## Settings → Network

Every outgoing connection follows these settings: the proxy, Repeater, the
Fuzzer, the Sequencer, the crawler and content discovery
([ADR-0029](adr/0029-network-and-match-replace.md)). They are stored in
`network.toml` in the configuration folder.

### Upstream proxies
- Rules are checked in order. The first enabled rule whose host matches is
  used. Host patterns are `*` (everything), `example.com` or
  `*.example.com` (subdomains only).
- **HTTP proxy:** HTTPS and other tunnels use `CONNECT`. Plain HTTP is sent
  in absolute form (`GET http://host/path`). HTTP history still shows the
  request as your client sent it, with a note saying which proxy carried
  it.
- **SOCKS5 proxy:** with or without a user name and password. "DNS on the
  proxy" lets the proxy resolve names (needed when only it can reach the
  target's DNS).
- **Direct:** exempts hosts from a later catch-all rule. For example, put
  `intranet.test → Direct` before `* → HTTP proxy`.
- Credentials are kept in `network.toml`, which only your user can read.
  They are never logged and never included in a settings export. NTLM and
  Digest proxy authentication arrive with the credential keyring (M24).

### Host name overrides
These resolve a name (exact or `*.domain`) to a fixed IPv4 or IPv6
address, for labs without DNS. They also apply when a request goes through
an upstream proxy.

### Timeouts
- **Connect:** DNS, TCP and the proxy handshake.
- **Response:** waiting for the response head.
- **Idle:** silence while a body is transferring.

The Fuzzer, Sequencer, crawler and discovery tasks keep their own per-task
timeout.

## Invisible listeners

Turn on **Invisible** for a listener in Proxy → Listeners when a client
cannot be configured to use a proxy (thick clients, devices):

1. Point the client's host names at the proxy, for example with a
   hosts-file entry such as `127.0.0.1 api.lab.test`. The listener must
   listen on the port the client uses (often 80 or 443); ports under 1024
   need privileges.
2. In Settings → Network → Host name overrides, give ChosProxy the real
   address of the same name. Otherwise ChosProxy would connect to itself;
   it refuses such requests with "508 Loop Detected".
3. **Plain HTTP** goes where its Host header says. **TLS** goes where its
   server name (SNI) says, on the listener's "TLS port" (443 by default).
   It is intercepted with the local CA unless the host is in the
   passthrough list, and the client must trust the CA.

An invisible listener cannot also ask for proxy credentials.

## Match & Replace

Open it from Intercept → Match & Replace. Rules run in order on every
proxied message, before Intercept pauses it:

| Where | Works on |
|---|---|
| Request line / Status line | The first line |
| Request header / Response header | Each header line on its own. An empty "Find" adds "Replace with" as a new header; a result that is empty removes the line. |
| Request body / Response body | The body. Compressed or chunked bodies are decoded first and sent with a new `Content-Length`. Bodies larger than the Intercept hold limit pass unchanged, with a note. |

With **Regex**, "Find" is a regular expression and `$1` / `${name}` insert
groups. Without it, both texts are literal.

Changed exchanges are marked as edited in HTTP history. The original
message is kept, and a note names the rules that changed it. Rules apply to
proxy traffic only. Repeater and the tools send exactly what you give them.

## NTLM, listener certificates and the keyring (M31b)

- **NTLMv2**: Settings → Network → platform authentication → scheme
  NTLMv2. The user name may be `DOMAIN\user`. It answers a host's
  `401 NTLM` offer for tool sends; it is not used on kept Repeater
  connections or HTTP/2. Kerberos is not supported.
- **Certificate per listener** (Proxy → Listeners): ChosProxy's CA, a CA
  of its own (download it from `http://chosproxy/cert` through that
  listener), or a certificate file with its key (PEM), presented for every
  host. If the files cannot be read the listener does not start.
- **Keyring protection** (Settings → Credentials): the key that encrypts
  the keyring lives in a private file (default), in the macOS Keychain, or
  behind a master password asked once per session. A forgotten password
  or a deleted keychain item means the saved credentials are lost.
