mod client_requests;
mod connections;
mod join;
mod models;
mod websocket;
mod world_lifecycle;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use actix::{
    fut::wrap_future, Actor, ActorFutureExt, Addr, AsyncContext, Context, Handler,
    Message as ActixMessage, MessageResult, WrapFuture,
};
use fern::colors::{Color, ColoredLevelConfig};
use futures_util::future::join_all;
use hashbrown::{HashMap, HashSet};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{info, warn};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{
    errors::AddWorldError,
    world::{ClientPreferencesPatch, Registry, World},
    ClientCancelJoinRequest, ClientDespawnRequest, ClientDetachOutcome, ClientDetachRequest,
    ClientJoinRequest, ClientRequest, ConnectionPrincipal, GetInfo, GetWorldStats, HttpConfig,
    Preload, Prepare, RtcSenders, SyncWorld, Tick, TransportJoinRequest, TransportLeaveRequest,
    WorldStatsResponse,
};

use connections::{DetachedConnection, PendingJoin};
pub use connections::{WsReceiver, WsSendError, WsSender};
pub use models::*;
pub(crate) use websocket::{ws_route, HandshakeConfig};
pub use world_lifecycle::{AddWorld, RemoveWorld, RemoveWorldOutcome};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnJoinRequest {
    world: String,
    username: String,
    #[serde(default, flatten)]
    flat_preferences: ClientPreferencesPatch,
    #[serde(default)]
    preferences: Option<ClientPreferencesPatch>,
}

#[derive(Serialize, Deserialize)]
struct OnActionRequest {
    action: String,
    data: Value,
}

type ServerInfoHandle = fn(&Server) -> Value;

fn default_info_handle(server: &Server) -> Value {
    let mut info = HashMap::new();

    info.insert(
        "lost_sessions".to_owned(),
        json!(server.lost_sessions.len()),
    );

    let mut connections = HashMap::new();

    for (id, (_, world, _)) in server.connections.iter() {
        connections.insert(id.to_owned(), json!(world));
    }

    info.insert("connections".to_owned(), json!(connections));

    let mut transports = vec![];

    for (id, _) in server.transport_sessions.iter() {
        transports.push(id.to_owned());
    }

    info.insert("transports".to_owned(), json!(transports));

    // for (name, world) in server.worlds.iter() {
    //     let mut world_info = HashMap::new();

    //     {
    //         let clients = world.clients();
    //         world_info.insert(
    //             "clients".to_owned(),
    //             json!(clients
    //                 .values()
    //                 .map(|client| json!({
    //                     "id": client.id.to_owned(),
    //                     "username": client.username.to_owned(),
    //                 }))
    //                 .collect::<Vec<_>>()),
    //         );
    //     }

    //     {
    //         let config = world.config();
    //         world_info.insert("config".to_owned(), json!(*config));
    //     }

    //     {
    //         let stats = world.read_resource::<Stats>();
    //         let mut stats_info = HashMap::new();

    //         stats_info.insert("tick".to_owned(), json!(stats.tick));
    //         stats_info.insert("delta".to_owned(), json!(stats.delta));

    //         world_info.insert("stats".to_owned(), json!(stats_info));
    //     }

    //     {
    //         let chunks = world.chunks();
    //         let pipeline = world.pipeline();
    //         let mesher = world.read_resource::<Mesher>();

    //         let mut generating: i32 = 0;
    //         let mut meshing: i32 = 0;
    //         let mut ready: i32 = 0;

    //         for chunk in chunks.map.values() {
    //             match chunk.status {
    //                 ChunkStatus::Generating(_) => generating += 1,
    //                 ChunkStatus::Meshing => meshing += 1,
    //                 ChunkStatus::Ready => ready += 1,
    //             }
    //         }

    //         world_info.insert(
    //             "chunks".to_owned(),
    //             json!({
    //                 "count": chunks.map.len(),
    //                 "generating": generating,
    //                 "meshing": meshing,
    //                 "ready": ready,
    //                 "pipeline_chunks": pipeline.chunks,
    //                 "pipeline_queue": pipeline.queue,
    //                 "mesher_chunks": mesher.map,
    //                 "mesher_queue": mesher.queue,
    //                 "active_voxels": chunks.active_voxels.len()
    //             }),
    //         );
    //     }

    //     {
    //         let pipeline = world.pipeline();

    //         let pipeline_info = json!({
    //             "count": json!(pipeline.chunks.len()),
    //             "stages": json!(
    //                 pipeline
    //                     .stages
    //                     .iter()
    //                     .map(|stage| json!(stage.name()))
    //                     .collect::<Vec<_>>()
    //             )
    //         });

    //         world_info.insert("pipeline".to_owned(), pipeline_info);
    //     }

    //     worlds.insert(name.to_owned(), json!(world_info));
    // }

    // info.insert("worlds".to_owned(), json!(worlds));

    serde_json::to_value(info).unwrap()
}

