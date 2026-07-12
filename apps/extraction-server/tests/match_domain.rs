use extraction_server::{
    match_world::{FixedMatchLoadout, PlayableBounds, ENGINE_MAX_CHUNK, ENGINE_MIN_CHUNK},
    matchmaking::{
        ActivationDeadlines, FrozenRoster, FrozenRosterError, MatchRecord, MatchState,
        MatchVersions, ParticipantState, QueuedPlayer, MATCH_SIZE,
    },
    ports::MatchWorldSpec,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[test]
fn frozen_roster_requires_ten_distinct_accounts_and_public_ids() {
    assert_eq!(
        FrozenRoster::try_from(players(9)),
        Err(FrozenRosterError::WrongSize { actual: 9 })
    );
    assert_eq!(
        FrozenRoster::try_from(players(11)),
        Err(FrozenRosterError::WrongSize { actual: 11 })
    );

    let mut duplicate_account = players(MATCH_SIZE);
    duplicate_account[9].account_id = duplicate_account[0].account_id;
    assert_eq!(
        FrozenRoster::try_from(duplicate_account),
        Err(FrozenRosterError::DuplicateAccount(Uuid::from_u128(1)))
    );

    let mut duplicate_public_id = players(MATCH_SIZE);
    duplicate_public_id[9].public_player_id = duplicate_public_id[0].public_player_id;
    assert_eq!(
        FrozenRoster::try_from(duplicate_public_id),
        Err(FrozenRosterError::DuplicatePublicPlayer(Uuid::from_u128(
            101
        )))
    );
}

#[test]
fn frozen_roster_assigns_stable_fifo_seats() {
    let roster = FrozenRoster::try_from(players(MATCH_SIZE)).unwrap();
    for (index, participant) in roster.iter().enumerate() {
        assert_eq!(usize::from(participant.seat_id.get()), index);
        assert_eq!(participant.account_id, Uuid::from_u128(index as u128 + 1));
        assert_eq!(
            participant.enqueued_at,
            at() + Duration::seconds(index as i64)
        );
    }
}

#[test]
fn match_and_participant_transitions_are_monotonic() {
    assert_eq!(
        MatchState::Waiting.transition_to(MatchState::Preparing),
        Ok(MatchState::Preparing)
    );
    assert_eq!(
        MatchState::Preparing.transition_to(MatchState::Active),
        Ok(MatchState::Active)
    );
    assert!(MatchState::Active
        .transition_to(MatchState::Preparing)
        .is_err());
    assert!(MatchState::Finished
        .transition_to(MatchState::Active)
        .is_err());

    assert_eq!(
        ParticipantState::Active.transition_to(ParticipantState::Disconnected),
        Ok(ParticipantState::Disconnected)
    );
    assert_eq!(
        ParticipantState::Disconnected.transition_to(ParticipantState::Active),
        Ok(ParticipantState::Active)
    );
    assert!(ParticipantState::TimedOut
        .transition_to(ParticipantState::Active)
        .is_err());
    assert!(ParticipantState::Extracted
        .transition_to(ParticipantState::SettlementPending)
        .is_err());
}

#[test]
fn activation_deadlines_are_absolute_and_exact() {
    let started_at = at();
    assert_eq!(
        ActivationDeadlines::from_started_at(started_at),
        Some(ActivationDeadlines {
            extraction_open_at: started_at + Duration::minutes(8),
            hard_deadline: started_at + Duration::minutes(12),
            settlement_grace_deadline: started_at + Duration::minutes(12) + Duration::seconds(30),
        })
    );
}

#[test]
fn world_spec_reuses_exact_bounds_and_fixed_loadout() {
    let roster = FrozenRoster::try_from(players(MATCH_SIZE)).unwrap();
    let record = preparing_record();
    let spec = MatchWorldSpec::from_preparing(&record, roster.clone());

    assert_eq!(spec.match_id, record.match_id);
    assert_eq!(spec.world_name, record.world_name);
    assert_eq!(spec.seed, record.seed);
    assert_eq!(spec.roster, roster);
    assert_eq!(spec.engine_min_chunk, ENGINE_MIN_CHUNK);
    assert_eq!(spec.engine_max_chunk, ENGINE_MAX_CHUNK);
    assert_eq!(spec.playable_bounds, PlayableBounds::EXTRACTION);
    assert_eq!(spec.loadout, FixedMatchLoadout::default());
    assert!(!spec.saving);
}

fn players(count: usize) -> Vec<QueuedPlayer> {
    (0..count)
        .map(|index| QueuedPlayer {
            account_id: Uuid::from_u128(index as u128 + 1),
            public_player_id: Uuid::from_u128(index as u128 + 101),
            enqueued_at: at() + Duration::seconds(index as i64),
        })
        .collect()
}

fn preparing_record() -> MatchRecord {
    MatchRecord {
        match_id: Uuid::from_u128(500),
        state: MatchState::Preparing,
        world_name: "match-500".to_owned(),
        seed: 42,
        versions: MatchVersions {
            generation: "generation-v1".to_owned(),
            gameplay: "gameplay-v1".to_owned(),
            config: "config-v1".to_owned(),
        },
        created_at: at(),
        started_at: None,
        extraction_open_at: None,
        hard_deadline: None,
        settlement_grace_deadline: None,
        finished_at: None,
        abort_reason: None,
    }
}

fn at() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_752_372_000).unwrap()
}
