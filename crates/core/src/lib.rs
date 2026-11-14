//! Shared foundations for ChosProxy.
//!
//! This crate has no networking and no UI code. It defines the vocabulary the
//! other crates share: branding, configuration sections, filesystem paths,
//! engine events, traffic sources and redaction helpers.

pub mod brand;
pub mod config;
pub mod event;
pub mod exchange;
pub mod fs;
pub mod paths;
pub mod redact;
pub mod scope;
pub mod source;
pub mod time;
pub mod websocket;

pub use brand::{Brand, brand};
pub use scope::{Scope, ScopeConfig, ScopeDecision, ScopeTarget};
pub use source::Source;
