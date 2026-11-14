# ADR-0011: HTTPS interception with a per-install local CA

* Status: Accepted
* Date: 2026-10-22

## Context
Milestone 2 must let users inspect HTTPS of systems they are authorized to
test, from a lab browser, without weakening the user's normal browsing or
system trust, and without ever shipping or sharing a CA key.

## Decisions

1. **CA per installation**, generated on first start (ECDSA P-256, 5 years,
   `pathlen:0`), stored in `<data dir>/ca/` (dir `0700`, key `0600`). The key
   is never exported, transmitted or logged. A CA that fails validation
   (missing file, key/cert mismatch, expired) disables interception and is
   **not** overwritten; the user regenerates explicitly.
2. **Leaf certificates** per host, 90 days (capped by CA validity), SAN from
   the client's SNI (else the CONNECT host), one in-memory leaf key per
   process, cached. Only HTTP/1.1 is offered via ALPN until HTTP/2 lands
   (ADR-0009).
3. **Decide after CONNECT by the first bytes**: TLS ClientHello → intercept;
   ASCII → plain HTTP inside the tunnel (captured); anything else or silence
   → opaque tunnel. Passthrough hosts and "interception off" are always
   opaque.
4. **Upstream connected lazily** (after the client's first request) so
   connection and certificate errors reach the browser as readable proxy
   error pages.
5. **Upstream certificates are verified** (system roots + Mozilla roots).
   Hosts listed under "skip verification" (default: loopback only, for
   self-signed local labs) are not verified; handshake signatures are still
   checked and every affected exchange carries a note.
6. **Lab browser without touching trust stores**: Chromium-family browsers
   are started with a temporary profile and
   `--ignore-certificate-errors-spki-list=<leaf key pin>`, which only affects
   that profile and only our leaf key. Firefox gets a temporary profile with
   proxy preferences and the CA imported into that profile with NSS
   `certutil` when available. Profiles are deleted on exit; leftovers older
   than 24 h are removed at startup.
7. The CA certificate (public part only) is served by the proxy at
   `http://chosproxy/cert` for manual installation in testing profiles.

## Alternatives considered
* Shared/pre-installed CA: rejected (spec §7; any user could MITM others).
* Installing the CA in the system store automatically: rejected (affects all
  software on the machine; requires root).
* Accepting all upstream certificates by default (common in other tools):
  rejected as "insecure TLS for convenience" (spec §54); loopback default
  covers local labs.
* One certificate key per host: slower, and incompatible with the Chromium
  SPKI pin approach.

## Consequences
* ➕ HTTPS works in the lab browser with zero manual steps on Chromium.
* ➕ No global trust changes; keys never leave the machine.
* ➖ Google Chrome generates background requests to Google services; they
  are captured like any other traffic until scope filtering (M5).
* ➖ Apps with certificate pinning fail unless added to passthrough.
