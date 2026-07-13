use std::ops::Range;

use hashbrown::{HashMap, HashSet};

use crate::{
    BlockUtils, Chunk, ChunkProtocol, ChunkUtils, Chunks, Registry, UpdateProtocol, Vec3,
    VoxelAccess,
};

const FACE_NEIGHBORS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// 将单个世界的权威 Chunk 体素投影为客户端安全视图。
///
/// 默认空映射保持原值。游戏可以把敏感体素 ID 映射成不透明宿主体素 ID；
/// 此资源不会修改权威 `Chunks` 存储。
#[derive(Clone, Debug, Default)]
pub struct ChunkProjection {
    replacements: HashMap<u32, u32>,
    revealed: HashSet<Vec3<i32>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkProjectionError {
    InvalidVoxelId,
    SameVoxelId,
    DuplicateHiddenVoxel,
}

impl ChunkProjection {
    pub fn obfuscating(
        replacements: impl IntoIterator<Item = (u32, u32)>,
    ) -> Result<Self, ChunkProjectionError> {
        let mut mapped = HashMap::new();
        for (hidden, host) in replacements {
            if hidden == 0 || host == 0 {
                return Err(ChunkProjectionError::InvalidVoxelId);
            }
            if hidden == host {
                return Err(ChunkProjectionError::SameVoxelId);
            }
            if mapped.insert(hidden, host).is_some() {
                return Err(ChunkProjectionError::DuplicateHiddenVoxel);
            }
        }
        Ok(Self {
            replacements: mapped,
            revealed: HashSet::new(),
        })
    }

    pub fn is_identity(&self) -> bool {
        self.replacements.is_empty()
    }

    pub fn is_revealed(&self, voxel: &Vec3<i32>) -> bool {
        self.revealed.contains(voxel)
    }

    pub fn project_chunk(
        &mut self,
        chunk: &Chunk,
        chunks: &Chunks,
        registry: &Registry,
        include_meshes: bool,
        levels: Range<u32>,
    ) -> ChunkProtocol {
        let mut model = chunk.to_model(include_meshes, true, levels);
        if self.is_identity() {
            return model;
        }

        let Some(voxels) = model.voxels.as_mut() else {
            return model;
        };
        let Vec3(min_x, min_y, min_z) = chunk.min;
        for lx in 0..chunk.options.size {
            for ly in 0..chunk.options.max_height {
                for lz in 0..chunk.options.size {
                    let index = [lx, ly, lz];
                    let raw = voxels[&index];
                    if self.replacement(raw).is_none() {
                        continue;
                    }
                    let position = Vec3(min_x + lx as i32, min_y + ly as i32, min_z + lz as i32);
                    if self.revealed.contains(&position)
                        || Self::is_exposed(
                            &position,
                            chunks,
                            registry,
                            chunk.options.size,
                            chunk.options.max_height as i32,
                        )
                    {
                        self.revealed.insert(position);
                    } else {
                        voxels[&index] = self.project_raw(&position, raw);
                    }
                }
            }
        }
        model
    }

