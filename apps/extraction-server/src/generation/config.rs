#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct MapPoint {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl MapPoint {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OreFieldConfig {
    pub anchors: &'static [[i32; 2]],
    pub horizontal_jitter: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_horizontal_radius: i32,
    pub max_horizontal_radius: i32,
    pub min_vertical_radius: i32,
    pub max_vertical_radius: i32,
    pub segments: i32,
    pub segment_step: i32,
    pub stream: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GenerationConfig {
    pub generation_version: &'static str,
    pub config_version: &'static str,
    pub min_xz: i32,
    pub max_xz_exclusive: i32,
    pub max_height: usize,
    pub surface_y: i32,
    pub unbreakable_floor_y: i32,
    pub spawn_points: &'static [MapPoint; 10],
    pub extraction_candidates: &'static [MapPoint; 8],
    pub(super) gold: OreFieldConfig,
    pub(super) diamond: OreFieldConfig,
}

impl GenerationConfig {
    pub(crate) fn resolve(generation_version: &str, config_version: &str) -> Option<&'static Self> {
        match (generation_version, config_version) {
            ("generation-v1", "balance-v1") => Some(&GENERATION_V1),
            ("generation-v2", "balance-v2") => Some(&GENERATION_V2),
            _ => None,
        }
    }

    pub(crate) fn contains_xz(self, x: i32, z: i32) -> bool {
        x >= self.min_xz
            && x < self.max_xz_exclusive
            && z >= self.min_xz
            && z < self.max_xz_exclusive
    }
}

// 金矿位于相邻出生点的角平分线上，避免某些席位天然离热点更近。
const GOLD_ANCHORS: [[i32; 2]; 5] = [[74, 24], [0, 78], [-74, 24], [-46, -63], [46, -63]];
const DIAMOND_ANCHORS: [[i32; 2]; 3] = [[0, -32], [28, 16], [-28, 16]];

const SPAWN_POINTS: [MapPoint; 10] = [
    MapPoint::new(125, 50, 0),
    MapPoint::new(101, 50, 73),
    MapPoint::new(39, 50, 119),
    MapPoint::new(-39, 50, 119),
    MapPoint::new(-101, 50, 73),
    MapPoint::new(-125, 50, 0),
    MapPoint::new(-101, 50, -73),
    MapPoint::new(-39, 50, -119),
    MapPoint::new(39, 50, -119),
    MapPoint::new(101, 50, -73),
];

const EXTRACTION_CANDIDATES: [MapPoint; 8] = [
    MapPoint::new(0, 50, -16),
    MapPoint::new(11, 50, -11),
    MapPoint::new(16, 50, 0),
    MapPoint::new(11, 50, 11),
    MapPoint::new(0, 50, 16),
    MapPoint::new(-11, 50, 11),
    MapPoint::new(-16, 50, 0),
    MapPoint::new(-11, 50, -11),
];

pub(crate) const GENERATION_V1: GenerationConfig = GenerationConfig {
    generation_version: "generation-v1",
    config_version: "balance-v1",
    min_xz: -150,
    max_xz_exclusive: 150,
    max_height: 64,
    surface_y: 48,
    unbreakable_floor_y: 0,
    spawn_points: &SPAWN_POINTS,
    extraction_candidates: &EXTRACTION_CANDIDATES,
    gold: OreFieldConfig {
        anchors: &GOLD_ANCHORS,
        horizontal_jitter: 8,
        min_y: 27,
        max_y: 34,
        min_horizontal_radius: 12,
        max_horizontal_radius: 12,
        min_vertical_radius: 5,
        max_vertical_radius: 5,
        segments: 3,
        segment_step: 3,
        stream: 0x474f_4c44_5f56_3101,
    },
    diamond: OreFieldConfig {
        anchors: &DIAMOND_ANCHORS,
        horizontal_jitter: 4,
        min_y: 10,
        max_y: 14,
        min_horizontal_radius: 7,
        max_horizontal_radius: 7,
        min_vertical_radius: 3,
        max_vertical_radius: 3,
        segments: 3,
        segment_step: 2,
        stream: 0x4449_414d_5f56_3101,
    },
};

// 新版本只能通过显式版本对启用；生产 manifest 仍指向已冻结的 V1。
pub(crate) const GENERATION_V2: GenerationConfig = GenerationConfig {
    generation_version: "generation-v2",
    config_version: "balance-v2",
    min_xz: -150,
    max_xz_exclusive: 150,
    max_height: 64,
    surface_y: 48,
    unbreakable_floor_y: 0,
    spawn_points: &SPAWN_POINTS,
    extraction_candidates: &EXTRACTION_CANDIDATES,
    gold: OreFieldConfig {
        anchors: &GOLD_ANCHORS,
        horizontal_jitter: 10,
        min_y: 22,
        max_y: 28,
        min_horizontal_radius: 9,
        max_horizontal_radius: 11,
        min_vertical_radius: 4,
        max_vertical_radius: 6,
        segments: 5,
        segment_step: 3,
        stream: 0x474f_4c44_5f56_3201,
    },
    diamond: OreFieldConfig {
        anchors: &DIAMOND_ANCHORS,
        horizontal_jitter: 5,
        min_y: 6,
        max_y: 10,
        min_horizontal_radius: 5,
        max_horizontal_radius: 8,
        min_vertical_radius: 2,
        max_vertical_radius: 4,
        segments: 5,
        segment_step: 2,
        stream: 0x4449_414d_5f56_3201,
    },
};
