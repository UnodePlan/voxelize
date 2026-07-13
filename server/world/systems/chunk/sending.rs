use hashbrown::HashMap;
use specs::{ReadExpect, System, WriteExpect};
use std::collections::VecDeque;

use crate::{
    ChunkInterests, ChunkProjection, ChunkProtocol, Chunks, ClientFilter, Message, MessageQueues,
    MessageType, Registry, WorldConfig,
};

#[derive(Default)]
pub struct ChunkSendingSystem;

impl ChunkSendingSystem {
    pub fn new() -> Self {
        ChunkSendingSystem
    }
}

impl<'a> System<'a> for ChunkSendingSystem {
    type SystemData = (
        ReadExpect<'a, WorldConfig>,
        ReadExpect<'a, Registry>,
        ReadExpect<'a, ChunkInterests>,
        WriteExpect<'a, ChunkProjection>,
        WriteExpect<'a, Chunks>,
        WriteExpect<'a, MessageQueues>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (config, registry, interests, mut projection, mut chunks, mut queue) = data;

        if chunks.to_send.is_empty() {
            return;
        }

        // 服务器网格包含权威体素材质 ID。即使配置启用了服务器网格，投影世界也只发送
        // 投影后的体素数据，避免通过几何材质泄露隐藏体素。
        let send_server_meshes = !config.client_only_meshing && projection.is_identity();
        let mut to_send = VecDeque::new();
        std::mem::swap(&mut chunks.to_send, &mut to_send);

        let mut client_load_mesh: HashMap<String, Vec<ChunkProtocol>> = HashMap::new();
        let mut client_load_data: HashMap<String, Vec<ChunkProtocol>> = HashMap::new();
        let mut client_update_mesh: HashMap<String, Vec<ChunkProtocol>> = HashMap::new();
        let mut client_update_data: HashMap<String, Vec<ChunkProtocol>> = HashMap::new();

        while let Some((coords, msg_type)) = to_send.pop_front() {
            let interested_clients: Vec<String> = interests
                .get_interests(&coords)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();

            if interested_clients.is_empty() {
                continue;
            }

            if msg_type == MessageType::Load {
                let chunk = match chunks.get(&coords) {
                    Some(c) => c,
                    None => panic!("Something went wrong with sending chunks..."),
                };
                let mesh_model = send_server_meshes
                    .then(|| chunk.to_model(true, false, 0..(config.sub_chunks as u32)));
                let data_model = projection.project_chunk(
                    chunk,
                    &chunks,
                    &registry,
                    false,
                    0..(config.sub_chunks as u32),
                );

                for client_id in &interested_clients {
                    if let Some(mesh_model) = &mesh_model {
                        client_load_mesh
                            .entry(client_id.clone())
                            .or_default()
                            .push(mesh_model.clone());
                    }
                    client_load_data
                        .entry(client_id.clone())
                        .or_default()
                        .push(data_model.clone());
                }
            } else {
                let mesh_model = {
                    let chunk = match chunks.get_mut(&coords) {
                        Some(c) => c,
                        None => panic!("Something went wrong with sending chunks..."),
                    };
                    let updated_levels: Vec<u32> = chunk.updated_levels.drain().collect();
                    if updated_levels.is_empty() {
                        None
                    } else {
                        let min_level = *updated_levels.iter().min().unwrap();
                        let max_level = *updated_levels.iter().max().unwrap();
                        send_server_meshes
                            .then(|| chunk.to_model(true, false, min_level..(max_level + 1)))
                    }
                };
                if let Some(mesh_model) = mesh_model {
                    for client_id in &interested_clients {
                        client_update_mesh
                            .entry(client_id.clone())
                            .or_default()
                            .push(mesh_model.clone());
                    }
                }

                let chunk = chunks
                    .get(&coords)
                    .expect("Something went wrong with sending chunks...");
                let data_model = projection.project_chunk(chunk, &chunks, &registry, false, 0..0);
                for client_id in &interested_clients {
                    client_update_data
                        .entry(client_id.clone())
                        .or_default()
                        .push(data_model.clone());
                }
            }
        }

        for (client_id, chunk_models) in client_load_mesh {
            if !chunk_models.is_empty() {
                queue.push((
                    Message::new(&MessageType::Load)
                        .chunks(&chunk_models)
                        .build(),
                    ClientFilter::Direct(client_id),
                ));
            }
        }

        for (client_id, chunk_models) in client_load_data {
            if !chunk_models.is_empty() {
                queue.push((
                    Message::new(&MessageType::Load)
                        .chunks(&chunk_models)
                        .build(),
                    ClientFilter::Direct(client_id),
                ));
            }
        }

        for (client_id, chunk_models) in client_update_mesh {
            if !chunk_models.is_empty() {
                queue.push((
                    Message::new(&MessageType::Update)
                        .chunks(&chunk_models)
                        .build(),
                    ClientFilter::Direct(client_id),
                ));
            }
        }

        for (client_id, chunk_models) in client_update_data {
            if !chunk_models.is_empty() {
                queue.push((
                    Message::new(&MessageType::Update)
                        .chunks(&chunk_models)
                        .build(),
                    ClientFilter::Direct(client_id),
                ));
            }
        }
    }
}
