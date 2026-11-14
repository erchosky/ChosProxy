//! Listener lifecycle and shared proxy state.

use crate::ProxyError;
use crate::auth::AuthGate;
use crate::conn::{self, ConnCtx};
use crate::hold::InterceptQueue;
use crate::match_replace::MatchReplace;
use chosproxy_core::config::{
    ConfigSection, ListenerConfig, MatchReplaceRule, ProxyConfig, TlsConfig,
};
use chosproxy_core::event::ListenerState;
use chosproxy_core::exchange::CapturedExchange;
use chosproxy_core::websocket::WsEvent;
use chosproxy_tls::{Interceptor, UpstreamTls};
use serde::Serialize;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

/// Receives every captured exchange. Implementations must not block: they
/// run on the connection task (queue the work instead).
pub trait CaptureSink: Send + Sync + 'static {
    /// Called once per completed (or failed) exchange.
    fn record(&self, exchange: CapturedExchange);

    /// Called for WebSocket connections and messages, in order.
    fn ws_event(&self, _event: WsEvent) {}
}

/// Observes listener state changes and interception problems.
pub trait ProxyObserver: Send + Sync + 'static {
    /// Called whenever a listener starts, stops or fails.
    fn listener_changed(&self, listener_id: &str, state: &ListenerState);

    /// Called when a client's TLS handshake with the proxy fails — usually
    /// because the browser does not trust the local CA.
    fn tls_client_failure(&self, _host: &str, _error: &str) {}

    /// Called when the Intercept switch or the queue of paused messages
    /// changes, with the number of paused messages.
    fn intercept_changed(&self, _pending: usize) {}

    /// A client sent wrong proxy credentials.
    fn auth_failure(&self, _listener_id: &str, _peer: SocketAddr) {}
}

/// What the proxy needs to intercept HTTPS.
#[derive(Clone, Debug)]
pub struct TlsContext {
    /// Issues per-host certificates signed by the local CA.
    pub interceptor: Arc<Interceptor>,
    /// Client configuration for upstream TLS.
    pub upstream: UpstreamTls,
}

/// Limits applied to captured messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    /// Maximum stored bytes per request or response (head + body).
    pub max_message_bytes: usize,
}

/// Live proxy counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProxyStats {
    /// Client connections currently open.
    #[ts(type = "number")]
    pub active_connections: u64,
    /// Client connections accepted since start.
    #[ts(type = "number")]
    pub total_connections: u64,
    /// Exchanges handed to the capture sink.
    #[ts(type = "number")]
    pub exchanges: u64,
    /// CONNECT tunnels relayed without decryption.
    #[ts(type = "number")]
    pub tunnels: u64,
    /// CONNECT tunnels decrypted with the local CA.
    #[ts(type = "number")]
    pub intercepted: u64,
    /// Client TLS handshakes that failed (typically: CA not trusted).
    #[ts(type = "number")]
    pub tls_client_failures: u64,
    /// Requests rejected for wrong proxy credentials.
    #[ts(type = "number")]
    pub auth_failures: u64,
    /// Exchanges or tunnels that failed (upstream unreachable, timeouts, …).
    #[ts(type = "number")]
    pub errors: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) active: AtomicU64,
    pub(crate) total: AtomicU64,
    pub(crate) exchanges: AtomicU64,
    pub(crate) tunnels: AtomicU64,
    pub(crate) intercepted: AtomicU64,
    pub(crate) tls_client_failures: AtomicU64,
    pub(crate) auth_failures: AtomicU64,
    pub(crate) errors: AtomicU64,
}

/// State shared by all listeners and connections.
pub(crate) struct Shared {
    sink: Arc<dyn CaptureSink>,
    observer: Arc<dyn ProxyObserver>,
    limits: RwLock<CaptureLimits>,
    tls: RwLock<Option<TlsContext>>,
    /// Listeners that present their own certificate (M31b), by id.
    listener_tls: RwLock<std::collections::HashMap<String, Arc<Interceptor>>>,
    policy: RwLock<Arc<TlsConfig>>,
    intercept: Arc<InterceptQueue>,
    match_replace: RwLock<Arc<MatchReplace>>,
    request_hook: RwLock<Option<Arc<dyn crate::RequestHook>>>,
    pub(crate) counters: Counters,
    own_addrs: RwLock<Vec<SocketAddr>>,
}