/// A websocket server for Voxelize, holds all worlds data, and runs as a background
/// system service.
pub struct Server {
    /// The port that this voxelize server is running on.
    pub port: u16,

    /// The address that this voxelize server is running on.
    pub addr: String,

    /// Whether or not if the socket server has started as a system service.
    pub started: bool,

    /// Static folder to serve from.
    pub serve: String,

    /// Whether the server should show debug information.
    pub debug: bool,

    /// Interval to tick the server at.
    pub interval: u64,

    /// A secret to join the server.
    pub secret: Option<String>,

    /// HTTP and WebSocket security configuration.
    pub http_config: HttpConfig,

    /// A map of all the worlds.
    pub worlds: HashMap<String, Addr<SyncWorld>>,

    /// Stable instance ID for each world name.
    world_generations: HashMap<String, String>,

    /// World names waiting for their previous actor instance to stop completely.
    removing_worlds: HashSet<String>,

    /// Registry of the server.
    pub registry: Registry,

    /// Session IDs and senders who haven't connected to a world.
    /// Value: (sender, connection_token)
    pub lost_sessions: HashMap<String, (WsSender, String)>,

    /// Transport sessions, not connected to any particular world.
    pub transport_sessions: HashMap<String, WsSender>,

    /// What world each client ID is connected to, client ID <-> world ID.
    /// Value: (sender, world_name, connection_token)
    pub connections: HashMap<String, (WsSender, String, String)>,

    /// Authenticated principal for each connection. Legacy sessions have no entry.
    pub connection_principals: HashMap<String, ConnectionPrincipal>,

    /// Sessions waiting for an atomic World join acknowledgement.
    pending_joins: HashMap<String, PendingJoin>,

    /// Explicit leaves waiting for World to confirm despawn before rejoining.
    leaving_sessions: HashMap<String, String>,

    /// World-facing client ID for each active socket connection.
    connection_client_ids: HashMap<String, String>,

    /// Detached authenticated seats keyed by account ID for later rebind.
    detached_connections: HashMap<String, DetachedConnection>,

    /// Disconnects awaiting confirmation that World retained the client entity.
    pending_detaches: HashMap<String, DetachedConnection>,

    /// Detached seats currently being rebound, keyed by account ID.
    pending_rebinds: HashMap<String, DetachedConnection>,

    /// Admitted client requests currently queued or executing per World instance.
    pending_world_requests: HashMap<String, usize>,

    /// World instance IDs with a tick already queued or running.
    pending_world_ticks: HashSet<String>,

    /// The information sent to the client when requested.
    info_handle: ServerInfoHandle,

    /// The handler for `Action`s.
    action_handles: HashMap<String, Arc<dyn Fn(Value, &mut Server)>>,

    /// WebRTC senders for hybrid networking.
    rtc_senders: Option<RtcSenders>,
}

impl Server {
    /// Create a new Voxelize server instance used to host all the worlds.
    pub fn new() -> ServerBuilder {
        ServerBuilder::new()
    }

    /// Set the RTC senders for hybrid WebSocket/WebRTC networking.
    pub fn set_rtc_senders(&mut self, rtc_senders: RtcSenders) {
        self.rtc_senders = Some(rtc_senders);
    }

    /// Get the RTC senders reference.
    pub fn rtc_senders(&self) -> Option<&RtcSenders> {
        self.rtc_senders.as_ref()
    }

