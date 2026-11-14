# ADR-0033: Plugin API v2 and signed plugins (ChosHub)

* Status: Accepted
* Date: 2026-11-12

## Context
Plugin API v1 (ADR-0025) lets a plugin receive traffic, report findings and
annotate the history. M23 asks for:
* a stable, versioned extension API that can also send requests, read the
  project and extend the UI;
* signed distribution.

Plugins stay separate processes. Permissions limit what they do *through
ChosProxy*, not what the program could do on its own (see the warning in
the Plugins view).

## Decisions
1. **Versioned API.**
   * The manifest's `api` picks 1 or 2. Both run side by side, so v1
     plugins keep working unchanged.
   * The host sends `hello.api` and `CHOSPROXY_PLUGIN_API` with the
     manifest's version.
   * The v2 permissions are only accepted in v2 manifests.
2. **Calls with ids (v2).**
   * `send` (`network.send`) goes through the Request Engine, so network
     settings and HTTP/2 apply.
   * It is refused out of scope when a scope exists, like the Fuzzer.
   * It is limited to 4 requests in flight and 300 per minute.
   * Each send is stored with source `plugin` and a note naming the plugin.
     These exchanges are never fed back to plugins (no loops).
   * `query` (`project.read`) reads one exchange, history rows (at most
     200) or the findings.
   * A call refused for a missing permission still gets a `result` with
     the reason. It is also logged and audited.
3. **Menu actions (`ui.extend`).**
   * A plugin registers up to 20 actions. They appear in every request
     menu while it runs.
   * Running one sends `action` with the exchange and waits up to 30 s for
     `output`, which is shown in the same dialog as action scripts.
   * Each run is audited.
4. **Signatures.**
   * `plugin.sig` holds an Ed25519 signature (ring) by the publisher over
     `id`, `version` and the existing directory fingerprint. The signature
     file itself is excluded from the fingerprint.
   * The Plugins view shows one of four states: unsigned, invalid,
     untrusted (valid, unknown key) or trusted.
   * Trusted keys are kept in `plugins.toml`. "Only run plugins signed by a
     trusted publisher" blocks the rest.
   * Signing never grants anything.
   * The `chosplugin` tool generates keys (the private key is written with
     mode 0600, never overwriting a file), signs and verifies.
5. **ChosHub is local.**
   * "Install from folder" copies a plugin folder from any catalog
     directory. It arrives disabled, refuses links, and never replaces an
     installed plugin.
   * There is no network store. One would need an explicit decision about
     hosting, review and update policy.

## Consequences
* Plugins can automate workflows (send, read, add actions) within explicit
  grants. The engine enforces scope and rate limits.
* Trust is per key and decided by the tester. Key distribution happens out
  of band (compare fingerprints).
* Deferred, explicitly:
  * plugins that block or answer live proxy traffic (EXT-002) → M26, with
    live tasks and resource pools;
  * external OAST providers through plugins → M25;
  * an OS-level sandbox for plugin processes → M31;
  * a hosted marketplace: not planned without a separate decision.
