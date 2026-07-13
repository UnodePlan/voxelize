use std::{collections::HashMap, time::Duration};

use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    gate::AttachGate,
    gate_types::{AttachDecision, AttachRequest, GateParticipant, GateSnapshot},
    MatchAttachKind, MatchState, ParticipantState,
};

#[test]
fn hard_deadline_closure_cannot_be_reopened_by_stale_sync() {
    let account_id = Uuid::from_u128(1);
    let public_player_id = Uuid::from_u128(2);
    let gate = AttachGate::default();
    let active = || GateSnapshot {
        world_name: "match-1".to_owned(),
        world_generation: Some("generation-1".to_owned()),
        state: MatchState::Active,
        #[cfg(feature = "engine")]
        extraction_open: false,
        #[cfg(feature = "engine")]
        hard_deadline: Some(Duration::from_secs(720)),
        #[cfg(feature = "engine")]
        hard_deadline_utc: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(720)),
        participants: HashMap::from([(
            account_id,
            GateParticipant {
                public_player_id,
                state: ParticipantState::Disconnected,
                joined: true,
                reconnect_deadline: Some(Duration::from_secs(60)),
            },
        )]),
    };
    gate.replace(Some(active()));
    assert!(gate.close_for_hard_deadline("match-1", "generation-1"));
    assert!(gate.close_for_hard_deadline("match-1", "generation-1"));

    // 模拟跨过硬截止的慢 SQL 返回后，协调器仍拿旧 Active 状态执行 sync_gate。
    gate.replace(Some(active()));
    assert_eq!(
        gate.allows(
            AttachRequest {
                world_name: "match-1",
                world_generation: "generation-1",
                client_id: &public_player_id.to_string(),
                attach_attempt_id: "attempt-2",
                account_id,
                kind: MatchAttachKind::Rebind,
            },
            Duration::from_secs(10),
            OffsetDateTime::UNIX_EPOCH,
        ),
        AttachDecision::Denied
    );
}

#[test]
fn rebind_admission_and_timeout_claim_are_linearized() {
    let account_id = Uuid::from_u128(11);
    let public_player_id = Uuid::from_u128(12);
    let snapshot = || GateSnapshot {
        world_name: "match-2".to_owned(),
        world_generation: Some("generation-2".to_owned()),
        state: MatchState::Active,
        #[cfg(feature = "engine")]
        extraction_open: false,
        #[cfg(feature = "engine")]
        hard_deadline: Some(Duration::from_secs(720)),
        #[cfg(feature = "engine")]
        hard_deadline_utc: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(720)),
        participants: HashMap::from([(
            account_id,
            GateParticipant {
                public_player_id,
                state: ParticipantState::Disconnected,
                joined: true,
                reconnect_deadline: Some(Duration::from_secs(60)),
            },
        )]),
    };
    let client_id = public_player_id.to_string();

    let timeout_wins = AttachGate::default();
    timeout_wins.replace(Some(snapshot()));
    assert!(timeout_wins.claim_rebind_timeout("match-2", "generation-2", account_id));
    assert_eq!(
        timeout_wins.allows(
            AttachRequest {
                world_name: "match-2",
                world_generation: "generation-2",
                client_id: &client_id,
                attach_attempt_id: "late-reservation",
                account_id,
                kind: MatchAttachKind::Rebind,
            },
            Duration::from_secs(59),
            OffsetDateTime::UNIX_EPOCH,
        ),
        AttachDecision::Denied
    );

    let admission_wins = AttachGate::default();
    admission_wins.replace(Some(snapshot()));
    assert_eq!(
        admission_wins.allows(
            AttachRequest {
                world_name: "match-2",
                world_generation: "generation-2",
                client_id: &client_id,
                attach_attempt_id: "admitted-reservation",
                account_id,
                kind: MatchAttachKind::Rebind,
            },
            Duration::from_secs(59),
            OffsetDateTime::UNIX_EPOCH,
        ),
        AttachDecision::Allowed
    );
    assert!(!admission_wins.claim_rebind_timeout("match-2", "generation-2", account_id));
}

#[test]
fn stale_join_commit_cannot_consume_a_newer_attempt() {
    let account_id = Uuid::from_u128(21);
    let public_player_id = Uuid::from_u128(22);
    let client_id = public_player_id.to_string();
    let gate = AttachGate::default();
    gate.replace(Some(GateSnapshot {
        world_name: "match-3".to_owned(),
        world_generation: Some("generation-3".to_owned()),
        state: MatchState::Preparing,
        #[cfg(feature = "engine")]
        extraction_open: false,
        #[cfg(feature = "engine")]
        hard_deadline: None,
        #[cfg(feature = "engine")]
        hard_deadline_utc: None,
        participants: HashMap::from([(
            account_id,
            GateParticipant {
                public_player_id,
                state: ParticipantState::Preparing,
                joined: false,
                reconnect_deadline: None,
            },
        )]),
    }));
    for attempt_id in ["join-attempt-old", "join-attempt-new"] {
        assert_eq!(
            gate.allows(
                AttachRequest {
                    world_name: "match-3",
                    world_generation: "generation-3",
                    client_id: &client_id,
                    attach_attempt_id: attempt_id,
                    account_id,
                    kind: MatchAttachKind::Join,
                },
                Duration::ZERO,
                OffsetDateTime::UNIX_EPOCH,
            ),
            AttachDecision::Allowed
        );
    }

    assert!(!gate.take_join_reservation(
        "match-3",
        "generation-3",
        &client_id,
        "join-attempt-old",
        account_id,
    ));
    assert!(gate.take_join_reservation(
        "match-3",
        "generation-3",
        &client_id,
        "join-attempt-new",
        account_id,
    ));
}