    /// Add a world instance to the server. Different worlds have different configurations, and can hold
    /// their own set of clients within. If the server has already started, the added world will be
    /// started right away.
    pub fn add_world(&mut self, mut world: World) -> Result<&mut Addr<SyncWorld>, AddWorldError> {
        let name = world.name.clone();
        if self.worlds.contains_key(&name) || self.removing_worlds.contains(&name) {
            return Err(AddWorldError);
        }
        // 内部代次不信任可变的公开 World.id，避免旧异步回调命中新实例。
        let world_generation = nanoid!();
        let saving = world.config().saving;
        let save_dir = world.config().save_dir.clone();
        let preload = world.config().preload;
        world.ecs_mut().insert(self.registry.clone());

        if let Some(rtc_senders) = &self.rtc_senders {
            world.ecs_mut().insert(rtc_senders.clone());
        }

        let addr = world.start();
        if self.started {
            addr.do_send(Prepare);
            if preload {
                addr.do_send(Preload);
            }
        }
        self.worlds.insert(name.clone(), addr);
        self.world_generations
            .insert(name.clone(), world_generation);

        info!(
            "World created: {} ({})",
            name,
            if saving {
                format!("on-disk @ {}", save_dir)
            } else {
                "in-memory".to_owned()
            }
        );

        Ok(self.worlds.get_mut(&name).unwrap())
    }

    // /// Create a world in the server. Different worlds have different configurations, and can hold
    // /// their own set of clients within. If the server has already started, the added world will be
    // /// started right away.
    // pub fn create_world(
    //     &mut self,
    //     name: &str,
    //     config: &WorldConfig,
    // ) -> Result<&mut Addr<SyncWorld>, AddWorldError> {
    //     let mut world = World::new(name, config);
    //     world.ecs_mut().insert(self.registry.clone());
    //     self.add_world(world)
    // }

    /// Get a world reference by name.
    pub fn get_world(&self, world_name: &str) -> Option<&Addr<SyncWorld>> {
        self.worlds.get(world_name)
    }

    /// Get a mutable world reference by name.
    pub fn get_world_mut(&mut self, world_name: &str) -> Option<&mut Addr<SyncWorld>> {
        self.worlds.get_mut(world_name)
    }

    /// Get the information of the server
    pub fn get_info(&mut self) -> Value {
        (self.info_handle)(self)
    }

    /// Prepare all worlds on the server to start.
    pub async fn prepare(&mut self) {
        for world in self.worlds.values_mut() {
            world.do_send(Prepare);
        }
    }

    /// Preload all the worlds.
    pub async fn preload(&mut self) {
        let m = MultiProgress::new();
        let sty = ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {msg} {spinner:.green} {percent:>7}%",
        )
        .unwrap()
        .progress_chars("#>-");

        let infos: Vec<_> = join_all(self.worlds.values().map(|world| world.send(GetInfo)))
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        let mut bars = vec![];
        for (world, info) in self.worlds.values().zip(infos.iter()) {
            if !info.config.preload {
                bars.push(None);
                continue;
            }

            world.do_send(Preload);

            let bar = m.insert_from_back(0, ProgressBar::new(100));
            bar.set_message(info.name.clone());
            bar.set_style(sty.clone());
            bar.set_position(0);
            bars.push(Some(bar));
        }

        let start = Instant::now();

        loop {
            let infos: Vec<_> = join_all(self.worlds.values().map(|world| world.send(GetInfo)))
                .await
                .into_iter()
                .map(|r| r.unwrap())
                .collect();

            let mut done = true;

            for (i, (world, info)) in self.worlds.values().zip(infos.iter()).enumerate() {
                if bars[i].is_none() || !info.config.preload {
                    continue;
                }

                let bar = bars[i].as_mut().unwrap();

                if !info.preloading || info.preload_progress >= 1.0 {
                    bar.finish_and_clear();
                    continue;
                }

                let _ = world.try_send(Tick);

                let at = (info.preload_progress * 100.0) as u64;

                done = false;
                bar.set_position(at);
            }

            if done {
                m.clear().unwrap();
                break;
            }
        }

        let preload_len = infos.iter().filter(|info| info.config.preload).count();

