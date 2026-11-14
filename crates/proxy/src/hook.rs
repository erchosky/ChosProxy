//! Request hooks (M26, matrix EXT-002): something outside the proxy — in
//! ChosProxy, plugins granted `proxy.intercept` — sees a request after
//! Match & Replace and before Intercept, and lets it go on, changes it,
//! blocks it or answers it. The proxy only asks; the hook decides quickly
//! and must fail open (a hook that cannot decide says [`HookVerdict::Continue`]).

use std::future::Future;
use std::pin::Pin;

/// Where a request is going, for a hook to decide whether it cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookTarget<'a> {
    /// `http` or `https`.
    pub scheme: &'a str,
    /// Host.
    pub host: &'a str,
    /// Port.
    pub port: u16,
    /// Request-line path with query.
    pub path: &'a str,
}

/// What to do with the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookVerdict {
    /// Send it as it is.
    Continue,
    /// Send these bytes instead (a whole HTTP/1.1 request).
    Replace(Vec<u8>),
    /// Do not send it; the client gets a 403 saying why.
    Block(String),
    /// Do not send it; the client gets this response (a whole HTTP/1.1
    /// response; the connection is closed after it).
    Respond(Vec<u8>),
}

/// A hook's answer, with what to note in the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookOutcome {
    /// The decision.
    pub verdict: HookVerdict,
    /// Notes for the history entry (who decided what, timeouts…).
    pub notes: Vec<String>,
}

impl HookOutcome {
    /// Go on, with nothing to note.
    pub fn pass() -> HookOutcome {
        HookOutcome {
            verdict: HookVerdict::Continue,
            notes: Vec::new(),
        }
    }
}

/// A boxed hook answer.
pub type HookFuture = Pin<Box<dyn Future<Output = HookOutcome> + Send>>;

/// Looks at requests passing through the proxy.
pub trait RequestHook: Send + Sync + 'static {
    /// Cheap check: does any hook want requests to this target? Only
    /// then is the request read whole and passed to [`Self::on_request`].
    fn wants(&self, target: &HookTarget<'_>) -> bool;

    /// Decides about one complete request (bounded in time by the hook).
    /// A request from an HTTP/2 client comes in the text form
    /// (`GET / HTTP/2`), and a replacement is read the same way.
    fn on_request(&self, target: &HookTarget<'_>, request: Vec<u8>) -> HookFuture;

    /// Cheap check: does any hook want the responses from this target?
    /// Only then is a response read whole (up to the hold limit) and
    /// passed to [`Self::on_response`].
    fn wants_response(&self, _target: &HookTarget<'_>) -> bool {
        false
    }

    /// Decides about one complete response (M31b). `Continue` delivers it
    /// as it is, `Replace` and `Respond` deliver those bytes instead (a
    /// whole HTTP/1.1 response), `Block` gives the client a 502 saying why.
    fn on_response(
        &self,
        _target: &HookTarget<'_>,
        _request: Vec<u8>,
        _response: Vec<u8>,
    ) -> HookFuture {
        Box::pin(async { HookOutcome::pass() })
    }
}
