use specs::ReadStorage;

use super::{
    components::MatchPlayerComp,
    intents::{AttackIntentQueue, QueuedAttackIntent},
};

pub(super) fn drain_attacks_in_stable_order(
    intents: &mut AttackIntentQueue,
    players: &ReadStorage<'_, MatchPlayerComp>,
) -> Vec<QueuedAttackIntent> {
    let mut attacks = intents.drain().collect::<Vec<_>>();
    // 稳定排序只统一跨玩家裁决顺序；同一玩家必须保留网络接收顺序。
    attacks.sort_by_key(|intent| {
        players
            .get(intent.entity)
            .map_or(u8::MAX, |player| player.seat_id().get())
    });
    attacks
}