        info!(
            "✅ Total of {} world{} preloaded in {}s",
            preload_len,
            if preload_len == 1 { "" } else { "s" },
            (Instant::now() - start).as_millis() as f64 / 1000.0
        );
    }

    /// Tick every world on this server.
    pub(crate) fn tick(&mut self) {
        for world in self.worlds.values_mut() {
            let _ = world.try_send(Tick);
        }
    }

    /// Setup Fern for debug logging.
    fn setup_logger() {
        fern::Dispatch::new()
            .format(|out, message, record| {
                let colors = ColoredLevelConfig::new().info(Color::Green);

                out.finish(format_args!(
                    "{} [{}] [{}]: {}",
                    chrono::Local::now().format("[%H:%M:%S]"),
                    colors.color(record.level()),
                    record.target(),
                    message
                ))
            })
            .level(log::LevelFilter::Debug)
            .level_for("tungstenite", log::LevelFilter::Info)
            .level_for("webrtc", log::LevelFilter::Warn)
            .level_for("webrtc_ice", log::LevelFilter::Warn)
            .level_for("webrtc_sctp", log::LevelFilter::Warn)
            .level_for("webrtc_dtls", log::LevelFilter::Warn)
            .level_for("webrtc_srtp", log::LevelFilter::Warn)
            .level_for("webrtc_data", log::LevelFilter::Warn)
            .level_for("webrtc_mdns", log::LevelFilter::Warn)
            .level_for("webrtc_util", log::LevelFilter::Warn)
            .chain(std::io::stdout())
            .apply()
            .expect("Fern did not run successfully");
    }

    pub fn set_action_handle<F: Fn(Value, &mut Server) + 'static>(
        &mut self,
        action: &str,
        handle: F,
    ) {
        self.action_handles
            .insert(action.to_lowercase(), Arc::new(handle));
    }
}

/// New chat session is created. Returns (client_id, connection_token).
#[derive(ActixMessage)]
#[rtype(result = "(String, String)")]
pub struct Connect {
    pub id: Option<String>,
    pub principal: Option<crate::ConnectionPrincipal>,
    pub is_transport: bool,
    pub sender: WsSender,
}

/// Session is disconnected
#[derive(ActixMessage)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub id: String,
    /// The connection token assigned when this session was created.
    /// Used to distinguish stale disconnects from kicked sessions.
    pub token: String,
}

#[derive(ActixMessage)]
#[rtype(result = "Value")]
pub struct Info;

#[derive(ActixMessage)]
#[rtype(result = "Vec<WorldStatsResponse>")]
pub struct GetAllWorldStats;

/// Send message to specific world
#[derive(ActixMessage)]
#[rtype(result = "Option<String>")]
pub struct ClientMessage {
    /// Id of the client session
    pub id: String,

    /// Protobuf message
    pub data: Message,
}

/// Make actor from `ChatServer`
impl Actor for Server {
    /// We are going to use simple Context, we just need ability to communicate
    /// with other actors.
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.started = true;
        ctx.run_interval(Duration::from_millis(self.interval), |act, ctx| {
            let worlds_to_tick: Vec<_> = act
                .worlds
                .iter()
                .filter_map(|(name, world)| {
                    let generation = act.world_generations.get(name)?.clone();
                    if act.pending_world_ticks.contains(&generation) {
                        None
                    } else {
                        Some((name.clone(), generation, world.clone()))
                    }
                })
                .collect();

            for (world_name, generation, world) in worlds_to_tick {
                act.pending_world_ticks.insert(generation.clone());
                ctx.spawn(
                    wrap_future(world.send(Tick)).map(move |result, act: &mut Server, _| {
                        act.pending_world_ticks.remove(&generation);
                        if let Err(error) = result {
                            warn!("World tick failed for {}: {:?}", world_name, error);
                        }
                    }),
                );
            }
        });
    }
}

/// Handler for Connect message.
///
/// Register new session and assign unique id to this session.
/// Returns (client_id, connection_token).
impl Handler<Connect> for Server {
    type Result = actix::ResponseActFuture<Self, (String, String)>;

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
        let id = if msg.principal.is_some() || msg.id.is_none() {
            nanoid!()
        } else {
            msg.id.unwrap()
        };

        let token = nanoid!();

