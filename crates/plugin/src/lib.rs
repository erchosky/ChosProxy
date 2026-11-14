//! Plugin APIs v1 and v2 (spec §36–37, ADR-0010, ADR-0025, ADR-0033): the manifest a plugin
//! ships, the permissions it can ask for, and the JSON-lines protocol it
//! speaks with ChosProxy over its standard input and output.
//!
//! A plugin is a separate program. ChosProxy decides what it may do *through
//! ChosProxy* (receive traffic, add findings, annotate history) by checking
//! every message against the permissions the tester granted. It does not
//! sandbox the program itself: a plugin runs with the operating-system
//! rights of the user, so only trusted plugins should be installed.

mod manifest;
mod permissions;
mod protocol;
mod signing;

pub use manifest::{API_VERSION, MANIFEST_FILE, Manifest, SUPPORTED_APIS, hash_dir};
pub use permissions::Permission;
pub use protocol::{
    Exchange, HookRequest, HostMessage, MAX_BODY, MAX_LINE, PluginFinding, PluginMessage, Query,
};
pub use signing::{
    SIG_FILE, SigFile, Verified, generate_key, key_fingerprint, sign_dir, signed_message,
    verify_dir,
};
