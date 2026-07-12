use super::{
    config::{MapPoint, OreFieldConfig},
    hash::range_i32,
};

const DIRECTIONS: [[i32; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResourceDeposit {
    pub center: MapPoint,
    horizontal_radius_x: i32,
    horizontal_radius_z: i32,
    vertical_radius: i32,
    direction: [i32; 2],
    segments: i32,
    segment_step: i32,
}

impl ResourceDeposit {
    pub fn contains(&self, point: MapPoint) -> bool {
        let half = self.segments / 2;
        (0..self.segments).any(|segment| {
            let offset = segment - half;
            let center = MapPoint::new(
                self.center.x + self.direction[0] * self.segment_step * offset,
                self.center.y,
                self.center.z + self.direction[1] * self.segment_step * offset,
            );
            inside_ellipsoid(
                point,
                center,
                self.horizontal_radius_x,
                self.vertical_radius,
                self.horizontal_radius_z,
            )
        })
    }
}

pub(super) fn build_deposits(seed: u64, config: OreFieldConfig) -> Vec<ResourceDeposit> {
    config
        .anchors
        .iter()
        .enumerate()
        .map(|(index, anchor)| {
            let offset = index * 8;
            let jitter = config.horizontal_jitter;
            let direction = DIRECTIONS[range_i32(seed, config.stream, offset + 6, 0, 3) as usize];
            ResourceDeposit {
                center: MapPoint::new(
                    anchor[0] + range_i32(seed, config.stream, offset, -jitter, jitter),
                    range_i32(seed, config.stream, offset + 1, config.min_y, config.max_y),
                    anchor[1] + range_i32(seed, config.stream, offset + 2, -jitter, jitter),
                ),
                horizontal_radius_x: range_i32(
                    seed,
                    config.stream,
                    offset + 3,
                    config.min_horizontal_radius,
                    config.max_horizontal_radius,
                ),
                horizontal_radius_z: range_i32(
                    seed,
                    config.stream,
                    offset + 4,
                    config.min_horizontal_radius,
                    config.max_horizontal_radius,
                ),
                vertical_radius: range_i32(
                    seed,
                    config.stream,
                    offset + 5,
                    config.min_vertical_radius,
                    config.max_vertical_radius,
                ),
                direction,
                segments: config.segments,
                segment_step: config.segment_step,
            }
        })
        .collect()
}

fn inside_ellipsoid(point: MapPoint, center: MapPoint, rx: i32, ry: i32, rz: i32) -> bool {
    let dx = i64::from(point.x - center.x);
    let dy = i64::from(point.y - center.y);
    let dz = i64::from(point.z - center.z);
    let rx = i64::from(rx);
    let ry = i64::from(ry);
    let rz = i64::from(rz);
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let rz2 = rz * rz;
    dx * dx * ry2 * rz2 + dy * dy * rx2 * rz2 + dz * dz * rx2 * ry2 <= rx2 * ry2 * rz2
}