        if msg.is_transport {
            self.worlds.values_mut().for_each(|world| {
                world.do_send(TransportJoinRequest {
                    id: id.clone(),
                    sender: msg.sender.clone(),
                })
            });

            self.transport_sessions.insert(id.to_owned(), msg.sender);

            return Box::pin(actix::fut::ready((id, token)));
        }

        if let Some(principal) = msg.principal.clone() {
            let account_id = principal.account_id.clone();
            let detached = self
                .detached_connections
                .remove(&account_id)
                .or_else(|| self.pending_detaches.remove(&account_id));

            if let Some(detached) = detached {
                let generation_matches = self
                    .world_generations
                    .get(&detached.world_name)
                    .is_some_and(|generation| generation == &detached.world_generation);

                if generation_matches {
                    if let Some(world) = self.worlds.get(&detached.world_name).cloned() {
                        let sender = msg.sender.clone();
                        let request = crate::ClientRebindRequest {
                            id: detached.client_id.clone(),
                            sender: sender.clone(),
                            principal: principal.clone(),
                        };
                        let connection_id = id.clone();
                        let connection_token = token.clone();
                        let rebind_world = world.clone();
                        let rebind_request = world.send(request);
                        self.pending_rebinds
                            .insert(account_id.clone(), detached.clone());

                        return Box::pin(rebind_request.into_actor(self).map(
                            move |result, server, _| {
                                let reservation_matches = server
                                    .pending_rebinds
                                    .get(&account_id)
                                    .is_some_and(|current| current == &detached);
                                let world_matches = server
                                    .world_generations
                                    .get(&detached.world_name)
                                    .is_some_and(|generation| {
                                        generation == &detached.world_generation
                                    });
                                if reservation_matches {
                                    server.pending_rebinds.remove(&account_id);
                                }

                                let receipt = match result {
                                    Ok(Ok(receipt)) => Some(receipt),
                                    _ => None,
                                };
                                if receipt.is_some() && reservation_matches && world_matches {
                                    server.connections.insert(
                                        connection_id.clone(),
                                        (sender, detached.world_name, connection_token.clone()),
                                    );
                                    server
                                        .connection_client_ids
                                        .insert(connection_id.clone(), detached.client_id);
                                    server
                                        .connection_principals
                                        .insert(connection_id.clone(), principal);
                                } else {
                                    if let Some(receipt) = receipt {
                                        rebind_world.do_send(ClientDespawnRequest {
                                            id: receipt.client_id,
                                            join_attempt_id: Some(receipt.join_attempt_id),
                                        });
                                    }
                                    server.lost_sessions.insert(
                                        connection_id.clone(),
                                        (sender, connection_token.clone()),
                                    );
                                    server
                                        .connection_principals
                                        .insert(connection_id.clone(), principal);
                                }

                                (connection_id, connection_token)
                            },
                        ));
                    }
                }
            }
        }

        let kick_msg = encode_message(
            &Message::new(&MessageType::Error)
                .text("Another session connected with your account.")
                .build(),
        );

        if let Some((old_sender, _old_token)) = self.lost_sessions.remove(&id) {
            info!("Kicking duplicate pre-join session: {}", id);
            let _ = old_sender.send(kick_msg.clone());
        }

        if let Some(pending) = self.pending_joins.remove(&id) {
            info!("Kicking duplicate pending session: {}", id);
            let _ = pending.sender.send(kick_msg.clone());
            if let Some(world) = self.worlds.get(&pending.world_name) {
                world.do_send(ClientCancelJoinRequest {
                    id: pending.client_id,
                    join_attempt_id: pending.attempt_id,
                });
            }
        }

        if let Some((old_sender, world_name, _old_token)) = self.connections.remove(&id) {
            info!("Kicking duplicate in-world session: {}", id);
            let _ = old_sender.send(kick_msg);
            let client_id = self
                .connection_client_ids
                .remove(&id)
                .unwrap_or_else(|| id.clone());
            if let Some(world) = self.worlds.get_mut(&world_name) {
                world.do_send(ClientDespawnRequest {
                    id: client_id,
                    join_attempt_id: None,
                });
            }
        }

