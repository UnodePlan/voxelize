use super::*;

impl World {
    pub(crate) fn add_client(
        &mut self,
        id: &str,
        username: &str,
        sender: &WsSender,
        preferences: ClientPreferencesPatch,
        principal: Option<ConnectionPrincipal>,
        join_attempt_id: String,
    ) -> Result<ClientJoinReceipt, ClientJoinError> {
        if self.cancelled_join_attempts.remove(&join_attempt_id) {
            return Err(ClientJoinError::JoinCancelled);
        }
        let lifecycle = self.lifecycle;
        if !lifecycle.accepts_clients() {
            return Err(ClientJoinError::WorldNotReady(lifecycle));
        }

        let config = self.config();
        let max_clients = config.max_clients;
        drop(config);
        let clients = self.clients();
        if clients.contains_key(id) {
            return Err(ClientJoinError::DuplicateClient);
        }
        if clients.len() >= max_clients {
            return Err(ClientJoinError::WorldFull {
                capacity: max_clients,
            });
        }
        if let Some(principal) = &principal {
            if clients.values().any(|client| {
                client
                    .principal
                    .as_ref()
                    .is_some_and(|owner| owner.account_id == principal.account_id)
            }) {
                return Err(ClientJoinError::DuplicatePrincipal);
            }
        }
        drop(clients);

        let body =
            RigidBody::new(&AABB::new().scale_x(0.8).scale_y(1.8).scale_z(0.8).build()).build();
        let interactor = self.physics_mut().register(&body);
        let entity = self
            .ecs
            .create_entity()
            .with(ClientFlag::default())
            .with(ClientPreferencesComp(
                ClientPreferences::default().apply_patch(preferences),
            ))
            .with(IDComp::new(id))
            .with(NameComp::new(username))
            .with(AddrComp::new(sender))
            .with(ChunkRequestsComp::default())
            .with(CurrentChunkComp::default())
            .with(MetadataComp::default())
            .with(PositionComp::default())
            .with(DirectionComp::default())
            .with(RigidBodyComp::new(&body))
            .with(InteractorComp::new(&interactor))
            .with(CollisionsComp::new())
            .build();

        if let Some(modifier) = self.client_modifier.clone() {
            modifier(self, entity);
        }

        let (init_message, known_entities) = self.generate_client_init(id, entity);
        self.clients_mut().insert(
            id.to_owned(),
            Client {
                id: id.to_owned(),
                entity,
                username: username.to_owned(),
                sender: sender.clone(),
                principal,
                attached: true,
                join_attempt_id: join_attempt_id.clone(),
            },
        );
        self.entity_ids_mut().insert(id.to_owned(), entity.id());
        self.replace_known_entities(id, known_entities);
        self.send(sender, &init_message);
        self.broadcast(
            Message::new(&MessageType::Join).text(id).build(),
            ClientFilter::All,
        );

        info!("Client at {} joined the server to world: {}", id, self.name);
        Ok(ClientJoinReceipt {
            client_id: id.to_owned(),
            join_attempt_id,
        })
    }

    pub(crate) fn detach_client(&mut self, id: &str) -> ClientDetachOutcome {
        if self.config().client_disconnect_policy == ClientDisconnectPolicy::Despawn {
            return if self.remove_client(id) {
                ClientDetachOutcome::Despawned
            } else {
                ClientDetachOutcome::NotFound
            };
        }

        let Some((entity, attached)) = self
            .clients()
            .get(id)
            .map(|client| (client.entity, client.attached))
        else {
            return ClientDetachOutcome::NotFound;
        };
        if !attached {
            return ClientDetachOutcome::Detached;
        }

        if let Some(client) = self.clients_mut().get_mut(id) {
            client.attached = false;
        }
        self.write_component::<AddrComp>().remove(entity);
        self.write_component::<ChunkRequestsComp>().remove(entity);
        self.chunk_interest_mut().remove_client(id);
        self.bookkeeping_mut().remove_client(id);

        info!("Client at {} detached from world: {}", id, self.name);
        ClientDetachOutcome::Detached
    }

