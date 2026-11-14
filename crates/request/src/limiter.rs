//! Resource-pool limiter: how many requests an automated task (or a pool
//! of tools) may have in flight, how many it may start per second, an
//! optional random delay before each request, and backing off when a
//! server asks for it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Counts a waiter until it is dropped (also when the wait is cancelled).
struct Waiting<'a>(&'a AtomicU64);

impl<'a> Waiting<'a> {
    fn new(counter: &'a AtomicU64) -> Waiting<'a> {
        counter.fetch_add(1, Ordering::Relaxed);
        Waiting(counter)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Limits concurrency and request rate for one task (or a pool of tasks).
#[derive(Debug, Clone)]
pub struct Limiter {
    slots: Arc<Semaphore>,
    concurrency: usize,
    interval: Option<Duration>,
    random_delay: Duration,
    /// When the next request may start by the rate limit (a reservation;
    /// the lock is never held across a wait).
    next: Arc<Mutex<Instant>>,
    /// No request starts before this (back-off asked by a server).
    paused_until: Arc<Mutex<Instant>>,
    waiting: Arc<AtomicU64>,
}

/// A random duration in `0..=max` (no delay when the system random source
/// fails, which it does not in practice).
fn random_up_to(max: Duration) -> Duration {
    use ring::rand::SecureRandom;
    let millis = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
    if millis == 0 {
        return Duration::ZERO;
    }
    let mut bytes = [0u8; 8];
    if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
        return Duration::ZERO;
    }
    Duration::from_millis(u64::from_le_bytes(bytes) % (millis + 1))
}

impl Limiter {
    /// At most `concurrency` requests at once (at least 1) and
    /// `per_second` starts per second (0 = no rate limit).
    pub fn new(concurrency: usize, per_second: u32) -> Limiter {
        Limiter::with_delay(concurrency, per_second, Duration::ZERO)
    }

    /// The same, with a random extra delay of up to `random_delay` before
    /// each request starts.
    pub fn with_delay(concurrency: usize, per_second: u32, random_delay: Duration) -> Limiter {
        let concurrency = concurrency.max(1);
        Limiter {
            slots: Arc::new(Semaphore::new(concurrency)),
            concurrency,
            interval: (per_second > 0).then(|| Duration::from_secs(1) / per_second),
            random_delay,
            next: Arc::new(Mutex::new(Instant::now())),
            paused_until: Arc::new(Mutex::new(Instant::now())),
            waiting: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Waits for a free slot, the rate limit, any back-off and the random
    /// delay; the request may start once this returns and holds the slot
    /// until the permit is dropped. (`None` only if the limiter were
    /// closed, which it never is.)
    pub async fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        let _waiting = Waiting::new(&self.waiting);
        let permit = self.slots.clone().acquire_owned().await.ok();
        // Reserve a start time by the rate limit, then wait outside the lock.
        let start = {
            let mut next = lock(&self.next);
            let start = (*next).max(Instant::now());
            *next = match self.interval {
                Some(interval) => start + interval,
                None => start,
            };
            start
        };
        tokio::time::sleep_until(start).await;
        // A back-off asked while waiting applies too.
        loop {
            let until = *lock(&self.paused_until);
            if until <= Instant::now() {
                break;
            }
            tokio::time::sleep_until(until).await;
        }
        let extra = random_up_to(self.random_delay);
        if !extra.is_zero() {
            tokio::time::sleep(extra).await;
        }
        permit
    }

    /// No request starts for `pause` from now (requests in flight go on).
    pub async fn back_off(&self, pause: Duration) {
        let mut until = lock(&self.paused_until);
        *until = (*until).max(Instant::now() + pause);
    }

    /// How long until requests may start again after a back-off, if they
    /// are paused.
    pub async fn paused_for(&self) -> Option<Duration> {
        let until = *lock(&self.paused_until);
        until
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
    }

    /// Requests holding a slot now.
    pub fn in_flight(&self) -> usize {
        self.concurrency
            .saturating_sub(self.slots.available_permits())
    }

    /// Requests waiting for a slot, the rate or a delay.
    pub fn waiting(&self) -> u64 {
        self.waiting.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use super::*;

    #[tokio::test]
    async fn spaces_requests_by_the_rate() {
        let limiter = Limiter::new(10, 50);
        let start = Instant::now();
        for _ in 0..6 {
            drop(limiter.acquire().await);
        }
        // Five intervals of 20 ms between six starts.
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(95), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn bounds_concurrency() {
        let limiter = Limiter::new(2, 0);
        let a = limiter.acquire().await;
        let _b = limiter.acquire().await;
        let third = tokio::time::timeout(Duration::from_millis(50), limiter.acquire()).await;
        assert!(third.is_err(), "a third request waits");
        drop(a);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), limiter.acquire())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn backs_off_and_counts() {
        let limiter = Limiter::new(3, 0);
        limiter.back_off(Duration::from_millis(80)).await;
        assert!(limiter.paused_for().await.is_some());
        let start = Instant::now();
        let permit = limiter.acquire().await;
        assert!(start.elapsed() >= Duration::from_millis(75));
        assert_eq!(limiter.in_flight(), 1);
        assert_eq!(limiter.waiting(), 0);
        drop(permit);
        assert_eq!(limiter.in_flight(), 0);
    }

    #[tokio::test]
    async fn random_delay_stays_within_its_bound() {
        for _ in 0..50 {
            assert!(random_up_to(Duration::from_millis(5)) <= Duration::from_millis(5));
        }
        assert_eq!(random_up_to(Duration::ZERO), Duration::ZERO);
        let limiter = Limiter::with_delay(1, 0, Duration::from_millis(20));
        let start = Instant::now();
        drop(limiter.acquire().await);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn status_never_waits_for_a_paused_request_and_cancelled_waits_are_not_counted() {
        let limiter = Limiter::new(2, 0);
        limiter.back_off(Duration::from_secs(30)).await;
        let waiter = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire().await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(limiter.waiting(), 1);
        // Status and further back-offs answer at once while a request waits.
        let quick = tokio::time::timeout(Duration::from_millis(200), async {
            limiter.back_off(Duration::from_secs(1)).await;
            limiter.paused_for().await
        })
        .await
        .unwrap();
        assert!(
            quick.unwrap() > Duration::from_secs(20),
            "the longer pause is kept"
        );
        waiter.abort();
        let _ = waiter.await;
        assert_eq!(limiter.waiting(), 0, "a cancelled wait is not counted");
        assert_eq!(limiter.in_flight(), 0);
    }

    #[tokio::test]
    async fn a_back_off_delays_requests_that_already_reserved_a_start() {
        let limiter = Limiter::new(1, 0);
        let start = Instant::now();
        let waiter = {
            let limiter = limiter.clone();
            tokio::spawn(async move {
                let held = limiter.acquire().await;
                tokio::time::sleep(Duration::from_millis(30)).await;
                drop(held);
                limiter.acquire().await.is_some()
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        limiter.back_off(Duration::from_millis(120)).await;
        assert!(waiter.await.unwrap());
        assert!(start.elapsed() >= Duration::from_millis(115));
    }
}
