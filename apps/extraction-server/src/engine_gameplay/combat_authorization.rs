use specs::{Entities, Entity, ReadStorage, WriteStorage};
use voxelize::Clients;

use super::{
    authority::GameplayAuthority,
    components::{EliminationComp, FixedEquipmentComp, HealthComp, MatchPlayerComp},
};
use crate::contracts::AttackWeaponSlot;

#[allow(clippy::too_many_arguments)]
pub(super) fn is_attack_authorized(
    entities: &Entities<'_>,
    authority: &GameplayAuthority,
    clients: &Clients,
    attacker_entity: Entity,
    attacker: &MatchPlayerComp,
    client_id: &str,
    weapon_slot: AttackWeaponSlot,
    equipment: &ReadStorage<'_, FixedEquipmentComp>,
    health: &WriteStorage<'_, HealthComp>,
    eliminations: &WriteStorage<'_, EliminationComp>,
) -> bool {
    entities.is_alive(attacker_entity)
        && attacker.public_player_id().to_string() == client_id
        && authority.allows_entity(clients, attacker_entity, client_id, attacker.account_id())
        && health
            .get(attacker_entity)
            .is_some_and(|value| value.state().is_alive())
        && eliminations
            .get(attacker_entity)
            .is_some_and(|value| value.record().is_none())
        && equipment
            .get(attacker_entity)
            .is_some_and(FixedEquipmentComp::has_basic_melee_weapon)
        && matches!(weapon_slot, AttackWeaponSlot::Melee)
}
