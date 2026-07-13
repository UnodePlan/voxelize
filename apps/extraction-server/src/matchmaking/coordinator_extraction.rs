use super::{
    coordinator::{Coordinator, PendingSettlement},
    CommitSettlement, MatchmakingError, ParticipantState, SettlementRecord,
};
#[cfg(any(feature = "engine", test))]
use super::{MatchExtractionNotice, MatchState};
use crate::ports::{SettlementRepositoryError, TransitionOutcome};

#[cfg(any(feature = "engine", test))]
const POST_GRACE_RECONCILIATION_READS: u8 = 3;

impl Coordinator {
    #[cfg(any(feature = "engine", test))]
    pub(super) async fn apply_extraction(
        &mut self,
        notice: MatchExtractionNotice,
    ) -> Result<(), MatchmakingError> {
        if !notice.is_valid() {
            return Err(MatchmakingError::RosterLocked);
        }
        let qualification = &notice.qualification;
        let Some(current) = self.current.as_ref() else {
            return Ok(());
        };
        if current.match_id != qualification.match_id
            || current.world_name != notice.world_name
            || current.world_generation.as_deref() != Some(&notice.world_generation)
        {
            return Ok(());
        }
        if current.state != MatchState::ExtractionOpen
            || current
                .hard_deadline_utc
                .is_none_or(|at| qualification.qualified_at > at)
            || current
                .extraction_open_at_utc
                .is_none_or(|at| qualification.qualified_at < at)
        {
            return Err(MatchmakingError::RosterLocked);
        }
        let grace_deadline = current
            .settlement_grace_deadline_utc
            .ok_or(MatchmakingError::Unavailable)?;
        let participant_state = current
            .participants
            .get(&qualification.account_id)
            .map(|participant| participant.state)
            .ok_or(MatchmakingError::RosterLocked)?;
        let key = (qualification.match_id, qualification.account_id);
        if let Some(existing) = self.pending_settlements.get(&key) {
            return (existing.command.qualification == *qualification)
                .then_some(())
                .ok_or(MatchmakingError::RosterLocked);
        }
        if participant_state == ParticipantState::Extracted {
            let record = self
                .repository
                .find_settlement(key.0, key.1)
                .await
                .map_err(settlement_error)?
                .ok_or(MatchmakingError::RosterLocked)?;
            return verify_qualification(qualification, &record);
        }
        if participant_state != ParticipantState::Active {
            return Err(MatchmakingError::RosterLocked);
        }
        if let Some(participant) = self
            .current
            .as_mut()
            .and_then(|current| current.participants.get_mut(&qualification.account_id))
        {
            participant.state = ParticipantState::SettlementPending;
            participant.control_connection = None;
            participant.reconnect_deadline = None;
        }
        self.sync_gate();
        self.pending_settlements.insert(
            key,
            PendingSettlement {
                command: CommitSettlement {
                    settlement_id: self.ids.next_uuid(),
                    qualification: qualification.clone(),
                },
                grace_deadline,
                pending_persisted: false,
                read_before_write: false,
                post_grace_reads_remaining: POST_GRACE_RECONCILIATION_READS,
            },
        );
        self.progress_settlement(key).await
    }

    pub(super) async fn retry_pending_settlements(&mut self) -> Result<(), MatchmakingError> {
        let keys = self.pending_settlements.keys().copied().collect::<Vec<_>>();
        for key in keys {
            self.progress_settlement(key).await?;
        }
        Ok(())
    }

    async fn progress_settlement(
        &mut self,
        key: (uuid::Uuid, uuid::Uuid),
    ) -> Result<(), MatchmakingError> {
        let Some(mut pending) = self.pending_settlements.get(&key).cloned() else {
            return Ok(());
        };
        let after_grace = self.utc_now() > pending.grace_deadline;
        if after_grace {
            return self.reconcile_after_grace(key, pending).await;
        }
        if !pending.pending_persisted {
            match self
                .repository
                .mark_settlement_pending(pending.command.qualification.clone())
                .await
            {
                Ok(_) => pending.pending_persisted = true,
                Err(SettlementRepositoryError::Unavailable) => return Ok(()),
                Err(error) => return Err(settlement_error(error)),
            }
            self.pending_settlements.insert(key, pending.clone());
        }

        if pending.read_before_write {
            match self.repository.find_settlement(key.0, key.1).await {
                Ok(Some(record)) => {
                    verify_record(&pending.command, &record)?;
                    return self.complete_settlement(key).await;
                }
                Ok(None) => {
                    pending.read_before_write = false;
                    self.pending_settlements.insert(key, pending);
                    return Ok(());
                }
                Err(SettlementRepositoryError::Unavailable) => return Ok(()),
                Err(error) => return Err(settlement_error(error)),
            }
        }

        match self
            .repository
            .commit_settlement(pending.command.clone())
            .await
        {
            Ok(TransitionOutcome::Applied(record) | TransitionOutcome::AlreadyApplied(record)) => {
                verify_record(&pending.command, &record)?;
                self.complete_settlement(key).await
            }
            Err(SettlementRepositoryError::Unavailable) => Ok(()),
            Err(SettlementRepositoryError::OutcomeUnknown) => {
                pending.read_before_write = true;
                self.pending_settlements.insert(key, pending);
                Ok(())
            }
            Err(SettlementRepositoryError::WindowClosed) => {
                pending.read_before_write = true;
                self.pending_settlements.insert(key, pending);
                Ok(())
            }
            Err(error) => Err(settlement_error(error)),
        }
    }

