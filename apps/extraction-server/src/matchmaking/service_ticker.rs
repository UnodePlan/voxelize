use std::{
    sync::{atomic::Ordering, Arc, Weak},
    time::Duration,
};

use tokio::sync::mpsc;

use super::{command::Command, service::MatchmakingService};

const TICK_INTERVAL: Duration = Duration::from_millis(100);

impl MatchmakingService {
    pub fn start_ticker(self: &Arc<Self>) {
        let service = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(TICK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(service) = Weak::upgrade(&service) else {
                    break;
                };
                if !service.schedule_tick_once() {
                    break;
                }
            }
        });
    }

    pub(super) fn schedule_tick_once(&self) -> bool {
        if self
            .tick_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return true;
        }
        let command = Command::Tick {
            reply: None,
            ticker_pending: Some(self.tick_pending.clone()),
        };
        match self.sender.try_send(command) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.tick_pending.store(false, Ordering::Release);
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }
}