    pub(crate) fn project_updates(
        &mut self,
        updates: Vec<UpdateProtocol>,
        chunks: &Chunks,
        registry: &Registry,
        chunk_size: usize,
        max_height: i32,
    ) -> Vec<UpdateProtocol> {
        if self.is_identity() {
            return updates;
        }

        let updated_positions: HashSet<_> = updates
            .iter()
            .map(|update| Vec3(update.vx, update.vy, update.vz))
            .collect();
        let mut newly_revealed = HashSet::new();

        // 只检查最终权威状态仍为 Air 的更新。这里在 Chunks 应用完整批次后执行，
        // 避免同一 tick 中后续不透明写入已覆盖 Air，却仍错误揭示相邻矿石。
        for update in &updates {
            if !registry.is_air(BlockUtils::extract_id(update.voxel)) {
                continue;
            }
            let update_coords =
                ChunkUtils::map_voxel_to_chunk(update.vx, update.vy, update.vz, chunk_size);
            if !chunks.is_chunk_ready(&update_coords)
                || !registry.is_air(BlockUtils::extract_id(
                    chunks.get_raw_voxel(update.vx, update.vy, update.vz),
                ))
            {
                continue;
            }
            for [ox, oy, oz] in FACE_NEIGHBORS {
                let neighbor = Vec3(update.vx + ox, update.vy + oy, update.vz + oz);
                if neighbor.1 < 0 || neighbor.1 >= max_height {
                    continue;
                }
                let coords =
                    ChunkUtils::map_voxel_to_chunk(neighbor.0, neighbor.1, neighbor.2, chunk_size);
                if !chunks.is_chunk_ready(&coords) {
                    continue;
                }
                let raw = chunks.get_raw_voxel(neighbor.0, neighbor.1, neighbor.2);
                if self.replacement(raw).is_some() && self.revealed.insert(neighbor.clone()) {
                    newly_revealed.insert(neighbor);
                }
            }
        }

        for update in &updates {
            let position = Vec3(update.vx, update.vy, update.vz);
            if self.replacement(update.voxel).is_some()
                && Self::is_exposed(&position, chunks, registry, chunk_size, max_height)
            {
                self.revealed.insert(position);
            }
        }

        let mut projected = updates
            .into_iter()
            .map(|mut update| {
                let position = Vec3(update.vx, update.vy, update.vz);
                update.voxel = self.project_raw(&position, update.voxel);
                update
            })
            .collect::<Vec<_>>();

        let mut newly_revealed = newly_revealed.into_iter().collect::<Vec<_>>();
        newly_revealed.sort_by_key(|voxel| (voxel.0, voxel.1, voxel.2));
        projected.extend(newly_revealed.into_iter().filter_map(|voxel| {
            if updated_positions.contains(&voxel) {
                return None;
            }
            Some(UpdateProtocol {
                vx: voxel.0,
                vy: voxel.1,
                vz: voxel.2,
                voxel: chunks.get_raw_voxel(voxel.0, voxel.1, voxel.2),
                light: chunks.get_raw_light(voxel.0, voxel.1, voxel.2),
            })
        }));
        projected
    }

    fn replacement(&self, raw: u32) -> Option<u32> {
        self.replacements.get(&BlockUtils::extract_id(raw)).copied()
    }

    fn project_raw(&self, position: &Vec3<i32>, raw: u32) -> u32 {
        if self.revealed.contains(position) {
            return raw;
        }
        self.replacement(raw)
            .map_or(raw, |host| BlockUtils::insert_id(raw, host))
    }

    fn is_exposed(
        position: &Vec3<i32>,
        chunks: &Chunks,
        registry: &Registry,
        chunk_size: usize,
        max_height: i32,
    ) -> bool {
        FACE_NEIGHBORS.into_iter().any(|[ox, oy, oz]| {
            let neighbor = Vec3(position.0 + ox, position.1 + oy, position.2 + oz);
            if neighbor.1 >= max_height {
                return true;
            }
            if neighbor.1 < 0 {
                return false;
            }
            let coords =
                ChunkUtils::map_voxel_to_chunk(neighbor.0, neighbor.1, neighbor.2, chunk_size);
            if !chunks.is_chunk_ready(&coords) {
                return false;
            }
            let raw = chunks.get_raw_voxel(neighbor.0, neighbor.1, neighbor.2);
            let block = registry.get_block_by_id(BlockUtils::extract_id(raw));
            let transparency = block.get_rotated_transparency(
                &chunks.get_voxel_rotation(neighbor.0, neighbor.1, neighbor.2),
            );
            face_towards_source_is_transparent(&transparency, ox, oy, oz)
        })
    }
}

fn face_towards_source_is_transparent(transparency: &[bool; 6], dx: i32, dy: i32, dz: i32) -> bool {
    match (dx, dy, dz) {
        (1, 0, 0) => transparency[3],
        (-1, 0, 0) => transparency[0],
        (0, 1, 0) => transparency[4],
        (0, -1, 0) => transparency[1],
        (0, 0, 1) => transparency[5],
        (0, 0, -1) => transparency[2],
        _ => false,
    }
}
