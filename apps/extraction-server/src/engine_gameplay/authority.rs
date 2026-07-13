use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use specs::Entity;
use uuid::Uuid;
use voxelize::{Clients, World};

use crate::matchmaking::MatchmakingService;

#[derive(Clone)]
pub(crate) struct GameplayAuthority {
    matchmaking: Weak<MatchmakingService>,
    generations: Arc<Mutex<HashMap<String, String>>>,
    world_name: String,
    #[cfg(test)]
    test_now: Option<Duration>,
}

impl GameplayAuthority {
    pub(crate) fn new(
        matchmaking: Weak<MatchmakingService>,
        generations: Arc<Mutex<HashMap<String, String>>>,
        world_name: String,
    ) -> Self {
        Self {
            matchmaking,
            generations,
            world_name,
            #[cfg(test)]
            test_now: None,
        }
    }

    #[cfg(test)]
    pub(super) fn allow_all_at(now: Duration) -> Self {
        Self {
            matchmaking: Weak::new(),
            generations: Arc::new(Mutex::new(HashMap::new())),
            world_name: "test-world".to_owned(),
            test_now: Some(now),
        }
    }

    pub(crate) fn allows(&self, client_id: &str, account_id: Uuid) -> bool {
        #[cfg(test)]
        if self.test_now.is_some() {
            return !client_id.is_empty() && !account_id.is_nil();
        }
        let Some(service) = self.matchmaking.upgrade() else {
            return false;
        };
        let Some(world_generation) = self.world_generation(&service) else {
            return false;
        };
        service.allows_gameplay(&self.world_name, &world_generation, client_id, account_id)
    }

    pub(super) fn allows_entity(
        &self,
        clients: &Clients,
        entity: Entity,
        client_id: &str,
        account_id: Uuid,
    ) -> bool {
        #[cfg(test)]
        if self.test_now.is_some() {
            return self.allows(client_id, account_id);
        }
        clients.get(client_id).is_some_and(|client| {
            client.attached && client.entity == entity && self.allows(client_id, account_id)
        })
    }

    /// 返回已认证账号与同一服务时钟的观测时间。
    pub(crate) fn authorize_client(
        &self,
        world: &World,
        client_id: &str,
    ) -> Option<(Uuid, Duration)> {
        let clients = world.clients();
        let client = clients.get(client_id).filter(|client| client.attached)?;
        let principal = client.principal.as_ref()?;
        let account_id = self.parse_account_id(&principal.account_id)?;
        if !self.allows(&client.id, account_id) {
            return None;
        }
        Some((account_id, self.monotonic_now()?))
    }

    pub(crate) fn authorize_entity(
        &self,
        world: &World,
        entity: Entity,
    ) -> Option<(Uuid, Duration)> {
        let client_id = world
            .clients()
            .values()
            .find(|client| client.entity == entity)
            .map(|client| client.id.clone())?;
        self.authorize_client(world, &client_id)
    }

    pub(crate) fn monotonic_now(&self) -> Option<Duration> {
        #[cfg(test)]
        if self.test_now.is_some() {
            return self.test_now;
        }
        self.matchmaking
            .upgrade()
            .map(|service| service.monotonic_now())
    }

    fn parse_account_id(&self, value: &str) -> Option<Uuid> {
        match Uuid::parse_str(value) {
            Ok(account_id) => Some(account_id),
            Err(_) => {
                if let Some(service) = self.matchmaking.upgrade() {
                    service.fail_closed();
                }
                None
            }
        }
    }

    fn world_generation(&self, service: &MatchmakingService) -> Option<String> {
        let generations = match self.generations.lock() {
            Ok(generations) => generations,
            Err(_) => {
                service.fail_closed();
                return None;
            }
        };
        generations.get(&self.world_name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_service_denies_gameplay_and_time() {
        let authority = GameplayAuthority::new(
            Weak::new(),
            Arc::new(Mutex::new(HashMap::from([(
                "match-world".to_owned(),
                "generation-1".to_owned(),
            )]))),
            "match-world".to_owned(),
        );

        assert!(!authority.allows("player-1", Uuid::nil()));
        assert_eq!(authority.monotonic_now(), None);
    }
}