        self.lost_sessions
            .insert(id.to_owned(), (msg.sender, token.clone()));

        if let Some(principal) = msg.principal {
            self.connection_principals.insert(id.clone(), principal);
        }

        Box::pin(actix::fut::ready((id, token)))
    }
}

/// Handler for Disconnect message.
/// Only cleans up session state if the connection token matches the currently
/// registered token, preventing stale disconnects from kicked sessions from
/// removing the new session's state.
impl Handler<Disconnect> for Server {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, ctx: &mut Context<Self>) {
        if let Some(pending) = self.pending_joins.get(&msg.id) {
            if pending.token == msg.token {
                let pending = self.pending_joins.remove(&msg.id).unwrap();
                if let Some(world) = self.worlds.get(&pending.world_name) {
                    world.do_send(ClientCancelJoinRequest {
                        id: pending.client_id,
                        join_attempt_id: pending.attempt_id,
                    });
                }
                self.connection_principals.remove(&msg.id);
            }
        }

        // Check connections: only remove if the token matches the current session
        if let Some((_, _, current_token)) = self.connections.get(&msg.id) {
            if *current_token == msg.token {
                let (_, world_name, _) = self.connections.remove(&msg.id).unwrap();
                let client_id = self
                    .connection_client_ids
                    .remove(&msg.id)
                    .unwrap_or_else(|| msg.id.clone());
                let principal = self.connection_principals.remove(&msg.id);
                let generation = self.world_generations.get(&world_name).cloned();
                let world = self.worlds.get(&world_name).cloned();

                match (principal, generation, world) {
                    (Some(principal), Some(world_generation), Some(world)) => {
                        let account_id = principal.account_id;
                        let reservation = DetachedConnection {
                            connection_id: msg.id.clone(),
                            world_name,
                            client_id: client_id.clone(),
                            world_generation,
                            connection_token: msg.token.clone(),
                        };
                        self.pending_detaches
                            .insert(account_id.clone(), reservation.clone());
                        let detach_world = world.clone();
                        let detach_request = world.send(ClientDetachRequest { id: client_id });
                        ctx.spawn(
                            detach_request
                                .into_actor(self)
                                .map(move |result, server, _| {
                                    let reservation_matches = server
                                        .pending_detaches
                                        .get(&account_id)
                                        .is_some_and(|current| current == &reservation);
                                    if !reservation_matches {
                                        return;
                                    }
                                    server.pending_detaches.remove(&account_id);

                                    let world_matches = server
                                        .world_generations
                                        .get(&reservation.world_name)
                                        .is_some_and(|generation| {
                                            generation == &reservation.world_generation
                                        });
                                    if matches!(result, Ok(ClientDetachOutcome::Detached))
                                        && world_matches
                                    {
                                        server.detached_connections.insert(account_id, reservation);
                                    } else if matches!(result, Ok(ClientDetachOutcome::Detached)) {
                                        detach_world.do_send(ClientDespawnRequest {
                                            id: reservation.client_id,
                                            join_attempt_id: None,
                                        });
                                    }
                                }),
                        );
                    }
                    (_, _, Some(world)) => {
                        world.do_send(ClientDespawnRequest {
                            id: client_id,
                            join_attempt_id: None,
                        });
                    }
                    _ => {}
                }
            } else {
                info!("Ignoring stale disconnect for {} (token mismatch)", msg.id);
            }
        }

        if let Some(_) = self.transport_sessions.remove(&msg.id) {
            self.worlds.values_mut().for_each(|world| {
                world.do_send(TransportLeaveRequest { id: msg.id.clone() });
            });

            info!("A transport server connection has ended.")
        }

        if self
            .leaving_sessions
            .get(&msg.id)
            .is_some_and(|token| token == &msg.token)
        {
            self.leaving_sessions.remove(&msg.id);
            self.connection_principals.remove(&msg.id);
        }

        // Check lost_sessions: only remove if the token matches
        if let Some((_, current_token)) = self.lost_sessions.get(&msg.id) {
            if *current_token == msg.token {
                self.lost_sessions.remove(&msg.id);
                self.connection_principals.remove(&msg.id);
            }
        }
    }
}

/// Handler for server info request.
impl Handler<Info> for Server {
    type Result = MessageResult<Info>;

