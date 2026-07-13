use std::sync::Arc;

use voxelize::{Chunk, ChunkOptions, ChunkStage, ChunkStatus, Resources, Vec2, WorldConfig};

use super::{stage::ExtractionTerrainStage, GenerationPlan};
use crate::{contracts::bundled_manifest, engine_catalog::EngineCatalog};

pub(crate) const FIXED_SEED: u64 = 0x1122_3344_5566_7788;

pub(crate) fn fixture() -> (Arc<GenerationPlan>, EngineCatalog, WorldConfig) {
    let manifest = bundled_manifest().unwrap();
    let catalog = EngineCatalog::from_manifest(&manifest).unwrap();
    let plan = Arc::new(
        GenerationPlan::new(
            FIXED_SEED,
            "generation-v1",
            "balance-v1",
            catalog.resources(),
        )
        .unwrap(),
    );
    let config = WorldConfig::new()
        .min_chunk([-10, -10])
        .max_chunk([9, 9])
        .max_height(plan.config().max_height)
        .water_level(0)
        .build();
    (plan, catalog, config)
}

pub(crate) fn ready_chunk(
    plan: Arc<GenerationPlan>,
    catalog: &EngineCatalog,
    config: &WorldConfig,
    coords: Vec2<i32>,
) -> Chunk {
    let options = ChunkOptions {
        size: config.chunk_size,
        max_height: config.max_height,
        sub_chunks: config.sub_chunks,
    };
    let mut chunk = ExtractionTerrainStage::new(plan).process(
        Chunk::new("fixed-generation-test", coords.0, coords.1, &options),
        Resources {
            registry: catalog.blocks(),
            config,
        },
        None,
    );
    chunk.status = ChunkStatus::Ready;
    chunk
}
