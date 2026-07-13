use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use crate::{ClientAttachKind, WorldConfig};
use futures_util::future::{join, join_all};

use super::*;

fn ws_sender() -> (WsSender, WsReceiver) {
    WsSender::channel(64)
}

#[derive(Clone, Default)]
struct RecordingObserver {
    events: Arc<Mutex<Vec<ConnectionLifecycleEvent>>>,
}

impl ConnectionLifecycleObserver for RecordingObserver {
    fn observe(&self, event: &ConnectionLifecycleEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

impl RecordingObserver {
    fn snapshot(&self) -> Vec<ConnectionLifecycleEvent> {
        self.events.lock().unwrap().clone()
    }

    async fn wait_for_len(&self, expected: usize) -> Vec<ConnectionLifecycleEvent> {
        for _ in 0..100 {
            let events = self.snapshot();
            if events.len() >= expected {
                return events;
            }
            actix::clock::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("connection lifecycle observer did not receive {expected} events");
    }
}

#[derive(actix::Message)]
#[rtype(result = "bool")]
struct DisconnectPendingRebind {
    account_id: String,
}

impl Handler<DisconnectPendingRebind> for Server {
    type Result = bool;

    fn handle(&mut self, message: DisconnectPendingRebind, context: &mut Context<Self>) -> bool {
        let Some((connection_id, token)) =
            self.pending_rebinds
                .get(&message.account_id)
                .map(|pending| {
                    (
                        pending.connection_id.clone(),
                        pending.connection_token.clone(),
                    )
                })
        else {
            return false;
        };
        <Server as Handler<Disconnect>>::handle(
            self,
            Disconnect {
                id: connection_id,
                token,
            },
            context,
        );
        true
    }
}

async fn prepared_server(config: WorldConfig) -> (Addr<Server>, Addr<SyncWorld>) {
    let mut server = Server::new().debug(false).build();
    server.add_world(World::new("arena", &config)).unwrap();
    server.prepare().await;
    let world = server.get_world("arena").unwrap().clone();
    let info = world.send(GetInfo).await.unwrap();
    assert_eq!(info.lifecycle, crate::WorldLifecycleState::Ready);
    (server.start(), world)
}

#[actix::test]
async fn authenticated_client_id_resolver_controls_lifecycle_and_world_routing() {
    let observer = RecordingObserver::default();
    let resolver_calls = Arc::new(Mutex::new(Vec::new()));
    let routed_client_id = Arc::new(Mutex::new(None));
    let resolver_calls_for_handler = resolver_calls.clone();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .authenticated_client_id_resolver(
            move |world_name: &str, principal: &ConnectionPrincipal| {
                resolver_calls_for_handler
                    .lock()
                    .unwrap()
                    .push((world_name.to_owned(), principal.clone()));
                Some("public-player-7".to_owned())
            },
        )
        .build();
    let mut world = World::new("arena", &WorldConfig::default());
    let routed_client_id_for_handler = routed_client_id.clone();
    world.set_method_handle("test:route", move |_, client_id, _| {
        *routed_client_id_for_handler.lock().unwrap() = Some(client_id.to_owned());
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let server = server.start();
    let principal = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, _receiver) = ws_sender();
    let (connection_id, _) = server
        .send(Connect {
            id: None,
            principal: Some(principal.clone()),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();

    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Method)
                    .method(MethodProtocol {
                        name: "test:route".to_owned(),
                        payload: "{}".to_owned(),
                    })
                    .build(),
            })
            .await
            .unwrap(),
        None
    );

    assert_eq!(
        *resolver_calls.lock().unwrap(),
        vec![("arena".to_owned(), principal.clone())]
    );
    assert_eq!(
        routed_client_id.lock().unwrap().as_deref(),
        Some("public-player-7")
    );
    let events = observer.wait_for_len(2).await;
    assert!(matches!(
        &events[1],
        ConnectionLifecycleEvent::JoinCommitted {
            connection_id: actual_connection,
            principal: Some(actual_principal),
            world_name,
            client_id,
            ..
        } if actual_connection == &connection_id
            && actual_principal == &principal
            && world_name == "arena"
            && client_id == "public-player-7"
    ));
}

#[actix::test]
async fn unavailable_authenticated_client_id_rejects_before_world_admission_but_keeps_legacy() {
    let admission_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut server = Server::new()
        .debug(false)
        .authenticated_client_id_resolver(|_: &str, _: &ConnectionPrincipal| None)
        .build();
    let mut world = World::new("arena", &WorldConfig::default());
    let admission_calls_for_guard = admission_calls.clone();
    world.set_client_attach_guard(move |_| {
        admission_calls_for_guard.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let world = server.get_world("arena").unwrap().clone();
    let server = server.start();
    let join = || {
        Message::new(&MessageType::Join)
            .json(r#"{"world":"arena","username":"Player"}"#)
            .build()
    };
    let (sender, _receiver) = ws_sender();
    let (authenticated_connection, _) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();

    assert_eq!(
        server
            .send(ClientMessage {
                id: authenticated_connection.clone(),
                data: join(),
            })
            .await
            .unwrap(),
        Some("Client admission was denied.".to_owned())
    );
    assert_eq!(
        server
            .send(ClientMessage {
                id: authenticated_connection,
                data: join(),
            })
            .await
            .unwrap(),
        Some("Client admission was denied.".to_owned())
    );
    assert_eq!(admission_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 0);

    let (legacy_sender, _legacy_receiver) = ws_sender();
    let (legacy_connection, _) = server
        .send(Connect {
            id: Some("legacy-player".to_owned()),
            principal: None,
            is_transport: false,
            sender: legacy_sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: legacy_connection,
                data: join(),
            })
            .await
            .unwrap(),
        None
    );
    assert_eq!(admission_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 1);
}

#[actix::test]
async fn running_server_accepts_and_prepares_dynamic_world() {
    let server = Server::new().debug(false).build().start();
    server
        .send(AddWorld {
            world: World::new("dynamic", &WorldConfig::default()),
        })
        .await
        .unwrap()
        .unwrap();

    let (sender, _receiver) = ws_sender();
    let (connection_id, _) = server
        .send(Connect {
            id: Some("dynamic-player".to_owned()),
            principal: None,
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    let result = server
        .send(ClientMessage {
            id: connection_id,
            data: Message::new(&MessageType::Join)
                .json(r#"{"world":"dynamic","username":"Player"}"#)
                .build(),
        })
        .await
        .unwrap();

    assert_eq!(result, None);
    assert!(server
        .send(AddWorld {
            world: World::new("dynamic", &WorldConfig::default()),
        })
        .await
        .unwrap()
        .is_err());
    assert!(
        server
            .send(RemoveWorld {
                name: "dynamic".to_owned(),
            })
            .await
            .unwrap()
            .unwrap()
            .removed
    );
}

#[actix::test]
async fn prepare_world_reports_ready_generation_and_rejects_stale_generation() {
    let server = Server::new().debug(false).build().start();
    server
        .send(AddWorld {
            world: World::new("dynamic", &WorldConfig::default()),
        })
        .await
        .unwrap()
        .unwrap();

    let prepared = server
        .send(PrepareWorld {
            name: "dynamic".to_owned(),
            expected_generation: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(prepared.lifecycle, crate::WorldLifecycleState::Ready);
    assert!(!prepared.generation.is_empty());

    let confirmed = server
        .send(PrepareWorld {
            name: "dynamic".to_owned(),
            expected_generation: Some(prepared.generation.clone()),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(confirmed, prepared);

    assert!(
        server
            .send(RemoveWorld {
                name: "dynamic".to_owned(),
            })
            .await
            .unwrap()
            .unwrap()
            .removed
    );
    server
        .send(AddWorld {
            world: World::new("dynamic", &WorldConfig::default()),
        })
        .await
        .unwrap()
        .unwrap();

    let error = server
        .send(PrepareWorld {
            name: "dynamic".to_owned(),
            expected_generation: Some(prepared.generation.clone()),
        })
        .await
        .unwrap()
        .unwrap_err();
    let PrepareWorldError::GenerationMismatch { expected, actual } = error else {
        panic!("reused world name did not reject the old generation");
    };
    assert_eq!(expected, prepared.generation);
    assert!(actual.is_some_and(|actual| actual != expected));
}

#[actix::test]
async fn observer_emits_ordered_lifecycle_events_and_ignores_stale_disconnect() {
    let observer = RecordingObserver::default();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    server.add_world(World::new("arena", &config)).unwrap();
    server.prepare().await;
    let server = server.start();
    let principal = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, _receiver) = ws_sender();
    let (connection_id, token) = server
        .send(Connect {
            id: None,
            principal: Some(principal.clone()),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();

    server
        .send(Disconnect {
            id: connection_id.clone(),
            token: "stale-token".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(observer.snapshot().len(), 1);

    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    server
        .send(Disconnect {
            id: connection_id.clone(),
            token: token.clone(),
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(4).await;
    assert!(matches!(
        &events[0],
        ConnectionLifecycleEvent::Connected {
            connection_id: actual,
            principal: Some(actual_principal),
        } if actual == &connection_id && actual_principal == &principal
    ));
    let (world_generation, client_id, attach_attempt_id) = match &events[1] {
        ConnectionLifecycleEvent::JoinCommitted {
            connection_id: actual,
            principal: Some(actual_principal),
            world_name,
            world_generation,
            client_id,
            attach_attempt_id,
        } => {
            assert_eq!(actual, &connection_id);
            assert_eq!(actual_principal, &principal);
            assert_eq!(world_name, "arena");
            (
                world_generation.clone(),
                client_id.clone(),
                attach_attempt_id.clone(),
            )
        }
        event => panic!("unexpected join event: {event:?}"),
    };
    assert!(matches!(
        &events[2],
        ConnectionLifecycleEvent::Disconnected {
            connection_id: actual,
            principal: Some(actual_principal),
            world_name: Some(world_name),
            world_generation: Some(actual_generation),
            client_id: Some(actual_client),
            attach_attempt_id: Some(actual_attempt),
        } if actual == &connection_id
            && actual_principal == &principal
            && world_name == "arena"
            && actual_generation == &world_generation
            && actual_client == &client_id
            && actual_attempt == &attach_attempt_id
    ));
    assert!(matches!(
        &events[3],
        ConnectionLifecycleEvent::Detached {
            connection_id: actual,
            principal: actual_principal,
            world_name,
            world_generation: actual_generation,
            client_id: actual_client,
            attach_attempt_id: actual_attempt,
        } if actual == &connection_id
            && actual_principal == &principal
            && world_name == "arena"
            && actual_generation == &world_generation
            && actual_client == &client_id
            && actual_attempt == &attach_attempt_id
    ));

    server
        .send(Disconnect {
            id: connection_id,
            token,
        })
        .await
        .unwrap();
    assert_eq!(observer.snapshot().len(), 4);

    let (replacement_sender, _replacement_receiver) = ws_sender();
    let (replacement_connection, replacement_attempt_id) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-2")),
            is_transport: false,
            sender: replacement_sender,
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(6).await;
    assert!(matches!(
        events[4],
        ConnectionLifecycleEvent::Connected { .. }
    ));
    assert!(matches!(
        &events[5],
        ConnectionLifecycleEvent::Rebound {
            connection_id: actual,
            principal: actual_principal,
            world_name,
            world_generation: actual_generation,
            client_id: actual_client,
            attach_attempt_id: actual_attempt,
        } if actual == &replacement_connection
            && actual_principal.account_id == "account-1"
            && actual_principal.session_id == "session-2"
            && world_name == "arena"
            && actual_generation == &world_generation
            && actual_client == &client_id
            && actual_attempt == &replacement_attempt_id
    ));
}

#[actix::test]
async fn denied_rebind_restores_detached_reservation_for_generation_safe_cleanup() {
    let observer = RecordingObserver::default();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    let mut world = World::new("arena", &config);
    world.set_client_attach_guard(|request| request.kind == ClientAttachKind::Join);
    server.add_world(world).unwrap();
    server.prepare().await;
    let world = server.get_world("arena").unwrap().clone();
    let server = server.start();
    let prepared = server
        .send(PrepareWorld {
            name: "arena".to_owned(),
            expected_generation: None,
        })
        .await
        .unwrap()
        .unwrap();
    let (sender, _receiver) = ws_sender();
    let (connection_id, token) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    server
        .send(Disconnect {
            id: connection_id,
            token,
        })
        .await
        .unwrap();
    observer.wait_for_len(4).await;

    let (replacement, _replacement_receiver) = ws_sender();
    let (_, rejected_attempt_id) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-2")),
            is_transport: false,
            sender: replacement,
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(6).await;
    let original_client_id = match &events[1] {
        ConnectionLifecycleEvent::JoinCommitted { client_id, .. } => client_id,
        event => panic!("unexpected initial join event: {event:?}"),
    };
    assert!(matches!(
        &events[4],
        ConnectionLifecycleEvent::Connected {
            principal: Some(principal),
            ..
        } if principal.account_id == "account-1" && principal.session_id == "session-2"
    ));
    assert!(matches!(
        &events[5],
        ConnectionLifecycleEvent::RebindRejected {
            principal,
            world_name,
            world_generation,
            client_id,
            attach_attempt_id,
        } if principal.account_id == "account-1"
            && principal.session_id == "session-2"
            && world_name == "arena"
            && world_generation == &prepared.generation
            && client_id == original_client_id
            && attach_attempt_id == &rejected_attempt_id
    ));

    assert!(!server
        .send(DespawnDetachedPrincipal {
            account_id: "account-1".to_owned(),
            world_name: "arena".to_owned(),
            world_generation: "stale-generation".to_owned(),
        })
        .await
        .unwrap());
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 1);
    assert!(server
        .send(DespawnDetachedPrincipal {
            account_id: "account-1".to_owned(),
            world_name: "arena".to_owned(),
            world_generation: prepared.generation.clone(),
        })
        .await
        .unwrap());
    assert!(!server
        .send(DespawnDetachedPrincipal {
            account_id: "account-1".to_owned(),
            world_name: "arena".to_owned(),
            world_generation: prepared.generation,
        })
        .await
        .unwrap());
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 0);
}

#[actix::test]
async fn stale_generation_rebind_receipt_emits_rejected_lifecycle_event() {
    let observer = RecordingObserver::default();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    let mut world = World::new("arena", &config);
    world.set_client_attach_guard(|request| {
        if request.kind == ClientAttachKind::Rebind {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        true
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let server = server.start();
    let (sender, _receiver) = ws_sender();
    let (connection_id, token) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    server
        .send(Disconnect {
            id: connection_id,
            token,
        })
        .await
        .unwrap();
    let initial_events = observer.wait_for_len(4).await;
    let (world_generation, client_id) = match &initial_events[1] {
        ConnectionLifecycleEvent::JoinCommitted {
            world_generation,
            client_id,
            ..
        } => (world_generation.clone(), client_id.clone()),
        event => panic!("unexpected initial join event: {event:?}"),
    };

    let (replacement, _replacement_receiver) = ws_sender();
    let reconnect = server.send(Connect {
        id: None,
        principal: Some(ConnectionPrincipal::new("account-1", "session-2")),
        is_transport: false,
        sender: replacement,
    });
    let remove = async {
        actix::clock::sleep(std::time::Duration::from_millis(1)).await;
        server
            .send(RemoveWorld {
                name: "arena".to_owned(),
            })
            .await
    };
    let (reconnect, removed) = join(reconnect, remove).await;

    let (_, rejected_attempt_id) = reconnect.unwrap();
    assert!(removed.unwrap().unwrap().removed);
    let events = observer.wait_for_len(6).await;
    assert!(matches!(
        &events[5],
        ConnectionLifecycleEvent::RebindRejected {
            principal,
            world_name,
            world_generation: actual_generation,
            client_id: actual_client_id,
            attach_attempt_id,
        } if principal.account_id == "account-1"
            && principal.session_id == "session-2"
            && world_name == "arena"
            && actual_generation == &world_generation
            && actual_client_id == &client_id
            && attach_attempt_id == &rejected_attempt_id
    ));
}

#[actix::test]
async fn eviction_is_generation_safe_and_returns_online_session_to_lobby() {
    let observer = RecordingObserver::default();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    server
        .add_world(World::new("arena", &WorldConfig::default()))
        .unwrap();
    server.prepare().await;
    let world = server.worlds.get("arena").unwrap().clone();
    let server = server.start();
    let (sender, _receiver) = ws_sender();
    let (connection_id, _) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    let events = observer.wait_for_len(2).await;
    let generation = match &events[1] {
        ConnectionLifecycleEvent::JoinCommitted {
            world_generation, ..
        } => world_generation.clone(),
        event => panic!("unexpected join event: {event:?}"),
    };

    assert!(!server
        .send(EvictMatchPrincipal {
            account_id: "account-1".to_owned(),
            world_name: "arena".to_owned(),
            world_generation: "stale-generation".to_owned(),
        })
        .await
        .unwrap());
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 1);

    assert!(server
        .send(EvictMatchPrincipal {
            account_id: "account-1".to_owned(),
            world_name: "arena".to_owned(),
            world_generation: generation,
        })
        .await
        .unwrap());
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 0);

    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id,
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 1);
}

#[actix::test]
async fn rejected_rebind_attempt_is_distinct_from_the_successful_retry() {
    let observer = RecordingObserver::default();
    let rebind_attempts = Arc::new(Mutex::new(Vec::new()));
    let rebind_count = Arc::new(AtomicUsize::new(0));
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    let mut world = World::new("arena", &config);
    let attempts_for_guard = rebind_attempts.clone();
    let count_for_guard = rebind_count.clone();
    world.set_client_attach_guard(move |request| {
        if request.kind != ClientAttachKind::Rebind {
            return true;
        }
        attempts_for_guard
            .lock()
            .unwrap()
            .push(request.attach_attempt_id.clone());
        count_for_guard.fetch_add(1, Ordering::SeqCst) > 0
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let server = server.start();
    let (sender, _receiver) = ws_sender();
    let (connection_id, token) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    server
        .send(Disconnect {
            id: connection_id,
            token,
        })
        .await
        .unwrap();
    observer.wait_for_len(4).await;

    let (first_sender, _first_receiver) = ws_sender();
    let (_, first_attempt_id) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-2")),
            is_transport: false,
            sender: first_sender,
        })
        .await
        .unwrap();
    let first_events = observer.wait_for_len(6).await;
    assert!(matches!(
        &first_events[5],
        ConnectionLifecycleEvent::RebindRejected {
            attach_attempt_id,
            ..
        } if attach_attempt_id == &first_attempt_id
    ));

    let (second_sender, _second_receiver) = ws_sender();
    let (_, second_attempt_id) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-3")),
            is_transport: false,
            sender: second_sender,
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(8).await;

    assert_ne!(first_attempt_id, second_attempt_id);
    assert_eq!(
        *rebind_attempts.lock().unwrap(),
        vec![first_attempt_id.clone(), second_attempt_id.clone()]
    );
    assert!(matches!(
        &events[7],
        ConnectionLifecycleEvent::Rebound {
            principal,
            attach_attempt_id,
            ..
        } if principal.session_id == "session-3" && attach_attempt_id == &second_attempt_id
    ));
}

#[actix::test]
async fn disconnect_before_rebind_guard_completion_detaches_the_same_attempt_and_retries() {
    let observer = RecordingObserver::default();
    let rebind_count = Arc::new(AtomicUsize::new(0));
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    let mut world = World::new("arena", &config);
    let rebind_count_for_guard = rebind_count.clone();
    world.set_client_attach_guard(move |request| {
        if request.kind == ClientAttachKind::Rebind
            && rebind_count_for_guard.fetch_add(1, Ordering::SeqCst) == 0
        {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        true
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let world = server.get_world("arena").unwrap().clone();
    let server = server.start();
    let (sender, _receiver) = ws_sender();
    let (connection_id, token) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    server
        .send(Disconnect {
            id: connection_id,
            token,
        })
        .await
        .unwrap();
    observer.wait_for_len(4).await;

    let (replacement, _replacement_receiver) = ws_sender();
    let reconnect = server.send(Connect {
        id: None,
        principal: Some(ConnectionPrincipal::new("account-1", "session-2")),
        is_transport: false,
        sender: replacement,
    });
    let disconnect_pending = async {
        actix::clock::sleep(std::time::Duration::from_millis(1)).await;
        server
            .send(DisconnectPendingRebind {
                account_id: "account-1".to_owned(),
            })
            .await
    };
    let (reconnect, disconnected) = join(reconnect, disconnect_pending).await;
    let (_, interrupted_attempt_id) = reconnect.unwrap();
    assert!(disconnected.unwrap());
    let events = observer.wait_for_len(7).await;

    assert!(matches!(
        &events[5],
        ConnectionLifecycleEvent::Disconnected {
            attach_attempt_id: Some(attach_attempt_id),
            ..
        } if attach_attempt_id == &interrupted_attempt_id
    ));
    assert!(matches!(
        &events[6],
        ConnectionLifecycleEvent::Detached {
            attach_attempt_id,
            ..
        } if attach_attempt_id == &interrupted_attempt_id
    ));
    assert!(!events.iter().any(|event| matches!(
        event,
        ConnectionLifecycleEvent::Rebound { attach_attempt_id, .. }
            | ConnectionLifecycleEvent::RebindRejected { attach_attempt_id, .. }
            if attach_attempt_id == &interrupted_attempt_id
    )));
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 1);

    let (retry_sender, _retry_receiver) = ws_sender();
    let (_, retry_attempt_id) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-3")),
            is_transport: false,
            sender: retry_sender,
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(9).await;
    assert_ne!(interrupted_attempt_id, retry_attempt_id);
    assert!(matches!(
        &events[8],
        ConnectionLifecycleEvent::Rebound {
            principal,
            attach_attempt_id,
            ..
        } if principal.session_id == "session-3" && attach_attempt_id == &retry_attempt_id
    ));
}

#[actix::test]
async fn despawn_revokes_pending_rebind_receipt_with_the_same_attempt() {
    let observer = RecordingObserver::default();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    let mut world = World::new("arena", &config);
    world.set_client_attach_guard(|request| {
        if request.kind == ClientAttachKind::Rebind {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        true
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let world = server.get_world("arena").unwrap().clone();
    let server = server.start();
    let (sender, _receiver) = ws_sender();
    let (connection_id, token) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("account-1", "session-1")),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: connection_id.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    server
        .send(Disconnect {
            id: connection_id,
            token,
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(4).await;
    let world_generation = match &events[1] {
        ConnectionLifecycleEvent::JoinCommitted {
            world_generation, ..
        } => world_generation.clone(),
        event => panic!("unexpected initial join event: {event:?}"),
    };

    let (replacement, _replacement_receiver) = ws_sender();
    let reconnect = server.send(Connect {
        id: None,
        principal: Some(ConnectionPrincipal::new("account-1", "session-2")),
        is_transport: false,
        sender: replacement,
    });
    let despawn = async {
        actix::clock::sleep(std::time::Duration::from_millis(1)).await;
        server
            .send(DespawnDetachedPrincipal {
                account_id: "account-1".to_owned(),
                world_name: "arena".to_owned(),
                world_generation: world_generation.clone(),
            })
            .await
    };
    let (reconnect, despawned) = join(reconnect, despawn).await;
    let (_, revoked_attempt_id) = reconnect.unwrap();

    assert!(despawned.unwrap());
    let events = observer.wait_for_len(6).await;
    assert!(matches!(
        &events[5],
        ConnectionLifecycleEvent::RebindRejected {
            principal,
            world_generation: actual_generation,
            attach_attempt_id,
            ..
        } if principal.session_id == "session-2"
            && actual_generation == &world_generation
            && attach_attempt_id == &revoked_attempt_id
    ));
    for _ in 0..100 {
        if world.send(GetWorldStats).await.unwrap().client_count == 0 {
            return;
        }
        actix::clock::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("revoked rebind client was not despawned");
}

#[actix::test]
async fn cleanup_keeps_pending_rebind_token_until_disconnect_is_observed() {
    let observer = RecordingObserver::default();
    let mut server = Server::new()
        .debug(false)
        .connection_lifecycle_observer(observer.clone())
        .build();
    server
        .add_world(World::new("arena", &WorldConfig::default()))
        .unwrap();
    server.prepare().await;
    let generation = server.world_generations.get("arena").unwrap().clone();
    let principal = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, _receiver) = ws_sender();
    server.pending_rebinds.insert(
        principal.account_id.clone(),
        PendingRebind {
            detached: DetachedConnection {
                connection_id: "old-connection".to_owned(),
                world_name: "arena".to_owned(),
                client_id: "client-1".to_owned(),
                attach_attempt_id: "old-attach".to_owned(),
                world_generation: generation.clone(),
                connection_token: "old-token".to_owned(),
            },
            connection_id: "new-connection".to_owned(),
            connection_token: "new-token".to_owned(),
            sender,
            principal: principal.clone(),
            disconnected: false,
            despawn_requested: false,
        },
    );
    let server = server.start();

    assert!(server
        .send(DespawnDetachedPrincipal {
            account_id: principal.account_id.clone(),
            world_name: "arena".to_owned(),
            world_generation: generation.clone(),
        })
        .await
        .unwrap());
    assert!(!server
        .send(DespawnDetachedPrincipal {
            account_id: principal.account_id.clone(),
            world_name: "arena".to_owned(),
            world_generation: generation.clone(),
        })
        .await
        .unwrap());

    server
        .send(Disconnect {
            id: "new-connection".to_owned(),
            token: "new-token".to_owned(),
        })
        .await
        .unwrap();
    let events = observer.wait_for_len(1).await;
    assert!(matches!(
        &events[0],
        ConnectionLifecycleEvent::Disconnected {
            connection_id,
            principal: Some(actual_principal),
            world_name: Some(world_name),
            world_generation: Some(actual_generation),
            client_id: Some(client_id),
            attach_attempt_id: Some(attach_attempt_id),
        } if connection_id == "new-connection"
            && actual_principal == &principal
            && world_name == "arena"
            && actual_generation == &generation
            && client_id == "client-1"
            && attach_attempt_id == "new-token"
    ));
}

#[actix::test]
async fn server_commits_only_world_accepted_joins() {
    let (server, world) = prepared_server(WorldConfig::new().max_clients(1).build()).await;
    let mut receivers = Vec::new();
    let mut connection_ids = Vec::new();

    for id in ["first", "second"] {
        let (sender, receiver) = ws_sender();
        receivers.push(receiver);
        let (connection_id, _) = server
            .send(Connect {
                id: Some(id.to_owned()),
                principal: None,
                is_transport: false,
                sender,
            })
            .await
            .unwrap();
        connection_ids.push(connection_id);
    }

    let requests = connection_ids.iter().map(|connection_id| {
        server.send(ClientMessage {
            id: connection_id.clone(),
            data: Message::new(&MessageType::Join)
                .json(r#"{"world":"arena","username":"Player"}"#)
                .build(),
        })
    });
    let results = join_all(requests).await;
    let accepted = results
        .iter()
        .filter(|result| matches!(result, Ok(None)))
        .count();
    let rejected = results
        .iter()
        .filter(|result| matches!(result, Ok(Some(message)) if message == "World is full."))
        .count();
    let stats = world.send(GetWorldStats).await.unwrap();

    assert_eq!(accepted, 1);
    assert_eq!(rejected, 1);
    assert_eq!(stats.client_count, 1);
    drop(receivers);
}

#[actix::test]
async fn join_requests_share_the_bounded_world_admission_limit() {
    let mut server = Server::new()
        .debug(false)
        .http_config(HttpConfig::legacy().world_request_capacity(1))
        .build();
    let mut world = World::new("arena", &WorldConfig::default());
    world.set_client_modifier(|_, _| {
        std::thread::sleep(std::time::Duration::from_millis(25));
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let server = server.start();
    let mut receivers = Vec::new();
    let mut connection_ids = Vec::new();
    for id in ["first", "second"] {
        let (sender, receiver) = ws_sender();
        receivers.push(receiver);
        connection_ids.push(
            server
                .send(Connect {
                    id: Some(id.to_owned()),
                    principal: None,
                    is_transport: false,
                    sender,
                })
                .await
                .unwrap()
                .0,
        );
    }
    let join_message = |id: String| ClientMessage {
        id,
        data: Message::new(&MessageType::Join)
            .json(r#"{"world":"arena","username":"Player"}"#)
            .build(),
    };

    let first = server.send(join_message(connection_ids[0].clone()));
    let second = server.send(join_message(connection_ids[1].clone()));
    let (first, second) = join(first, second).await;

    assert_eq!(first.unwrap(), None);
    assert_eq!(
        second.unwrap(),
        Some("World is busy, please reconnect.".to_owned())
    );
    drop(receivers);
}

#[actix::test]
async fn cancelled_join_callback_cannot_commit_or_remove_a_retry() {
    let mut server = Server::new().debug(false).build();
    let mut world = World::new("arena", &WorldConfig::default());
    world.set_client_modifier(|_, _| {
        std::thread::sleep(std::time::Duration::from_millis(25));
    });
    server.add_world(world).unwrap();
    server.prepare().await;
    let world = server.get_world("arena").unwrap().clone();
    let server = server.start();
    let (sender, _receiver) = ws_sender();
    let (connection_id, _) = server
        .send(Connect {
            id: Some("retry-player".to_owned()),
            principal: None,
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    let join_message = || ClientMessage {
        id: connection_id.clone(),
        data: Message::new(&MessageType::Join)
            .json(r#"{"world":"arena","username":"Player"}"#)
            .build(),
    };

    let first = server.send(join_message());
    let leave = server.send(ClientMessage {
        id: connection_id.clone(),
        data: Message::new(&MessageType::Leave).build(),
    });
    let (first, leave) = join(first, leave).await;
    let retry = server.send(join_message()).await;

    assert_eq!(first.unwrap(), Some("Join was cancelled.".to_owned()));
    assert_eq!(leave.unwrap(), None);
    assert_eq!(retry.unwrap(), None);
    assert_eq!(world.send(GetWorldStats).await.unwrap().client_count, 1);
}

#[actix::test]
async fn authenticated_disconnect_rebinds_existing_world_client() {
    let config = WorldConfig::new()
        .client_disconnect_policy(crate::ClientDisconnectPolicy::Detach)
        .build();
    let (server, world) = prepared_server(config).await;
    let principal = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, mut first_receiver) = ws_sender();
    let (first_connection, first_token) = server
        .send(Connect {
            id: Some("forged-client-id".to_owned()),
            principal: Some(principal.clone()),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();
    assert_ne!(first_connection, "forged-client-id");

    assert_eq!(
        server
            .send(ClientMessage {
                id: first_connection.clone(),
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );
    let first_init = crate::decode_message(&first_receiver.recv().await.unwrap()).unwrap();
    let public_player_id = serde_json::from_str::<serde_json::Value>(&first_init.json).unwrap()
        ["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(public_player_id, first_connection);
    assert_ne!(public_player_id, "account-1");
    let (new_sender, mut new_receiver) = ws_sender();
    let disconnect = server.send(Disconnect {
        id: first_connection.clone(),
        token: first_token,
    });
    let reconnect = server.send(Connect {
        id: Some("another-forged-id".to_owned()),
        principal: Some(principal),
        is_transport: false,
        sender: new_sender,
    });
    let (disconnect, reconnect) = join(disconnect, reconnect).await;
    disconnect.unwrap();
    let (second_connection, _) = reconnect.unwrap();
    let stats = world.send(GetWorldStats).await.unwrap();
    let rebound_init = crate::decode_message(&new_receiver.recv().await.unwrap()).unwrap();
    let rebound_player_id = serde_json::from_str::<serde_json::Value>(&rebound_init.json).unwrap()
        ["id"]
        .as_str()
        .unwrap()
        .to_owned();

    assert_ne!(second_connection, first_connection);
    assert_ne!(second_connection, "another-forged-id");
    assert_eq!(rebound_player_id, public_player_id);
    assert_eq!(stats.client_count, 1);
}

#[actix::test]
async fn close_authenticated_session_actor_message_only_targets_matching_session() {
    let (server, _) = prepared_server(WorldConfig::default()).await;
    let (target_sender, target_receiver) = ws_sender();
    let (target_connection, _) = server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("target-account", "target-session")),
            is_transport: false,
            sender: target_sender,
        })
        .await
        .unwrap();
    assert_eq!(
        server
            .send(ClientMessage {
                id: target_connection,
                data: Message::new(&MessageType::Join)
                    .json(r#"{"world":"arena","username":"Ignored"}"#)
                    .build(),
            })
            .await
            .unwrap(),
        None
    );

    let (other_sender, other_receiver) = ws_sender();
    server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new("other-account", "other-session")),
            is_transport: false,
            sender: other_sender,
        })
        .await
        .unwrap();
    let (legacy_sender, legacy_receiver) = ws_sender();
    server
        .send(Connect {
            id: Some("legacy-player".to_owned()),
            principal: None,
            is_transport: false,
            sender: legacy_sender,
        })
        .await
        .unwrap();

    let closed = server
        .send(CloseAuthenticatedSession {
            session_id: "target-session".to_owned(),
        })
        .await
        .unwrap();

    assert_eq!(closed, 1);
    assert!(target_receiver.policy_close_requested());
    assert!(!other_receiver.policy_close_requested());
    assert!(!legacy_receiver.policy_close_requested());
}

#[actix::test]
async fn close_authenticated_session_preserves_state_until_disconnect_cleanup() {
    let mut server = Server::new().debug(false).build();
    let target = ConnectionPrincipal::new("target-account", "target-session");

    let (lost_sender, lost_receiver) = ws_sender();
    server.lost_sessions.insert(
        "target-lost".to_owned(),
        (lost_sender, "lost-token".to_owned()),
    );
    server
        .connection_principals
        .insert("target-lost".to_owned(), target.clone());

    let (pending_sender, pending_receiver) = ws_sender();
    server.pending_joins.insert(
        "target-pending".to_owned(),
        PendingJoin {
            sender: pending_sender,
            world_name: "arena".to_owned(),
            token: "pending-token".to_owned(),
            client_id: "pending-client".to_owned(),
            attempt_id: "attempt".to_owned(),
            world_generation: "generation".to_owned(),
        },
    );
    server
        .connection_principals
        .insert("target-pending".to_owned(), target.clone());

    let (world_sender, world_receiver) = ws_sender();
    server.connections.insert(
        "target-world".to_owned(),
        (world_sender, "arena".to_owned(), "world-token".to_owned()),
    );
    server
        .connection_principals
        .insert("target-world".to_owned(), target.clone());

    let (leaving_sender, leaving_receiver) = ws_sender();
    server.leaving_sessions.insert(
        "target-leaving".to_owned(),
        LeavingSession {
            sender: leaving_sender,
            token: "leaving-token".to_owned(),
        },
    );
    server
        .connection_principals
        .insert("target-leaving".to_owned(), target.clone());

    let (rebind_sender, rebind_receiver) = ws_sender();
    server.pending_rebinds.insert(
        "target-account".to_owned(),
        PendingRebind {
            detached: DetachedConnection {
                connection_id: "detached-connection".to_owned(),
                world_name: "arena".to_owned(),
                client_id: "detached-client".to_owned(),
                attach_attempt_id: "detached-attach".to_owned(),
                world_generation: "generation".to_owned(),
                connection_token: "detached-token".to_owned(),
            },
            connection_id: "target-rebinding".to_owned(),
            connection_token: "rebind-token".to_owned(),
            sender: rebind_sender,
            principal: target,
            disconnected: false,
            despawn_requested: false,
        },
    );

    let (other_sender, other_receiver) = ws_sender();
    server
        .lost_sessions
        .insert("other".to_owned(), (other_sender, "other-token".to_owned()));
    server.connection_principals.insert(
        "other".to_owned(),
        ConnectionPrincipal::new("other-account", "other-session"),
    );
    let (legacy_sender, legacy_receiver) = ws_sender();
    server.lost_sessions.insert(
        "legacy".to_owned(),
        (legacy_sender, "legacy-token".to_owned()),
    );

    assert_eq!(
        server.close_authenticated_session_sockets("target-session"),
        5
    );
    server.finish_leave("target-leaving", "leaving-token");
    assert_eq!(
        server.close_authenticated_session_sockets("target-session"),
        0
    );
    assert!(lost_receiver.policy_close_requested());
    assert!(pending_receiver.policy_close_requested());
    assert!(world_receiver.policy_close_requested());
    assert!(leaving_receiver.policy_close_requested());
    assert!(rebind_receiver.policy_close_requested());
    assert!(!other_receiver.policy_close_requested());
    assert!(!legacy_receiver.policy_close_requested());

    assert_eq!(server.lost_sessions.len(), 4);
    assert!(server.leaving_sessions.is_empty());
    assert_eq!(server.pending_joins.len(), 1);
    assert_eq!(server.pending_rebinds.len(), 1);
    assert_eq!(server.connections.len(), 1);
    assert_eq!(server.connection_principals.len(), 5);
}

#[actix::test]
async fn remove_world_keeps_pending_rebind_socket_routable_until_callback() {
    let mut server = Server::new().debug(false).build();
    server
        .add_world(World::new("arena", &WorldConfig::default()))
        .unwrap();
    server.prepare().await;

    let (rebind_sender, rebind_receiver) = ws_sender();
    server.pending_rebinds.insert(
        "target-account".to_owned(),
        PendingRebind {
            detached: DetachedConnection {
                connection_id: "detached-connection".to_owned(),
                world_name: "arena".to_owned(),
                client_id: "detached-client".to_owned(),
                attach_attempt_id: "detached-attach".to_owned(),
                world_generation: "generation".to_owned(),
                connection_token: "detached-token".to_owned(),
            },
            connection_id: "target-rebinding".to_owned(),
            connection_token: "rebind-token".to_owned(),
            sender: rebind_sender,
            principal: ConnectionPrincipal::new("target-account", "target-session"),
            disconnected: false,
            despawn_requested: false,
        },
    );
    let server = server.start();

    let removed = server
        .send(RemoveWorld {
            name: "arena".to_owned(),
        })
        .await
        .unwrap()
        .unwrap();
    let closed = server
        .send(CloseAuthenticatedSession {
            session_id: "target-session".to_owned(),
        })
        .await
        .unwrap();

    assert!(removed.removed);
    assert_eq!(closed, 1);
    assert!(rebind_receiver.policy_close_requested());
}

#[actix::test]
async fn remove_world_is_idempotent() {
    let (server, old_world) = prepared_server(WorldConfig::default()).await;

    let first = server
        .send(RemoveWorld {
            name: "arena".to_owned(),
        })
        .await
        .unwrap()
        .unwrap();
    let second = server
        .send(RemoveWorld {
            name: "arena".to_owned(),
        })
        .await
        .unwrap()
        .unwrap();

    assert!(first.removed);
    assert!(!second.removed);
    for _ in 0..10 {
        if old_world.send(GetInfo).await.is_err() {
            return;
        }
        actix::clock::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("removed World actor still accepts messages");
}