    fn handle(&mut self, _: Info, _: &mut Context<Self>) -> Self::Result {
        MessageResult(self.get_info())
    }
}

/// Handler for getting all world stats.
impl Handler<GetAllWorldStats> for Server {
    type Result = actix::ResponseActFuture<Self, Vec<WorldStatsResponse>>;

    fn handle(&mut self, _: GetAllWorldStats, _: &mut Context<Self>) -> Self::Result {
        let world_addrs: Vec<_> = self.worlds.iter().map(|(_, addr)| addr.clone()).collect();

        Box::pin(wrap_future(async move {
            let mut stats = Vec::new();
            for addr in world_addrs {
                if let Ok(world_stats) = addr.send(GetWorldStats).await {
                    stats.push(world_stats);
                }
            }
            stats
        }))
    }
}

const DEFAULT_DEBUG: bool = true;
const DEFAULT_PORT: u16 = 4000;
const DEFAULT_ADDR: &str = "0.0.0.0";
const DEFAULT_SERVE: &str = "";
const DEFAULT_INTERVAL: u64 = 16;

/// Builder for a voxelize server.
pub struct ServerBuilder {
    port: u16,
    debug: bool,
    addr: String,
    serve: String,
    interval: u64,
    secret: Option<String>,
    http_config: HttpConfig,
    registry: Option<Registry>,
}

impl ServerBuilder {
    /// Create a new server builder instance.
    pub fn new() -> Self {
        Self {
            debug: DEFAULT_DEBUG,
            port: DEFAULT_PORT,
            addr: DEFAULT_ADDR.to_owned(),
            serve: DEFAULT_SERVE.to_owned(),
            interval: DEFAULT_INTERVAL,
            secret: None,
            http_config: HttpConfig::legacy(),
            registry: None,
        }
    }

    /// Configure the port to the voxelize server.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Configure the address of the voxelize server.
    pub fn addr(mut self, addr: &str) -> Self {
        self.addr = addr.to_owned();
        self
    }

    /// Configure whether or not the voxelize server should be in debug mode.
    pub fn debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Configure the static folder to serve.
    pub fn serve(mut self, serve: &str) -> Self {
        self.serve = serve.to_owned();
        self
    }

    /// Configure the interval for the server to tick at.
    pub fn interval(mut self, interval: u64) -> Self {
        self.interval = interval;
        self
    }

    /// Configure the secret for the server to be able to join.
    pub fn secret(mut self, secret: &str) -> Self {
        self.secret = Some(secret.to_owned());
        self
    }

    /// Configure HTTP routes, limits, origins, and connection authentication.
    pub fn http_config(mut self, config: HttpConfig) -> Self {
        self.http_config = config;
        self
    }

    /// Configure the block registry of the server. Once a registry is configured, mutating it wouldn't
    /// change the server's block list.
    pub fn registry(mut self, registry: &Registry) -> Self {
        self.registry = Some(registry.to_owned());
        self
    }

    /// Instantiate a voxelize server instance.
    pub fn build(self) -> Server {
        let mut registry = self.registry.unwrap_or(Registry::new());
        registry.generate();

        if self.debug {
            Server::setup_logger();
        }

        Server {
            port: self.port,
            addr: self.addr,
            serve: self.serve,
            debug: self.debug,
            interval: self.interval,
            secret: self.secret,
            http_config: self.http_config,

            registry,

            started: false,

            connections: HashMap::default(),
            connection_principals: HashMap::default(),
            pending_joins: HashMap::default(),
            leaving_sessions: HashMap::default(),
            connection_client_ids: HashMap::default(),
            detached_connections: HashMap::default(),
            pending_detaches: HashMap::default(),
            pending_rebinds: HashMap::default(),
            pending_world_requests: HashMap::default(),
            lost_sessions: HashMap::default(),
            transport_sessions: HashMap::default(),
            pending_world_ticks: HashSet::default(),
            worlds: HashMap::default(),
            world_generations: HashMap::default(),
            removing_worlds: HashSet::default(),
            info_handle: default_info_handle,
            action_handles: HashMap::default(),
            rtc_senders: None,
        }
    }
}
