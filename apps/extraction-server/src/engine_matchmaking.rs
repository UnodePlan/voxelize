use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use actix::Addr;
use async_trait::async_trait;
use tokio::time::{sleep, timeout, Duration};
use uuid::Uuid;
use voxelize::{
    AddWorld, ClientAttachKind, ClientDisconnectPolicy, DespawnDetachedPrincipal, PrepareWorld,
    RemoveWorld, Server, World, WorldConfig, WorldLifecycleState, WorldRequestPolicy,
};

use crate::{
    engine_catalog::EngineCatalog,
    engine_gameplay::{install_gameplay_runtime, GameplayAuthority},
    engine_movement::install_bounded_movement,
    generation::{install_generation_stage, install_spawn_assignment, GenerationPlan},
    match_world::{engine_seed_v1, MatchWorldMetadata},
    matchmaking::{MatchAttachKind, MatchmakingService},
    ports::{MatchWorldRuntime, MatchWorldRuntimeError, MatchWorldSpec, PreparedMatchWorld},
};

const WORLD_PRELOAD_RADIUS: usize = 10;
const WORLD_READY_TIMEOUT: Duration = Duration::from_secs(60);
const WORLD_READY_POLL_INTERVAL: Duration = Duration::from_millis(25);
const WORLD_ROLLBACK_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct EngineMatchWorldRuntime {
    server: Addr<Server>,
    matchmaking: Weak<MatchmakingService>,
    generations: Arc<Mutex<HashMap<String, String>>>,
    owned_matches: Mutex<HashMap<String, Uuid>>,
    catalog: Arc<EngineCatalog>,
}

impl EngineMatchWorldRuntime {
    pub(crate) fn new(
        server: Addr<Server>,
        matchmaking: Weak<MatchmakingService>,
        catalog: Arc<EngineCatalog>,
    ) -> Self {
        Self {
            server,
            matchmaking,
            generations: Arc::new(Mutex::new(HashMap::new())),
            owned_matches: Mutex::new(HashMap::new()),
            catalog,
        }
    }

    async fn wait_until_ready(&self, world_name: &str) -> Result<String, MatchWorldRuntimeError> {
        let wait = async {
            let mut expected_generation = None;
            loop {
                let prepared = self
                    .server
                    .send(PrepareWorld {
                        name: world_name.to_owned(),
                        expected_generation: expected_generation.clone(),
                    })
                    .await
                    .map_err(|_| MatchWorldRuntimeError::Unavailable)?
                    .map_err(|_| MatchWorldRuntimeError::Unavailable)?;
                if prepared.lifecycle == WorldLifecycleState::Ready {
                    return Ok(prepared.generation);
                }
                if prepared.lifecycle != WorldLifecycleState::Preparing {
                    return Err(MatchWorldRuntimeError::Unavailable);
                }
                expected_generation = Some(prepared.generation);
                sleep(WORLD_READY_POLL_INTERVAL).await;
            }
        };
        timeout(WORLD_READY_TIMEOUT, wait)
            .await
            .map_err(|_| MatchWorldRuntimeError::Unavailable)?
    }

    async fn rollback_world(&self, world_name: &str) {
        let remove = self.server.send(RemoveWorld {
            name: world_name.to_owned(),
        });
        let _ = timeout(WORLD_ROLLBACK_TIMEOUT, remove).await;
        self.generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
        self.owned_matches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
    }
}

