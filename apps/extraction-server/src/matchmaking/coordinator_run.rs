use tokio::sync::mpsc;

use super::{command::Command, coordinator::Coordinator, MatchmakingError, QueueSnapshot};

impl Coordinator {
    pub(super) async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        while let Some(command) = receiver.recv().await {
            match command {
                Command::BindRuntime { runtime, reply } => {
                    if self.runtime.is_none() {
                        self.runtime = Some(runtime);
                    }
                    let _ = reply.send(());
                }
                Command::Enqueue { account_id, reply } => {
                    let result = self.enqueue(account_id).await;
                    let _ = reply.send(result);
                }
                Command::Cancel { account_id, reply } => {
                    let result = self.cancel(account_id).await;
                    let _ = reply.send(result);
                }
                Command::FindQueueSnapshot { account_id, reply } => {
                    let snapshot = self
                        .snapshot_for(account_id)
                        .unwrap_or_else(|| QueueSnapshot::idle(false));
                    let _ = reply.send(Ok(snapshot));
                }
                Command::FindMatchResult {
                    match_id,
                    account_id,
                    reply,
                } => {
                    let result = self
                        .repository
                        .find_match_result(match_id, account_id)
                        .await
                        .map_err(|_| MatchmakingError::Unavailable);
                    let _ = reply.send(result);
                }
                Command::FindLatestMatchResult { account_id, reply } => {
                    let result = self
                        .repository
                        .find_latest_match_result(account_id)
                        .await
                        .map_err(|_| MatchmakingError::Unavailable);
                    let _ = reply.send(result);
                }
                Command::Connection { event, reply } => {
                    let observed = reply.is_none();
                    let result = self.apply_connection(event).await;
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    } else if observed && result.is_err() {
                        self.gate.fail_closed();
                        let _ = self.abort_current("connection_event_failed").await;
                    }
                }
                #[cfg(any(feature = "engine", test))]
                Command::Death { notice } => {
                    if self.apply_death(notice).await.is_err() {
                        self.gate.fail_closed();
                        let _ = self.abort_current("death_notice_failed").await;
                    }
                }
                #[cfg(any(feature = "engine", test))]
                Command::TimeoutElimination { notice } => {
                    if self.apply_timeout_elimination(notice).await.is_err() {
                        self.gate.fail_closed();
                        let _ = self
                            .abort_current("timeout_elimination_notice_failed")
                            .await;
                    }
                }
                #[cfg(any(feature = "engine", test))]
                Command::Extraction { notice } => {
                    if self.apply_extraction(notice).await.is_err() {
                        self.gate.fail_closed();
                        let _ = self.abort_current("extraction_notice_failed").await;
                    }
                }
                Command::HardDeadlineSealed {
                    match_id,
                    world_name,
                    world_generation,
                    sealed,
                    world_stopped,
                } => {
                    if self
                        .complete_hard_deadline_seal(
                            match_id,
                            &world_name,
                            &world_generation,
                            sealed,
                            world_stopped,
                        )
                        .await
                        .is_err()
                    {
                        self.gate.fail_closed();
                        let _ = self.abort_current("hard_deadline_seal_failed").await;
                    }
                }
                Command::Tick {
                    reply,
                    ticker_pending,
                } => {
                    if let Some(pending) = ticker_pending {
                        pending.store(false, std::sync::atomic::Ordering::Release);
                    }
                    let result = self.advance_time().await;
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                }
                Command::FailClosed => {
                    let _ = self.abort_current("connection_event_overflow").await;
                }
            }
        }
    }
}
