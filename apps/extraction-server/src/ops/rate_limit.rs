use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_TRACKED_SOURCES: usize = 256;

#[derive(Clone, Debug)]
pub(super) struct OpsRateLimiter {
    state: Arc<Mutex<LimiterState>>,
    window: Duration,
    requests_per_window: u32,
}

impl OpsRateLimiter {
    pub(super) fn per_minute(requests: u32) -> Self {
        Self {
            state: Arc::new(Mutex::new(LimiterState::default())),
            window: Duration::from_secs(60),
            requests_per_window: requests.max(1),
        }
    }

    pub(super) fn allow(&self, peer: Option<SocketAddr>) -> bool {
        let key = peer
            .map(|value| value.ip())
            .unwrap_or(IpAddr::V4([0, 0, 0, 0].into()));
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .buckets
            .retain(|_, bucket| now.duration_since(bucket.started_at) < self.window);
        if let Some(bucket) = state.buckets.get_mut(&key) {
            if bucket.requests >= self.requests_per_window {
                return false;
            }
            bucket.requests += 1;
            return true;
        }
        if state.buckets.len() >= MAX_TRACKED_SOURCES {
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

#[derive(Debug, Default)]
struct LimiterState {
    buckets: HashMap<IpAddr, Bucket>,
}

#[derive(Clone, Copy, Debug)]
struct Bucket {
    started_at: Instant,
    requests: u32,
}