fn read_lock<T: Clone>(lock: &RwLock<T>) -> T {
    match lock.read() {
        Ok(g) => g.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn write_lock<T>(lock: &RwLock<T>, value: T) {
    match lock.write() {
        Ok(mut g) => *g = value,
        Err(poisoned) => *poisoned.into_inner() = value,
    }
}

impl Shared {
    pub(crate) fn intercept(&self) -> &InterceptQueue {
        &self.intercept
    }

    pub(crate) fn tls_policy(&self) -> Arc<TlsConfig> {
        read_lock(&self.policy)
    }

    pub(crate) fn match_replace(&self) -> Arc<MatchReplace> {
        read_lock(&self.match_replace)
    }

    pub(crate) fn request_hook(&self) -> Option<Arc<dyn crate::RequestHook>> {
        read_lock(&self.request_hook)
    }

    pub(crate) fn can_intercept(&self) -> bool {
        read_lock(&self.tls).is_some()
    }

    /// The interceptor for a listener: its own, else the shared one.
    /// Without the shared TLS context nothing is intercepted.
    pub(crate) fn interceptor(&self, listener: &str) -> Option<Arc<Interceptor>> {
        let shared = read_lock(&self.tls).map(|t| t.interceptor)?;
        let own = match self.listener_tls.read() {
            Ok(map) => map.get(listener).cloned(),
            Err(poisoned) => poisoned.into_inner().get(listener).cloned(),
        };
        Some(own.unwrap_or(shared))
    }

    pub(crate) fn upstream_tls(&self) -> Option<UpstreamTls> {
        read_lock(&self.tls).map(|t| t.upstream)
    }

    /// The CA certificate a listener's clients should trust, as (DER,
    /// PEM), for the download pages.
    pub(crate) fn ca_certificate(&self, listener: &str) -> Option<(Vec<u8>, String)> {
        self.interceptor(listener).map(|i| {
            let ca = i.authority();
            (ca.cert_der().to_vec(), ca.cert_pem().to_string())
        })
    }

    pub(crate) fn tls_client_failure(&self, host: &str, error: &str) {
        self.counters
            .tls_client_failures
            .fetch_add(1, Ordering::Relaxed);
        tracing::debug!(%host, %error, "client TLS handshake failed");
        self.observer.tls_client_failure(host, error);
    }

    pub(crate) fn auth_failure(&self, listener_id: &str, peer: SocketAddr) {
        self.counters.auth_failures.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(listener = %listener_id, %peer, "proxy authentication failed");
        self.observer.auth_failure(listener_id, peer);
    }

    pub(crate) fn ws_event(&self, event: WsEvent) {
        self.sink.ws_event(event);
    }

    pub(crate) fn limits(&self) -> CaptureLimits {
        match self.limits.read() {
            Ok(l) => *l,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    pub(crate) fn record(&self, ex: CapturedExchange) {
        self.counters.exchanges.fetch_add(1, Ordering::Relaxed);
        self.sink.record(ex);
    }

    /// True if `host:port` addresses one of our own listeners. Serving such
    /// requests locally (instead of relaying) prevents proxy loops.
    pub(crate) fn is_self(&self, host: &str, port: u16) -> bool {
        let addrs = match self.own_addrs.read() {
            Ok(a) => a,
            Err(poisoned) => poisoned.into_inner(),
        };
        let target_ip = host.parse::<IpAddr>().ok();
        addrs.iter().filter(|a| a.port() == port).any(|a| {
            let ours = a.ip();
            match target_ip {
                Some(ip) => {
                    ip == ours
                        || (is_loopback(ip) && (is_loopback(ours) || ours.is_unspecified()))
                        || (ip.is_unspecified() && ours.is_unspecified())
                }
                None => {
                    host.eq_ignore_ascii_case("localhost")
                        && (is_loopback(ours) || ours.is_unspecified())
                }
            }
        })
    }
}

/// Loopback check that also recognises IPv4-mapped IPv6 loopback addresses.
pub(crate) fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map_or(v6.is_loopback(), |v4| v4.is_loopback()),
    }
}

struct Running {
    addr: SocketAddr,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

/// Owns the proxy listeners.
pub struct ProxyService {
    shared: Arc<Shared>,
    listeners: tokio::sync::Mutex<HashMap<String, Running>>,
}

impl std::fmt::Debug for ProxyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyService").finish_non_exhaustive()
    }
}

impl ProxyService {
    /// Creates a service with no running listeners.
    pub fn new(
        sink: Arc<dyn CaptureSink>,
        observer: Arc<dyn ProxyObserver>,
        limits: CaptureLimits,
    ) -> ProxyService {
        let notify = observer.clone();
        let intercept = Arc::new(InterceptQueue::new(Box::new(move |n| {
            notify.intercept_changed(n);
        })));
        ProxyService {
            shared: Arc::new(Shared {
                sink,
                observer,
                intercept,
                match_replace: RwLock::new(Arc::new(MatchReplace::default())),
                request_hook: RwLock::new(None),
                limits: RwLock::new(limits),
                tls: RwLock::new(None),
                listener_tls: RwLock::new(std::collections::HashMap::new()),
                policy: RwLock::new(Arc::new(TlsConfig::default())),
                counters: Counters::default(),
                own_addrs: RwLock::new(Vec::new()),
            }),
            listeners: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Enables HTTPS interception with `ctx`, or disables it (`None`: every
    /// CONNECT becomes an opaque tunnel). Applies to new connections.
    /// Gives a listener its own interceptor (its own CA, or a chosen
    /// certificate), or takes it away (`None`: the shared one). Applies to
    /// connections accepted from now on.
    /// Gives a listener its own interceptor (its own CA, or a chosen
    /// certificate), or takes it away (`None`: the shared one). Applies to
    /// connections accepted from now on.
    pub fn set_listener_tls(&self, listener: &str, interceptor: Option<Arc<Interceptor>>) {
        let mut map = match self.shared.listener_tls.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match interceptor {
            Some(i) => {
                map.insert(listener.to_string(), i);
            }
            None => {
                map.remove(listener);
            }
        }
    }

    pub fn set_tls(&self, ctx: Option<TlsContext>) {
        write_lock(&self.shared.tls, ctx);
    }

    /// The Intercept switch, rules and queue of paused messages.
    pub fn intercept(&self) -> Arc<InterceptQueue> {
        self.shared.intercept.clone()
    }

    /// Updates the interception policy (passthrough and verification lists).
    pub fn set_tls_policy(&self, policy: TlsConfig) {
        write_lock(&self.shared.policy, Arc::new(policy));
    }

    /// Sets (or removes) the hook that sees requests after Match & Replace
    /// and before Intercept (HTTP/1 clients).
    pub fn set_request_hook(&self, hook: Option<Arc<dyn crate::RequestHook>>) {
        write_lock(&self.shared.request_hook, hook);
    }

    /// Replaces the Match & Replace rules (new messages use them).
    pub fn set_match_replace(&self, rules: &[MatchReplaceRule]) -> Result<(), String> {
        self.set_match_replace_with(rules, &|_| None)
    }

    /// The same, resolving script rules through `script` (rewrite scripts
    /// by id; `None` leaves the rule out).
    pub fn set_match_replace_with(
        &self,
        rules: &[MatchReplaceRule],
        script: &dyn Fn(&str) -> Option<Arc<chosproxy_script::Script>>,
    ) -> Result<(), String> {
        let compiled = MatchReplace::with_scripts(rules, script)?;
        write_lock(&self.shared.match_replace, Arc::new(compiled));
        Ok(())
    }

    /// The current interception policy; other senders (Repeater) follow its
    /// certificate verification exceptions too.
    pub fn tls_policy(&self) -> Arc<TlsConfig> {
        self.shared.tls_policy()
    }

    /// Client TLS configurations for upstream connections, when the local CA
    /// is loaded.
    pub fn upstream_tls(&self) -> Option<UpstreamTls> {
        self.shared.upstream_tls()
    }

    /// Updates capture limits for new exchanges.
    pub fn set_limits(&self, limits: CaptureLimits) {
        match self.shared.limits.write() {
            Ok(mut l) => *l = limits,
            Err(poisoned) => *poisoned.into_inner() = limits,
        }
    }

    /// Current counters.
    pub fn stats(&self) -> ProxyStats {
        let c = &self.shared.counters;
        ProxyStats {
            active_connections: c.active.load(Ordering::Relaxed),
            total_connections: c.total.load(Ordering::Relaxed),
            exchanges: c.exchanges.load(Ordering::Relaxed),
            tunnels: c.tunnels.load(Ordering::Relaxed),
            intercepted: c.intercepted.load(Ordering::Relaxed),
            tls_client_failures: c.tls_client_failures.load(Ordering::Relaxed),
            auth_failures: c.auth_failures.load(Ordering::Relaxed),
            errors: c.errors.load(Ordering::Relaxed),
        }
    }

    /// Ids and bound addresses of running listeners.
    pub async fn running(&self) -> Vec<(String, SocketAddr)> {
        self.listeners
            .lock()
            .await
            .iter()
            .map(|(id, r)| (id.clone(), r.addr))
            .collect()
    }

    /// Starts a listener and returns its bound address.
    pub async fn start(&self, cfg: &ListenerConfig) -> Result<SocketAddr, ProxyError> {
        ProxyConfig {
            version: 1,
            listeners: vec![cfg.clone()],
            ..ProxyConfig::default()
        }
        .validate()
        .map_err(ProxyError::InvalidConfig)?;

        let mut listeners = self.listeners.lock().await;
        if listeners.contains_key(&cfg.id) {
            return Err(ProxyError::AlreadyRunning(cfg.id.clone()));
        }
        let requested = cfg.socket_addr();
        let listener = match TcpListener::bind(requested).await {
            Ok(l) => l,
            Err(source) => {
                let err = ProxyError::Bind {
                    addr: requested,
                    source,
                };
                self.shared.observer.listener_changed(
                    &cfg.id,
                    &ListenerState::Failed {
                        error: err.to_string(),
                    },
                );
                return Err(err);
            }
        };
        let addr = listener.local_addr().map_err(|source| ProxyError::Bind {
            addr: requested,
            source,
        })?;
        if let Ok(mut own) = self.shared.own_addrs.write() {
            own.push(addr);
        }
        let cancel = CancellationToken::new();
        // Every connection starts from this context (its peer filled in).
        let template = ConnCtx {
            shared: self.shared.clone(),
            listener_id: cfg.id.as_str().into(),
            peer: addr,
            auth: cfg.auth.clone().map(|a| Arc::new(AuthGate::new(a))),
            invisible: cfg.invisible.then_some(cfg.invisible_tls_port),
            http2: cfg.http2,
            redirect: cfg.redirect_target(),
        };
        let task = tokio::spawn(accept_loop(
            listener,
            template,
            cfg.allow_remote,
            cancel.clone(),
        ));
        listeners.insert(cfg.id.clone(), Running { addr, cancel, task });
        tracing::info!(listener = %cfg.id, %addr, "proxy listener started");
        self.shared.observer.listener_changed(
            &cfg.id,
            &ListenerState::Running {
                address: addr.to_string(),
            },
        );
        Ok(addr)
    }

    /// Stops a listener and closes its connections. Returns false if it was
    /// not running.
    pub async fn stop(&self, id: &str) -> bool {
        let Some(running) = self.listeners.lock().await.remove(id) else {
            return false;
        };
        running.cancel.cancel();
        if tokio::time::timeout(Duration::from_secs(2), running.task)
            .await
            .is_err()
        {
            tracing::warn!(listener = %id, "listener did not stop within 2s");
        }
        if let Ok(mut own) = self.shared.own_addrs.write() {
            own.retain(|a| *a != running.addr);
        }
        tracing::info!(listener = %id, "proxy listener stopped");
        self.shared
            .observer
            .listener_changed(id, &ListenerState::Stopped);
        true
    }

    /// Stops every listener.
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self.listeners.lock().await.keys().cloned().collect();
        for id in ids {
            self.stop(&id).await;
        }
    }
}

async fn accept_loop(
    listener: TcpListener,
    template: ConnCtx,
    allow_remote: bool,
    cancel: CancellationToken,
) {
    loop {
        let accepted = tokio::select! {
            _ = cancel.cancelled() => break,
            a = listener.accept() => a,
        };
        let (stream, peer) = match accepted {
            Ok(x) => x,
            Err(e) => {
                // Typically EMFILE; back off instead of spinning.
                tracing::warn!(error = %e, "accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if !allow_remote && !is_loopback(peer.ip()) {
            tracing::warn!(%peer, "rejected non-loopback client (remote connections disabled)");
            continue;
        }
        template
            .shared
            .counters
            .total
            .fetch_add(1, Ordering::Relaxed);
        let ctx = ConnCtx {
            peer,
            ..template.clone()
        };
        tokio::spawn(conn::serve(ctx, stream, cancel.child_token()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nop;
    impl CaptureSink for Nop {
        fn record(&self, _: CapturedExchange) {}
    }
    impl ProxyObserver for Nop {
        fn listener_changed(&self, _: &str, _: &ListenerState) {}
    }

    fn shared_with(addrs: &[&str]) -> Shared {
        Shared {
            sink: Arc::new(Nop),
            observer: Arc::new(Nop),
            limits: RwLock::new(CaptureLimits {
                max_message_bytes: 1024,
            }),
            tls: RwLock::new(None),
            listener_tls: RwLock::new(std::collections::HashMap::new()),
            policy: RwLock::new(Arc::new(TlsConfig::default())),
            intercept: Arc::new(InterceptQueue::new(Box::new(|_| {}))),
            match_replace: RwLock::new(Arc::new(MatchReplace::default())),
            request_hook: RwLock::new(None),
            counters: Counters::default(),
            own_addrs: RwLock::new(addrs.iter().map(|a| a.parse().unwrap()).collect()),
        }
    }

    #[test]
    fn self_detection() {
        let s = shared_with(&["127.0.0.1:8080"]);
        assert!(s.is_self("127.0.0.1", 8080));
        assert!(s.is_self("localhost", 8080));
        assert!(s.is_self("127.0.0.2", 8080));
        assert!(!s.is_self("127.0.0.1", 8081));
        assert!(!s.is_self("example.com", 8080));
        let any = shared_with(&["0.0.0.0:9000"]);
        assert!(any.is_self("::1", 9000));
        assert!(any.is_self("0.0.0.0", 9000));
    }

    #[test]
    fn mapped_loopback() {
        assert!(is_loopback("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_loopback("::ffff:10.0.0.1".parse().unwrap()));
    }

    #[tokio::test]
    async fn start_stop_and_conflicts() {
        let svc = ProxyService::new(
            Arc::new(Nop),
            Arc::new(Nop),
            CaptureLimits {
                max_message_bytes: 1024,
            },
        );
        let cfg = ListenerConfig {
            port: 0,
            ..ListenerConfig::default()
        };
        let addr = svc.start(&cfg).await.unwrap();
        assert_ne!(addr.port(), 0);
        assert!(matches!(
            svc.start(&cfg).await,
            Err(ProxyError::AlreadyRunning(_))
        ));
        // Same port under another id fails to bind.
        let clash = ListenerConfig {
            id: "other".into(),
            port: addr.port(),
            ..ListenerConfig::default()
        };
        assert!(matches!(
            svc.start(&clash).await,
            Err(ProxyError::Bind { .. })
        ));
        assert!(svc.stop("default").await);
        assert!(!svc.stop("default").await);
        // Port is free again.
        assert!(svc.start(&clash).await.is_ok());
        svc.stop_all().await;
        assert!(svc.running().await.is_empty());
    }

    #[tokio::test]
    async fn rejects_exposed_listener_without_opt_in() {
        let svc = ProxyService::new(
            Arc::new(Nop),
            Arc::new(Nop),
            CaptureLimits {
                max_message_bytes: 1024,
            },
        );
        let cfg = ListenerConfig {
            bind_address: "0.0.0.0".parse().unwrap(),
            port: 0,
            ..ListenerConfig::default()
        };
        assert!(matches!(
            svc.start(&cfg).await,
            Err(ProxyError::InvalidConfig(_))
        ));
    }
}
