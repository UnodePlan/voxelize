use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_REQUESTS_PER_WINDOW: u32 = 30;
const DEFAULT_TRACKED_CLIENTS: usize = 4_096;

#[derive(Clone, Debug)]
pub(crate) struct NonceRateLimiter {
    state: Arc<Mutex<LimiterState>>,
    window: Duration,
    requests_per_window: u32,
    tracked_clients: usize,
}

impl Default for NonceRateLimiter {
    fn default() -> Self {
        Self::new(
            DEFAULT_WINDOW,
            DEFAULT_REQUESTS_PER_WINDOW,
            DEFAULT_TRACKED_CLIENTS,
        )
    }
}

impl NonceRateLimiter {
    fn new(window: Duration, requests_per_window: u32, tracked_clients: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(LimiterState {
                buckets: HashMap::new(),
                last_pruned_at: Duration::ZERO,
            })),
            window,
            requests_per_window: requests_per_window.max(1),
            tracked_clients: tracked_clients.max(1),
        }
    }

    pub(crate) fn allow(&self, peer_addr: Option<SocketAddr>, now: Duration) -> bool {
        let key = peer_addr
            .map(|address| ClientKey::Ip(address.ip()))
            .unwrap_or(ClientKey::Unknown);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());

        if let Some(bucket) = state.buckets.get_mut(&key) {
            if now.saturating_sub(bucket.started_at) >= self.window {
                *bucket = Bucket {
                    started_at: now,
                    requests: 1,
                };
                return true;
            }
            if bucket.requests >= self.requests_per_window {
                return false;
            }
            bucket.requests += 1;
            return true;
        }

        // 周期性或容量不足时才扫描全部来源，避免每个匿名请求都产生 O(n) 开销。
        if now.saturating_sub(state.last_pruned_at) >= self.window
            || state.buckets.len() >= self.tracked_clients
        {
            state
                .buckets
                .retain(|_, bucket| now.saturating_sub(bucket.started_at) < self.window);
            state.last_pruned_at = now;
        }
        if state.buckets.len() >= self.tracked_clients {
            return false;
        }
        state.buckets.insert(
            key,
            Bucket {
                started_at: now,
                requests: 1,
            },
        );
        true
    }
}

#[derive(Debug)]
struct LimiterState {
    buckets: HashMap<ClientKey, Bucket>,
    last_pruned_at: Duration,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ClientKey {
    Ip(IpAddr),
    Unknown,
}

#[derive(Clone, Copy, Debug)]
struct Bucket {
    started_at: Duration,
    requests: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_each_client_and_reopens_after_the_window() {
        let limiter = NonceRateLimiter::new(Duration::from_secs(10), 2, 2);
        let client: SocketAddr = "127.0.0.1:4000".parse().unwrap();

        assert!(limiter.allow(Some(client), Duration::ZERO));
        assert!(limiter.allow(Some(client), Duration::ZERO));
        assert!(!limiter.allow(Some(client), Duration::ZERO));
        assert!(limiter.allow(Some(client), Duration::from_secs(10)));
    }

    #[test]
    fn rejects_new_sources_when_the_active_client_bound_is_full() {
        let limiter = NonceRateLimiter::new(Duration::from_secs(10), 1, 1);
        let first = "127.0.0.1:4000".parse().unwrap();
        let second = "127.0.0.2:4000".parse().unwrap();

        assert!(limiter.allow(Some(first), Duration::ZERO));
        assert!(!limiter.allow(Some(second), Duration::ZERO));
        assert!(limiter.allow(Some(second), Duration::from_secs(10)));
    }
}