#[async_trait]
impl MatchWorldRuntime for EngineMatchWorldRuntime {
    async fn prepare_world(
        &self,
        spec: MatchWorldSpec,
    ) -> Result<PreparedMatchWorld, MatchWorldRuntimeError> {
        let plan = Arc::new(
            GenerationPlan::new(
                spec.seed,
                &spec.generation_version,
                &spec.config_version,
                self.catalog.resources(),
            )
            .map_err(|_| MatchWorldRuntimeError::Conflict)?,
        );
        let config = WorldConfig::new()
            .max_clients(spec.roster.iter().len())
            .request_policy(
                WorldRequestPolicy::strict()
                    .allow_method("pvp:v1:drop-slot")
                    .allow_method("pvp:v1:mining")
                    .allow_method("pvp:v1:get-state"),
            )
            .client_disconnect_policy(ClientDisconnectPolicy::Detach)
            .min_chunk(spec.engine_min_chunk)
            .max_chunk(spec.engine_max_chunk)
            .preload(true)
            .preload_radius(WORLD_PRELOAD_RADIUS)
            .saving(spec.saving)
            .seed(engine_seed_v1(spec.seed))
            .max_height(plan.config().max_height)
            .water_level(0)
            .build();
        let mut world = World::new(&spec.world_name, &config);
        world.set_item_registry(self.catalog.items().clone());
        install_generation_stage(&mut world, plan.clone());
        install_spawn_assignment(&mut world, &spec.roster, plan.layout());
        world.ecs_mut().insert(spec.playable_bounds);
        world.ecs_mut().insert(spec.roster.clone());
        world.ecs_mut().insert(plan.resources());
        world.ecs_mut().insert(plan.layout().selected_extraction());
        let gameplay_authority = GameplayAuthority::new(
            self.matchmaking.clone(),
            self.generations.clone(),
            spec.world_name.clone(),
        );
        install_gameplay_runtime(&mut world, &spec, gameplay_authority)
            .map_err(|_| MatchWorldRuntimeError::Conflict)?;
        world.ecs_mut().insert(MatchWorldMetadata {
            match_id: spec.match_id,
            seed: plan.seed(),
            engine_seed: engine_seed_v1(spec.seed),
            generation_version: spec.generation_version,
            gameplay_version: spec.gameplay_version,
            config_version: spec.config_version,
            catalog_version: self.catalog.resources().catalog_version,
            loadout: spec.loadout,
        });
        install_bounded_movement(
            &mut world,
            spec.playable_bounds,
            self.matchmaking.clone(),
            self.generations.clone(),
            spec.world_name.clone(),
        );

        let matchmaking = self.matchmaking.clone();
        let generations = self.generations.clone();
        let world_name = spec.world_name.clone();
        world.set_client_attach_guard(move |request| {
            let Some(service) = matchmaking.upgrade() else {
                return false;
            };
            let Some(principal) = request.principal.as_ref() else {
                return false;
            };
            let Ok(account_id) = Uuid::parse_str(&principal.account_id) else {
                service.fail_closed();
                return false;
            };
            let kind = match request.kind {
                ClientAttachKind::Join => MatchAttachKind::Join,
                ClientAttachKind::Rebind => MatchAttachKind::Rebind,
            };
            let Some(world_generation) = generations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&world_name)
                .cloned()
            else {
                return false;
            };
            service.allows_attach(
                &world_name,
                &world_generation,
                &request.client_id,
                &request.attach_attempt_id,
                account_id,
                kind,
            )
        });

        self.server
            .send(AddWorld { world })
            .await
            .map_err(|_| MatchWorldRuntimeError::Unavailable)?
            .map_err(|_| MatchWorldRuntimeError::Conflict)?;
        self.owned_matches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(spec.world_name.clone(), spec.match_id);
        let generation = match self.wait_until_ready(&spec.world_name).await {
            Ok(generation) => generation,
            Err(error) => {
                self.rollback_world(&spec.world_name).await;
                return Err(error);
            }
        };
        self.generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(spec.world_name.clone(), generation.clone());
        Ok(PreparedMatchWorld {
            world_generation: generation,
        })
    }

    async fn stop_world(
        &self,
        match_id: Uuid,
        world_name: &str,
    ) -> Result<bool, MatchWorldRuntimeError> {
        let owns_world = self
            .owned_matches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(world_name)
            .is_some_and(|owned_match_id| *owned_match_id == match_id);
        if !owns_world {
            return Ok(false);
        }
        let outcome = self
            .server
            .send(RemoveWorld {
                name: world_name.to_owned(),
            })
            .await
            .map_err(|_| MatchWorldRuntimeError::Unavailable)?
            .map_err(|_| MatchWorldRuntimeError::Unavailable)?;
        self.generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
        self.owned_matches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
        Ok(outcome.removed)
    }

    async fn despawn_detached(
        &self,
        world_name: &str,
        account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError> {
        let Some(world_generation) = self
            .generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(world_name)
            .cloned()
        else {
            return Ok(false);
        };
        self.server
            .send(DespawnDetachedPrincipal {
                account_id: account_id.to_string(),
                world_name: world_name.to_owned(),
                world_generation,
            })
            .await
            .map_err(|_| MatchWorldRuntimeError::Unavailable)
    }
}
