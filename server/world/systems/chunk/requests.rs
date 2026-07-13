use hashbrown::{HashMap, HashSet};
use specs::{Join, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};

use crate::{
    ChunkInterests, ChunkProjection, ChunkProtocol, ChunkRequestsComp, ChunkStatus, Chunks,
    ClientFilter, IDComp, Mesher, Message, MessageQueues, MessageType, Pipeline, Registry, Vec2,
    WorldConfig,
};

pub struct ChunkRequestsSystem;

impl<'a> System<'a> for ChunkRequestsSystem {
    type SystemData = (
        ReadExpect<'a, Chunks>,
        ReadExpect<'a, WorldConfig>,
        ReadExpect<'a, Registry>,
        WriteExpect<'a, ChunkProjection>,
        WriteExpect<'a, ChunkInterests>,
        WriteExpect<'a, Pipeline>,
        WriteExpect<'a, Mesher>,
        WriteExpect<'a, MessageQueues>,
        ReadStorage<'a, IDComp>,
        WriteStorage<'a, ChunkRequestsComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            chunks,
            config,
            registry,
            mut projection,
            mut interests,
            mut pipeline,
            mut mesher,
            mut queue,
            ids,
            mut requests,
        ) = data;

        let max_response_per_tick = config.max_response_per_tick;

        let mut to_send: HashMap<String, HashSet<Vec2<i32>>> = HashMap::new();

        for (id, requests) in (&ids, &mut requests).join() {
            let mut to_add_back_to_requested = HashSet::new();

            for coords in requests.requests.drain(..) {
                if chunks.is_chunk_ready(&coords) {
                    let clients_to_send = to_send.entry(id.0.clone()).or_default();

                    if clients_to_send.len() >= max_response_per_tick {
                        to_add_back_to_requested.insert(coords);
                        continue;
                    }

                    clients_to_send.insert(coords.clone());
                    interests.add(&id.0, &coords);
                } else {
                    if !interests.has_interests(&coords) {
                        for coords in chunks.light_traversed_chunks(&coords) {
                            match chunks.raw(&coords) {
                                Some(chunk) if matches!(chunk.status, ChunkStatus::Meshing) => {
                                    mesher.add_chunk(&coords, false);
                                }
                                None | Some(_) => {
                                    pipeline.add_chunk(&coords, false);
                                }
                            }
                        }
                    }
                    interests.add(&id.0, &coords);
                }
            }

            requests.requests.extend(to_add_back_to_requested);
        }

        for (id, coords) in to_send {
            // 服务器网格由权威体素生成，会绕过投影。投影世界必须抑制该网格并失败关闭，
            // 客户端只能从投影后的体素载荷生成安全网格。
            let include_meshes = !config.client_only_meshing && projection.is_identity();
            let chunks: Vec<ChunkProtocol> = coords
                .into_iter()
                .filter_map(|coords| {
                    chunks.get(&coords).map(|chunk| {
                        projection.project_chunk(
                            chunk,
                            &chunks,
                            &registry,
                            include_meshes,
                            0..config.sub_chunks as u32,
                        )
                    })
                })
                .collect();

            let message = Message::new(&MessageType::Load).chunks(&chunks).build();
            queue.push((message, ClientFilter::Direct(id)));
        }
    }
}
