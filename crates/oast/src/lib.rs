//! ChosOAST (spec addendum §68, ADR-0026): out-of-band interaction testing.
//!
//! A tester puts a unique identifier (as `<id>.<zone>` or
//! `http://<server>/<id>`) into a request. If the target later looks up
//! that name or fetches that URL, for example because it processes the
//! value on the server side, the interaction server records it and
//! ChosProxy links it back to the request that carried the identifier.
//!
//! * [`id`]: identifiers with a checksum, found in any text.
//! * [`dns`]: the minimal authoritative DNS used to observe lookups.
//! * [`server`]: DNS, HTTP and HTTPS listeners recording interactions.
//! * [`api`]: the token-protected, certificate-pinned polling API of a
//!   self-hosted server, and its client.
//! * [`cert`]: the self-signed certificate of a self-hosted server.
//!
//! The server only observes: it never executes or reflects what it
//! receives, never recurses DNS, and answers only for its own zone.

pub mod api;
pub mod cert;
pub mod dns;
pub mod id;
pub mod server;

pub use id::{extract_ids, is_valid, new_id};
pub use server::{Interaction, OastServer, Protocol, ServerConfig};
