use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use rayon::{ThreadPool, ThreadPoolBuilder};

static ACTIVE_TASKS: AtomicUsize = AtomicUsize::new(0);
static LIVE_WORLDS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub(crate) struct BackgroundTaskTracker {
    inner: Arc<TrackerInner>,
}

struct TrackerInner {
    changed: Condvar,
    state: Mutex<TrackerState>,
}

struct TrackerState {
    accepting: bool,
    in_flight: usize,
}

pub(crate) struct BackgroundTaskPermit {
    inner: Arc<TrackerInner>,
}

pub(crate) struct WorldLifetimeGuard;

impl WorldLifetimeGuard {
    pub(crate) fn new() -> Self {
        LIVE_WORLDS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for WorldLifetimeGuard {
    fn drop(&mut self) {
        LIVE_WORLDS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl BackgroundTaskTracker {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(TrackerInner {
                changed: Condvar::new(),
                state: Mutex::new(TrackerState {
                    accepting: true,
                    in_flight: 0,
                }),
            }),
        }
    }

    pub(crate) fn begin(&self) -> Option<BackgroundTaskPermit> {
        let mut state = self.inner.state.lock().unwrap();
        if !state.accepting {
            return None;
        }
        state.in_flight = state
            .in_flight
            .checked_add(1)
            .expect("background task count overflowed");
        ACTIVE_TASKS.fetch_add(1, Ordering::Relaxed);
        Some(BackgroundTaskPermit {
            inner: self.inner.clone(),
        })
    }

    pub(crate) fn close_and_wait(&self) {
        let mut state = self.inner.state.lock().unwrap();
        state.accepting = false;
        while state.in_flight != 0 {
            state = self.inner.changed.wait(state).unwrap();
        }
    }

    #[cfg(test)]
    fn in_flight(&self) -> usize {
        self.inner.state.lock().unwrap().in_flight
    }
}

impl Drop for BackgroundTaskPermit {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock().unwrap();
        state.in_flight = state
            .in_flight
            .checked_sub(1)
            .expect("background task count underflowed");
        ACTIVE_TASKS.fetch_sub(1, Ordering::Relaxed);
        if state.in_flight == 0 {
            self.inner.changed.notify_all();
        }
    }
}

pub(crate) fn resource_counts() -> (usize, usize) {
    (
        LIVE_WORLDS.load(Ordering::Relaxed),
        ACTIVE_TASKS.load(Ordering::Relaxed),
    )
}

pub(crate) fn shared_worker_pool() -> Arc<ThreadPool> {
    static POOL: OnceLock<Arc<ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        Arc::new(
            ThreadPoolBuilder::new()
                .thread_name(|index| format!("voxelize-world-{index}"))
                .build()
                .expect("failed to build shared world worker pool"),
        )
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use super::*;

    #[test]
    fn close_waits_for_registered_work_and_rejects_late_tasks() {
        let tracker = BackgroundTaskTracker::new();
        let permit = tracker.begin().unwrap();
        let waiter = tracker.clone();
        let (done_sender, done_receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            waiter.close_and_wait();
            done_sender.send(()).unwrap();
        });

        assert!(done_receiver
            .recv_timeout(Duration::from_millis(25))
            .is_err());
        assert_eq!(tracker.in_flight(), 1);
        drop(permit);
        done_receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        handle.join().unwrap();
        assert!(tracker.begin().is_none());
    }

    #[test]
    fn worker_pool_is_shared_for_the_process_lifetime() {
        assert!(Arc::ptr_eq(&shared_worker_pool(), &shared_worker_pool()));
    }
}