    pub(crate) fn rebind_client(
        &mut self,
        id: &str,
        sender: &WsSender,
        principal: &ConnectionPrincipal,
    ) -> Result<ClientJoinReceipt, ClientRebindError> {
        if !self.lifecycle.accepts_clients() {
            return Err(ClientRebindError::WorldNotReady(self.lifecycle));
        }

        let Some(client) = self.clients().get(id).cloned() else {
            return Err(ClientRebindError::NotFound);
        };
        if client.attached {
            return Err(ClientRebindError::ClientAlreadyAttached);
        }
        if client
            .principal
            .as_ref()
            .is_none_or(|owner| owner.account_id != principal.account_id)
        {
            return Err(ClientRebindError::PrincipalMismatch);
        }

        self.write_component::<AddrComp>()
            .insert(client.entity, AddrComp::new(sender))
            .map_err(|_| ClientRebindError::NotFound)?;
        self.write_component::<ChunkRequestsComp>()
            .insert(client.entity, ChunkRequestsComp::default())
            .map_err(|_| ClientRebindError::NotFound)?;
        if let Some(client) = self.clients_mut().get_mut(id) {
            client.sender = sender.clone();
            client.principal = Some(principal.clone());
            client.attached = true;
        }

        let (init_message, known_entities) = self.generate_client_init(id, client.entity);
        self.replace_known_entities(id, known_entities);
        self.send(sender, &init_message);
        info!("Client at {} rebound to world: {}", id, self.name);

        Ok(ClientJoinReceipt {
            client_id: id.to_owned(),
            join_attempt_id: client.join_attempt_id,
        })
    }

    pub(crate) fn remove_client_for_join_attempt(&mut self, id: &str, attempt_id: &str) -> bool {
        let matches = self
            .clients()
            .get(id)
            .is_some_and(|client| client.join_attempt_id == attempt_id);
        if matches {
            self.remove_client(id)
        } else {
            false
        }
    }

    pub(crate) fn cancel_join_attempt(&mut self, id: &str, attempt_id: &str) -> bool {
        if self.remove_client_for_join_attempt(id, attempt_id) {
            true
        } else {
            self.cancelled_join_attempts.insert(attempt_id.to_owned());
            false
        }
    }

    pub(crate) fn remove_client(&mut self, id: &str) -> bool {
        let removed = self.clients_mut().remove(id);
        self.entity_ids_mut().remove(id);
        self.chunk_interest_mut().remove_client(id);
        self.bookkeeping_mut().remove_client(id);

        let Some(client) = removed else {
            return false;
        };
        if let Some(handler) = self.client_leave_modifier.clone() {
            handler(self, client.entity);
        }

        let physics_handles = {
            let interactors = self.read_component::<InteractorComp>();
            interactors.get(client.entity).map(|interactor| {
                (
                    interactor.body_handle().to_owned(),
                    interactor.collider_handle().to_owned(),
                )
            })
        };
        if let Some((body, collider)) = physics_handles {
            self.physics_mut().unregister(&body, &collider);
        }

        self.write_component::<InteractorComp>()
            .remove(client.entity);
        self.write_component::<CollisionsComp>()
            .remove(client.entity);
        self.write_component::<RigidBodyComp>()
            .remove(client.entity);
        self.write_component::<ClientFlag>().remove(client.entity);
        if let Err(error) = self.ecs.entities().delete(client.entity) {
            warn!("Error deleting client entity {}: {:?}", client.id, error);
        }
        self.ecs.maintain();
        self.broadcast(
            Message::new(&MessageType::Leave).text(&client.id).build(),
            ClientFilter::All,
        );
        info!("Client at {} left the world: {}", id, self.name);
        true
    }

    pub(crate) fn stop(&mut self) -> WorldStopSummary {
        if self.lifecycle == WorldLifecycleState::Stopped {
            return WorldStopSummary {
                client_ids: Vec::new(),
            };
        }

        self.lifecycle = WorldLifecycleState::Stopping;
        let mut client_ids: Vec<_> = self.clients().keys().cloned().collect();
        client_ids.sort();
        for client_id in &client_ids {
            self.remove_client(client_id);
        }
        self.write_resource::<Transports>().clear();
        self.lifecycle = WorldLifecycleState::Stopped;

        WorldStopSummary { client_ids }
    }

    fn generate_client_init(&mut self, id: &str, entity: Entity) -> (Message, Vec<String>) {
        let position = self
            .read_component::<PositionComp>()
            .get(entity)
            .map(|value| [value.0 .0, value.0 .1, value.0 .2])
            .filter(|value| value.iter().any(|coordinate| *coordinate != 0.0));
        let direction = self
            .read_component::<DirectionComp>()
            .get(entity)
            .map(|value| [value.0 .0, value.0 .1, value.0 .2])
            .filter(|value| value.iter().any(|coordinate| *coordinate != 0.0));
        let body = self.read_component::<RigidBodyComp>();
        let flying = body
            .get(entity)
            .map(|value| value.0.gravity_multiplier == 0.0 && value.0.aabb.width() > 0.0);
        let ghost = body.get(entity).map(|value| value.0.aabb.width() <= 0.0);
        let swimming = body.get(entity).map(|value| value.0.is_swimming);
        drop(body);

        self.generate_init_message(id, position, direction, flying, ghost, swimming)
    }

    fn replace_known_entities(&mut self, id: &str, entity_ids: Vec<String>) {
        let mut bookkeeping = self.write_resource::<Bookkeeping>();
        let known = bookkeeping
            .client_known_entities
            .entry(id.to_owned())
            .or_default();
        known.clear();
        known.extend(entity_ids);
    }
}
