//! The shared request engine (ADR-0009).
//!
//! Every component that talks to origin servers — the proxy's upstream side,
//! Repeater, and later the fuzzer, crawler, scanner and plugins — uses the
//! connections in this crate, so DNS, TCP, TLS, timeouts and (later)
//! upstream proxies are implemented once.

mod auth;
mod h2;
pub mod io;
mod limiter;
mod network;
mod ntlm;
mod send;
mod upstream;

pub use auth::{PlatformCredential, set_platform_auth};
pub use h2::{H2Connection, MAX_SINGLE_PACKET, send_h2, send_h2_single_packet, translation_notes};
pub use limiter::Limiter;
pub use network::{network, set_network};
pub use send::{
    BodySink, Connection, Destination, ParallelRequest, SendError, SendLimits, SendTiming, Sent,
    request_method, send, send_parallel, send_streaming,
};
pub use upstream::{CONNECT_TIMEOUT, ConnectTiming, Upstream, UpstreamSecurity};
