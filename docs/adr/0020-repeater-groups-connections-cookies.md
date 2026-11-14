# ADR-0020: Repeater groups, kept connections and the cookie jar

* Status: Accepted
* Date: 2026-11-10

## Context
ADR-0019 gave Repeater a send path that opens a new connection per send
and left three things for later in Milestone 4: sending several tabs as a
group (in sequence, over one connection, or at the same moment to test race
conditions), reusing a connection between sends, and a cookie jar so that a
session obtained in the browser or by an earlier request can be used without
copying cookies by hand. A shared "send to" context menu was also pending.

## Decisions
1. **Connections are objects of the send path.** `chosproxy-request`
   exposes `Connection` (open, send, `is_reusable`, close); the one-shot
   `send` is open + send + close. A response leaves the connection usable
   only if request and response allow keep-alive and the body was read
   completely by its framing (not close-delimited, not truncated, not
   incomplete, not an upgrade). The first request's timing includes DNS,
   connect and TLS; later ones report 0 for them and are marked `reused`.
2. **Reuse is a per-tab option, off by default.** With *Reuse the
   connection*, the engine keeps one idle connection per tab, keyed by
   target and TLS verification, and checks it is still open before using
   it; otherwise every send opens a new connection, which is the
   predictable default for testing. A failure on a reused connection says
   so; there is no automatic retry (the request may have been processed).
3. **Groups are stored, contiguous and ordered by their tabs.** Schema v9
   adds `repeater_groups` (name, colour, send mode) and `group_id` on tabs.
   Adding tabs to a group moves them next to each other; removing a group
   keeps its tabs. A group sends what is saved, in tab order:
   * **sequence** — one after another, each with its own options
     (kept connection, jar); cookies set by one response are in the jar for
     the next request, so a login followed by an authenticated request
     works;
   * **single connection** — one new connection for all (every tab must
     have the same target); it stops when the server closes it, and the
     remaining tabs are reported as not sent;
   * **parallel** — `send_parallel` opens every connection and writes each
     request except its last byte, then writes all last bytes one right
     after another ("last-byte synchronisation"), so the server completes
     the requests within a fraction of a millisecond. This is the HTTP/1
     technique for race conditions; the HTTP/2 single-packet attack needs
     HTTP/2 (M21).
   Every send is recorded in its tab's history as usual; cancelling a group
   send stops it and keeps what already finished.
4. **One cookie jar per project.** `cookie_jar` (schema v9) stores cookies
   by name, domain and path with the RFC 6265 attributes. It is filled from
   the `Set-Cookie` headers of proxy traffic (written without blocking the
   capture path) and of Repeater responses. Parsing and matching (domain
   and path matching, default path, `Secure`, `Max-Age` over `Expires`,
   expired cookies delete) live in `chosproxy-http::cookies`; without a
   public-suffix list, a `Domain` naming a single label is refused.
5. **The jar changes what is sent, never the editor.** A tab with *Use the
   cookie jar* gets the matching cookies (most specific path first) merged
   into its `Cookie` header when sending: values of cookies already there
   are replaced, missing ones are added, and the header is untouched when
   nothing changes. The send records which cookies came from the jar, and
   the stored exchange shows the request exactly as sent.
6. **In-scope exports drop the jar.** Cookies carry no scheme or port to
   check against the scope and are session secrets, so an in-scope copy of
   a project has an empty jar (and no empty groups).
7. **One context menu for "send to" actions.** A single menu component and
   store serve every view; `exchangeMenu` builds the shared actions for a
   stored request (Repeater, Organizer, finding, Comparer, Decoder, scope,
   copy URL) in HTTP history, the Logger, Search and the Organizer, opened
   by right-click or Shift+F10 / the context-menu key. Repeater tabs and
   groups use the same menu for their own actions.

## Consequences
* ➕ Race conditions, multi-step flows and session reuse can be tested from
  Repeater without copying cookies by hand.
* ➕ The fuzzer and later senders reuse `Connection`, `send_parallel` and
  the jar.
* ➖ No HTTP/2 (single-packet attack), upstream proxies or resource pools
  yet (M21, M26); no session-handling rules or macros (M24).
* ➖ The jar has no public-suffix list; the single-label rule covers the
  common case but not suffixes such as `co.uk`.
