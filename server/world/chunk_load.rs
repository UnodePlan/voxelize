use log::warn;
use serde::{Deserialize, Serialize};

use crate::{ChunkRequestsComp, ChunkUtils, DirectionComp, PositionComp, Vec2, World};

/// Controls whether client-selected chunk coordinates are trusted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum ChunkLoadPolicy {
    /// Preserves the historical client-directed loading behavior.
    #[default]
    Legacy,
    /// Uses the server-owned player position as the center and bounds every request.
    AuthoritativeRadius { max_chunk_radius: u32 },
}

impl ChunkLoadPolicy {
    pub const fn legacy() -> Self {
        Self::Legacy
    }

    pub const fn authoritative_radius(max_chunk_radius: u32) -> Self {
        Self::AuthoritativeRadius { max_chunk_radius }
    }

    fn authoritative_center(
        self,
        position: Option<&PositionComp>,
        chunk_size: usize,
    ) -> Option<Vec2<i32>> {
        match self {
            Self::Legacy => None,
            Self::AuthoritativeRadius { .. } => {
                let position = &position?.0;
                if chunk_size == 0 || !position.0.is_finite() || !position.2.is_finite() {
                    return None;
                }
                Some(ChunkUtils::map_voxel_to_chunk(
                    position.0.floor() as i32,
                    0,
                    position.2.floor() as i32,
                    chunk_size,
                ))
            }
        }
    }

    fn allows(
        self,
        authoritative_center: &Vec2<i32>,
        requested: &Vec2<i32>,
        min_chunk: [i32; 2],
        max_chunk: [i32; 2],
    ) -> bool {
        match self {
            Self::Legacy => true,
            Self::AuthoritativeRadius { max_chunk_radius } => {
                if requested.0 < min_chunk[0]
                    || requested.0 > max_chunk[0]
                    || requested.1 < min_chunk[1]
                    || requested.1 > max_chunk[1]
                {
                    return false;
                }

                let dx = i128::from(requested.0) - i128::from(authoritative_center.0);
                let dz = i128::from(requested.1) - i128::from(authoritative_center.1);
                let radius = i128::from(max_chunk_radius);
                dx * dx + dz * dz <= radius * radius
            }
        }
    }
}

#[derive(Deserialize)]
struct OnLoadRequest {
    center: Vec2<i32>,
    direction: Vec2<f32>,
    chunks: Vec<Vec2<i32>>,
}

impl World {
    pub(super) fn on_load(&mut self, client_id: &str, data: crate::Message) {
        let Some(client_entity) = self.clients().get(client_id).map(|client| client.entity) else {
            return;
        };
        let request: OnLoadRequest = match serde_json::from_str(&data.json) {
            Ok(request) => request,
            Err(error) => {
                warn!("Rejected malformed chunk LOAD payload: {error}");
                return;
            }
        };
        if request.chunks.is_empty() {
            return;
        }

        let (policy, chunk_size, min_chunk, max_chunk) = {
            let config = self.config();
            (
                config.chunk_load_policy,
                config.chunk_size,
                config.min_chunk,
                config.max_chunk,
            )
        };
        let (center, direction) = match policy {
            ChunkLoadPolicy::Legacy => (request.center, request.direction),
            ChunkLoadPolicy::AuthoritativeRadius { .. } => {
                // PVP 模式只使用服务端 ECS 中的位置和朝向，客户端 center/direction 仅作兼容字段。
                let positions = self.read_component::<PositionComp>();
                let Some(center) =
                    policy.authoritative_center(positions.get(client_entity), chunk_size)
                else {
                    warn!("Rejected chunk LOAD without a finite authoritative position");
                    return;
                };
                drop(positions);
                let directions = self.read_component::<DirectionComp>();
                let direction = directions
                    .get(client_entity)
                    .map(|value| Vec2(value.0 .0, value.0 .2))
                    .filter(|value| value.0.is_finite() && value.1.is_finite())
                    .unwrap_or(Vec2(0.0, 0.0));
                (center, direction)
            }
        };

        let accepted: Vec<_> = request
            .chunks
            .into_iter()
            .filter(|coords| policy.allows(&center, coords, min_chunk, max_chunk))
            .collect();
        if accepted.is_empty() {
            return;
        }

        let mut requests = self.write_component::<ChunkRequestsComp>();
        let Some(requests) = requests.get_mut(client_entity) else {
            warn!("Client entity has no ChunkRequestsComp: {client_id}");
            return;
        };
        for coords in &accepted {
            requests.add(coords);
        }
        requests.set_center(&center);
        requests.set_direction(&direction);
        requests.sort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_policy_uses_euclidean_radius_and_world_bounds() {
        let policy = ChunkLoadPolicy::authoritative_radius(6);
        let center = Vec2(-1, 2);

        assert!(policy.allows(&center, &Vec2(5, 2), [-10, -10], [9, 9]));
        assert!(policy.allows(&center, &Vec2(2, 7), [-10, -10], [9, 9]));
        assert!(!policy.allows(&center, &Vec2(5, 3), [-10, -10], [9, 9]));
        assert!(!policy.allows(&center, &Vec2(-11, 2), [-10, -10], [9, 9]));
    }

    #[test]
    fn authoritative_center_handles_negative_voxel_coordinates() {
        let policy = ChunkLoadPolicy::authoritative_radius(6);
        let position = PositionComp::new(-0.1, 10.0, -16.1);

        assert_eq!(
            policy.authoritative_center(Some(&position), 16),
            Some(Vec2(-1, -2))
        );
        assert_eq!(policy.authoritative_center(None, 16), None);
        assert_eq!(
            policy.authoritative_center(Some(&PositionComp::new(f32::NAN, 0.0, 0.0)), 16),
            None
        );
    }
}
