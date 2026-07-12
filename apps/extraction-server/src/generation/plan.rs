use std::{error::Error, fmt};

use super::{
    config::{GenerationConfig, MapPoint},
    deposits::{build_deposits, ResourceDeposit},
    hash::shuffle,
};
use crate::engine_catalog::MatchResourceCatalog;

const SPAWN_STREAM: u64 = 0x5350_4157_4e5f_5631;
const EXTRACTION_STREAM: u64 = 0x4558_5452_4143_5431;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MatchMapLayout {
    spawn_points: [MapPoint; 10],
    extraction_candidates: [MapPoint; 8],
}

impl MatchMapLayout {
    #[cfg(test)]
    pub(crate) fn spawn_points(&self) -> &[MapPoint; 10] {
        &self.spawn_points
    }

    pub(crate) fn spawn_for_seat(&self, seat: usize) -> Option<MapPoint> {
        self.spawn_points.get(seat).copied()
    }

    #[cfg(test)]
    pub(crate) fn extraction_candidates(&self) -> &[MapPoint; 8] {
        &self.extraction_candidates
    }

    pub(crate) fn selected_extraction(&self) -> MapPoint {
        self.extraction_candidates[0]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GenerationPlan {
    seed: u64,
    config: &'static GenerationConfig,
    resources: MatchResourceCatalog,
    layout: MatchMapLayout,
    gold_deposits: Vec<ResourceDeposit>,
    diamond_deposits: Vec<ResourceDeposit>,
}

impl GenerationPlan {
    pub(crate) fn new(
        seed: u64,
        generation_version: &str,
        config_version: &str,
        resources: MatchResourceCatalog,
    ) -> Result<Self, GenerationError> {
        let config = GenerationConfig::resolve(generation_version, config_version)
            .ok_or_else(|| GenerationError::new("比赛引用了不受支持的地图配置版本"))?;
        let mut spawn_points = *config.spawn_points;
        let mut extraction_candidates = *config.extraction_candidates;
        shuffle(seed, SPAWN_STREAM, &mut spawn_points);
        shuffle(seed, EXTRACTION_STREAM, &mut extraction_candidates);

        Ok(Self {
            seed,
            config,
            resources,
            layout: MatchMapLayout {
                spawn_points,
                extraction_candidates,
            },
            gold_deposits: build_deposits(seed, config.gold),
            diamond_deposits: build_deposits(seed, config.diamond),
        })
    }

    pub(crate) fn seed(&self) -> u64 {
        self.seed
    }

    pub(crate) fn config(&self) -> &'static GenerationConfig {
        self.config
    }

    pub(crate) fn resources(&self) -> MatchResourceCatalog {
        self.resources
    }

    pub(crate) fn layout(&self) -> &MatchMapLayout {
        &self.layout
    }

    pub(crate) fn voxel_at(&self, point: MapPoint) -> u32 {
        if point.y < 0
            || point.y > self.config.surface_y
            || !self.config.contains_xz(point.x, point.z)
        {
            return 0;
        }
        if self
            .diamond_deposits
            .iter()
            .any(|deposit| deposit.contains(point))
        {
            return self.resources.diamond.voxel_id;
        }
        if self
            .gold_deposits
            .iter()
            .any(|deposit| deposit.contains(point))
        {
            return self.resources.gold.voxel_id;
        }
        self.resources.dirt.voxel_id
    }

    /// 只描述初始地图；实际挖掘还必须读取当前 World 体素并原子声明采集权。
    #[cfg(test)]
    pub(crate) fn is_initially_mineable(&self, point: MapPoint) -> bool {
        point.y > self.config.unbreakable_floor_y && self.voxel_at(point) != 0
    }

    #[cfg(test)]
    pub(super) fn gold_deposits(&self) -> &[ResourceDeposit] {
        &self.gold_deposits
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GenerationError {
    message: String,
}

impl GenerationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GenerationError {}
