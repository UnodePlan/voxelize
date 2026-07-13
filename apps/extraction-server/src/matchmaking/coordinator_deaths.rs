use super::{
    coordinator::{repository_error, Coordinator},
    MatchDeathNotice, MatchState, MatchTimeoutNotice, MatchmakingError, ParticipantDeath,
    ParticipantRecord, ParticipantState, ParticipantTimeout,
};

impl Coordinator {
    pub(super) async fn apply_death(
        &mut self,
        notice: MatchDeathNotice,
    ) -> Result<(), MatchmakingError> {
        if !notice.is_valid() {
            return Err(MatchmakingError::RosterLocked);
        }
        let Some(current) = self.current.as_ref() else {
            return Ok(());
        };
        if current.match_id != notice.death.match_id
            || current.world_name != notice.world_name
            || current.world_generation.as_deref() != Some(&notice.world_generation)
        {
            // 旧 World 的延迟通知不能影响复用同名世界后的新比赛。
            return Ok(());
        }
        if !matches!(
            current.state,
            MatchState::Active | MatchState::ExtractionOpen
        ) {
            // hard deadline/abort 已取得终态所有权时，迟到的 World 通知只能被忽略。
            return Ok(());
        }
        if !current
            .participants
            .contains_key(&notice.death.killer_account_id)
        {
            return Err(MatchmakingError::RosterLocked);
        }

        let victim_state = current
            .participants
            .get(&notice.death.victim_account_id)
            .map(|participant| participant.state)
            .ok_or(MatchmakingError::RosterLocked)?;
        if !matches!(
            victim_state,
            ParticipantState::Active | ParticipantState::Disconnected | ParticipantState::Dead
        ) {
            return Err(MatchmakingError::RosterLocked);
        }
        if let Some(participant) = self.current.as_mut().and_then(|current| {
            current
                .participants
                .get_mut(&notice.death.victim_account_id)
        }) {
            participant.state = ParticipantState::Dead;
            participant.control_connection = None;
            participant.reconnect_deadline = None;
        }
        // 先关闭 gameplay/rebind gate；数据库失败会中止整局，绝不把死者重新开放。
        self.sync_gate();

        let death = notice.death;
        if self
            .evict_one(&notice.world_name, death.victim_account_id)
            .await
            .is_err()
        {
            // 最终 Direct 已在 World Broadcast 中发出；有限驱逐失败改由 Tick 重试。
            if let Some(participant) = self
                .current
                .as_mut()
                .and_then(|current| current.participants.get_mut(&death.victim_account_id))
            {
                participant.despawn_pending = true;
            }
        }
        let record = self
            .repository
            .mark_dead(death.clone())
            .await
            .map_err(repository_error)?
            .into_value();
        if !record_matches_death(&record, &death) {
            return Err(MatchmakingError::RosterLocked);
        }
        Ok(())
    }
}

impl Coordinator {
    pub(super) async fn apply_timeout_elimination(
        &mut self,
        notice: MatchTimeoutNotice,
    ) -> Result<(), MatchmakingError> {
        if !notice.is_valid() {
            return Err(MatchmakingError::RosterLocked);
        }
        let Some(current) = self.current.as_ref() else {
            return Ok(());
        };
        if current.match_id != notice.timeout.match_id
            || current.world_name != notice.world_name
            || current.world_generation.as_deref() != Some(&notice.world_generation)
        {
            return Ok(());
        }
        if !matches!(
            current.state,
            MatchState::Active | MatchState::ExtractionOpen
        ) {
            return Ok(());
        }
        let state = current
            .participants
            .get(&notice.timeout.account_id)
            .map(|participant| participant.state)
            .ok_or(MatchmakingError::RosterLocked)?;
        if !matches!(
            state,
            ParticipantState::Disconnected | ParticipantState::TimedOut
        ) {
            return Err(MatchmakingError::RosterLocked);
        }
        if let Some(participant) = self
            .current
            .as_mut()
            .and_then(|current| current.participants.get_mut(&notice.timeout.account_id))
        {
            participant.state = ParticipantState::TimedOut;
            participant.control_connection = None;
            participant.reconnect_deadline = None;
        }
        self.sync_gate();

        let timeout = notice.timeout;
        if self
            .evict_one(&notice.world_name, timeout.account_id)
            .await
            .is_err()
        {
            if let Some(participant) = self
                .current
                .as_mut()
                .and_then(|current| current.participants.get_mut(&timeout.account_id))
            {
                participant.despawn_pending = true;
            }
        }
        let record = self
            .repository
            .mark_timed_out(timeout.clone(), self.utc_now())
            .await
            .map_err(repository_error)?
            .into_value();
        if !record_matches_timeout(&record, &timeout) {
            return Err(MatchmakingError::RosterLocked);
        }
        Ok(())
    }
}

fn record_matches_death(record: &ParticipantRecord, death: &ParticipantDeath) -> bool {
    record.match_id == death.match_id
        && record.account_id == death.victim_account_id
        && record.state == ParticipantState::Dead
        && record.killed_by_account_id == Some(death.killer_account_id)
        && record.stats == death.stats
        && record.reconnect_deadline.is_none()
}

fn record_matches_timeout(record: &ParticipantRecord, timeout: &ParticipantTimeout) -> bool {
    record.match_id == timeout.match_id
        && record.account_id == timeout.account_id
        && record.state == ParticipantState::TimedOut
        && record.killed_by_account_id.is_none()
        && record.stats == timeout.stats
        && record.reconnect_deadline.is_none()
}
