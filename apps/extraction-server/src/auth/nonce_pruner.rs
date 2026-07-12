use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use time::OffsetDateTime;

use crate::ports::AuthRepository;

const NONCE_PRUNE_INTERVAL: u64 = 64;
const NONCE_PRUNE_BATCH: u32 = 256;

#[derive(Clone, Default)]
pub(super) struct NoncePruner {
    issue_count: Arc<AtomicU64>,
    in_flight: Arc<AtomicBool>,
}

impl NoncePruner {
    pub(super) fn after_issue(&self, repository: Arc<dyn AuthRepository>, now: OffsetDateTime) {
        let issue_count = self
            .issue_count
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        if !issue_count.is_multiple_of(NONCE_PRUNE_INTERVAL)
            || self
                .in_flight
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return;
        }

        let reset = InFlightReset(self.in_flight.clone());
        let _prune_task = actix_web::rt::spawn(async move {
            let _reset = reset;
            let _ = repository
                .prune_expired_nonces(now, NONCE_PRUNE_BATCH)
                .await;
        });
    }
}

struct InFlightReset(Arc<AtomicBool>);

impl Drop for InFlightReset {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
