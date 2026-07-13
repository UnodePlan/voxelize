use std::io::{self, Write};

use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

const MATCH_EVENT_SCHEMA: &str = "extraction.match.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservedMatchPhase {
    Preparing,
    Active,
    ExtractionOpen,
    Settling,
    Finished,
    Aborted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RejectionReason {
    ConnectionRequired,
    Full,
    RosterLocked,
    ReconnectExpired,
    Unavailable,
    ConnectionEventInvalid,
    #[cfg(any(feature = "engine", test))]
    DeathNoticeInvalid,
    #[cfg(any(feature = "engine", test))]
    TimeoutNoticeInvalid,
    #[cfg(any(feature = "engine", test))]
    ExtractionNoticeInvalid,
    HardDeadlineSealFailed,
}

#[cfg(any(feature = "engine", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TerminalKind {
    Melee,
    ReconnectTimeout,
    HardDeadline,
}

#[cfg(any(feature = "engine", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApplyOutcome {
    Applied,
    AlreadyApplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SettlementOutcome {
    #[cfg(any(feature = "engine", test))]
    Qualified,
    CommitApplied,
    CommitAlreadyApplied,
    OutcomeUnknown,
    RecoveredCommitted,
    ReconciledAbsent,
    ReconciliationExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryOutcome {
    Completed,
    Failed,
}

/// 事件类型刻意不接受账号、钱包、连接、会话或任意错误字符串，防止调用方误记密钥。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum MatchEvent {
    PhaseChanged {
        match_id: Uuid,
        phase: ObservedMatchPhase,
    },
    RequestRejected {
        match_id: Option<Uuid>,
        reason: RejectionReason,
    },
    #[cfg(any(feature = "engine", test))]
    ParticipantDeath {
        match_id: Uuid,
        kind: TerminalKind,
        outcome: ApplyOutcome,
    },
    Settlement {
        match_id: Uuid,
        outcome: SettlementOutcome,
    },
    StartupRecovery {
        outcome: RecoveryOutcome,
        affected_matches: u64,
    },
}

pub(crate) trait MatchEventSink: Send + Sync {
    fn record(&self, event: MatchEvent);
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StderrMatchEventSink;

impl MatchEventSink for StderrMatchEventSink {
    fn record(&self, event: MatchEvent) {
        let stderr = io::stderr();
        write_event(stderr.lock(), &event, OffsetDateTime::now_utc());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchEventRecord<'a> {
    schema: &'static str,
    recorded_at: OffsetDateTime,
    #[serde(flatten)]
    event: &'a MatchEvent,
}

fn encode_event(
    event: &MatchEvent,
    recorded_at: OffsetDateTime,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MatchEventRecord {
        schema: MATCH_EVENT_SCHEMA,
        recorded_at,
        event,
    })
}

fn write_event(mut writer: impl Write, event: &MatchEvent, recorded_at: OffsetDateTime) {
    let encoded = encode_event(event, recorded_at).unwrap_or_else(|_| {
        r#"{"schema":"extraction.match.v1","event":"encode_failed"}"#.to_owned()
    });
    // 观测管道故障不能终止 matchmaking 协调器或改变领域结果。
    let _ = writeln!(writer, "{encoded}");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed log pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_observability_pipe_never_panics_or_changes_control_flow() {
        write_event(
            BrokenWriter,
            &MatchEvent::StartupRecovery {
                outcome: RecoveryOutcome::Completed,
                affected_matches: 0,
            },
            OffsetDateTime::UNIX_EPOCH,
        );
    }

    #[test]
    fn structured_match_events_cannot_serialize_secrets_or_identity_fields() {
        let match_id = Uuid::from_u128(42);
        let mut events = Vec::new();
        events.extend(
            [
                ObservedMatchPhase::Preparing,
                ObservedMatchPhase::Active,
                ObservedMatchPhase::ExtractionOpen,
                ObservedMatchPhase::Settling,
                ObservedMatchPhase::Finished,
                ObservedMatchPhase::Aborted,
            ]
            .map(|phase| MatchEvent::PhaseChanged { match_id, phase }),
        );
        events.extend(
            [
                RejectionReason::ConnectionRequired,
                RejectionReason::Full,
                RejectionReason::RosterLocked,
                RejectionReason::ReconnectExpired,
                RejectionReason::Unavailable,
                RejectionReason::ConnectionEventInvalid,
                RejectionReason::DeathNoticeInvalid,
                RejectionReason::TimeoutNoticeInvalid,
                RejectionReason::ExtractionNoticeInvalid,
                RejectionReason::HardDeadlineSealFailed,
            ]
            .map(|reason| MatchEvent::RequestRejected {
                match_id: Some(match_id),
                reason,
            }),
        );
        events.extend(
            [
                TerminalKind::Melee,
                TerminalKind::ReconnectTimeout,
                TerminalKind::HardDeadline,
            ]
            .map(|kind| MatchEvent::ParticipantDeath {
                match_id,
                outcome: ApplyOutcome::Applied,
                kind,
            }),
        );
        events.push(MatchEvent::ParticipantDeath {
            match_id,
            kind: TerminalKind::Melee,
            outcome: ApplyOutcome::AlreadyApplied,
        });
        events.extend(
            [
                SettlementOutcome::Qualified,
                SettlementOutcome::CommitApplied,
                SettlementOutcome::CommitAlreadyApplied,
                SettlementOutcome::OutcomeUnknown,
                SettlementOutcome::RecoveredCommitted,
                SettlementOutcome::ReconciledAbsent,
                SettlementOutcome::ReconciliationExhausted,
            ]
            .map(|outcome| MatchEvent::Settlement { match_id, outcome }),
        );
        events.extend(
            [RecoveryOutcome::Completed, RecoveryOutcome::Failed].map(|outcome| {
                MatchEvent::StartupRecovery {
                    outcome,
                    affected_matches: 3,
                }
            }),
        );
        let forbidden = [
            "account",
            "authorization",
            "client_id",
            "cookie",
            "nonce",
            "payload",
            "principal",
            "private_key",
            "rpc",
            "session",
            "signature",
            "siwe",
            "secret",
            "token",
            "wallet",
            "address",
        ];

        for event in events {
            let encoded = encode_event(&event, OffsetDateTime::UNIX_EPOCH).unwrap();
            assert!(encoded.contains(r#""schema":"extraction.match.v1""#));
            assert!(!encoded.contains("match_id"));
            if !matches!(event, MatchEvent::StartupRecovery { .. }) {
                assert!(encoded.contains("matchId"));
            }
            for field in forbidden {
                assert!(!encoded.to_ascii_lowercase().contains(field), "{encoded}");
            }
        }
    }
}
