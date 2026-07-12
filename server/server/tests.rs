use crate::WorldConfig;
use futures_util::future::{join, join_all};

use super::*;

fn ws_sender() -> (WsSender, WsReceiver) {
    WsSender::channel(64)
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