    async fn complete_settlement(
        &mut self,
        key: (uuid::Uuid, uuid::Uuid),
    ) -> Result<(), MatchmakingError> {
        self.pending_settlements.remove(&key);
        self.update_local_terminal(key, ParticipantState::Extracted);
        let world_name = self
            .current
            .as_ref()
            .filter(|current| current.match_id == key.0)
            .map(|current| current.world_name.clone());
        if let Some(world_name) = world_name {
            if self.evict_one(&world_name, key.1).await.is_err() {
                if let Some(participant) = self
                    .current
                    .as_mut()
                    .and_then(|current| current.participants.get_mut(&key.1))
                {
                    participant.despawn_pending = true;
                }
            }
        }
        Ok(())
    }

    async fn reconcile_after_grace(
        &mut self,
        key: (uuid::Uuid, uuid::Uuid),
        mut pending: PendingSettlement,
    ) -> Result<(), MatchmakingError> {
        match self.repository.find_settlement(key.0, key.1).await {
            Ok(Some(record)) => {
                verify_record(&pending.command, &record)?;
                self.complete_settlement(key).await
            }
            Ok(None) => {
                self.pending_settlements.remove(&key);
                self.update_local_terminal(key, ParticipantState::Aborted);
                Ok(())
            }
            Err(SettlementRepositoryError::Unavailable) => {
                pending.post_grace_reads_remaining =
                    pending.post_grace_reads_remaining.saturating_sub(1);
                if pending.post_grace_reads_remaining == 0 {
                    self.pending_settlements.remove(&key);
                    self.update_local_terminal(key, ParticipantState::Aborted);
                } else {
                    self.pending_settlements.insert(key, pending);
                }
                Ok(())
            }
            Err(error) => Err(settlement_error(error)),
        }
    }

    fn update_local_terminal(&mut self, key: (uuid::Uuid, uuid::Uuid), state: ParticipantState) {
        if let Some(participant) = self
            .current
            .as_mut()
            .filter(|current| current.match_id == key.0)
            .and_then(|current| current.participants.get_mut(&key.1))
        {
            participant.state = state;
            participant.control_connection = None;
            participant.reconnect_deadline = None;
        }
        self.sync_gate();
    }
}

fn verify_record(
    command: &CommitSettlement,
    record: &SettlementRecord,
) -> Result<(), MatchmakingError> {
    let qualification = &command.qualification;
    (record.settlement_id == command.settlement_id)
        .then_some(())
        .ok_or(MatchmakingError::RosterLocked)?;
    verify_qualification(qualification, record)
}

fn verify_qualification(
    qualification: &super::ExtractionQualification,
    record: &SettlementRecord,
) -> Result<(), MatchmakingError> {
    (record.match_id == qualification.match_id
        && record.account_id == qualification.account_id
        && record.idempotency_key == qualification.idempotency_key()
        && record.inventory_digest == qualification.inventory_digest
        && record.config_version == qualification.config_version
        && record.resources == qualification.resources)
        .then_some(())
        .ok_or(MatchmakingError::RosterLocked)
}

fn settlement_error(error: SettlementRepositoryError) -> MatchmakingError {
    match error {
        SettlementRepositoryError::Unavailable | SettlementRepositoryError::OutcomeUnknown => {
            MatchmakingError::Unavailable
        }
        SettlementRepositoryError::Conflict
        | SettlementRepositoryError::WindowClosed
        | SettlementRepositoryError::Overflow
        | SettlementRepositoryError::Invariant => MatchmakingError::RosterLocked,
    }
}
