use specs::{Entities, Join, ReadStorage, WriteStorage};

use super::{
    authority::GameplayAuthority,
    components::{EliminationComp, MatchPlayerComp, RoundStatsComp},
};
use crate::{
    contracts::DeathCause,
    gameplay::loot::ResourceBundle,
    matchmaking::{
        ParticipantDeath, ParticipantMatchStats, ParticipantResourceCounts,
        ParticipantTerminalCause, ParticipantTimeout,
    },
};

pub(super) fn flush_death_notices<'a>(
    entities: &Entities<'a>,
    authority: &GameplayAuthority,
    match_id: uuid::Uuid,
    players: &ReadStorage<'a, MatchPlayerComp>,
    stats: &WriteStorage<'a, RoundStatsComp>,
    eliminations: &mut WriteStorage<'a, EliminationComp>,
) {
    for (_, player, stats, elimination) in (entities, players, stats, eliminations).join() {
        let Some(record) = elimination.record_mut() else {
            continue;
        };
        if record.notice_sent {
            continue;
        }
        let match_stats = participant_match_stats(stats.stats());
        let reported = match record.result.data.cause {
            DeathCause::Melee => record.killer_account_id.is_some_and(|killer_account_id| {
                authority.report_death(ParticipantDeath {
                    match_id,
                    victim_account_id: player.account_id(),
                    killer_account_id,
                    occurred_at: record.occurred_at,
                    survived_ms: record.result.data.survived_ms,
                    stats: match_stats,
                })
            }),
            cause @ (DeathCause::ReconnectTimeout | DeathCause::HardDeadline) => authority
                .report_timeout_elimination(ParticipantTimeout {
                    match_id,
                    account_id: player.account_id(),
                    cause: match cause {
                        DeathCause::ReconnectTimeout => ParticipantTerminalCause::ReconnectTimeout,
                        DeathCause::HardDeadline => ParticipantTerminalCause::HardDeadline,
                        DeathCause::Melee => unreachable!(),
                    },
                    occurred_at: record.occurred_at,
                    survived_ms: record.result.data.survived_ms,
                    stats: match_stats,
                }),
        };
        if reported {
            record.notice_sent = true;
        }
    }
}

pub(super) fn participant_match_stats(
    stats: &crate::gameplay::round_stats::RoundStats,
) -> ParticipantMatchStats {
    ParticipantMatchStats {
        mined: counts(stats.mined()),
        picked_up: counts(stats.picked_up()),
        lost: counts(stats.lost()),
    }
}

fn counts(bundle: ResourceBundle) -> ParticipantResourceCounts {
    ParticipantResourceCounts {
        dirt: u64::from(bundle.quantity(crate::contracts::ResourceKey::Dirt)),
        gold: u64::from(bundle.quantity(crate::contracts::ResourceKey::Gold)),
        diamond: u64::from(bundle.quantity(crate::contracts::ResourceKey::Diamond)),
    }
}
