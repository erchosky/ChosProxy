//! Proxy listeners and the HTTP/1.1 relay.
//!
//! [`ProxyService`] owns any number of listeners. Each accepted connection is
//! handled by its own task which reads requests with the wire-exact codec,
//! relays them upstream, streams responses back and hands a
//! [`CapturedExchange`](chosproxy_core::exchange::CapturedExchange) to the
//! configured [`CaptureSink`]. The proxy knows nothing about storage or UI.

mod auth;
mod capture;
mod conn;
mod error;
mod h2_relay;
mod hold;
mod hook;
mod intercept;
mod match_replace;
mod relay;
mod responses;
mod service;
mod sni;
mod websocket;

pub use error::ProxyError;
pub use hold::{
    Decision, HeldKind, HeldMessage, HeldSummary, InterceptQueue, MAX_PENDING, MAX_PENDING_BYTES,
};
pub use hook::{HookFuture, HookOutcome, HookTarget, HookVerdict, RequestHook};
pub use match_replace::MatchReplace;
pub use service::{
    CaptureLimits, CaptureSink, ProxyObserver, ProxyService, ProxyStats, TlsContext,
};
