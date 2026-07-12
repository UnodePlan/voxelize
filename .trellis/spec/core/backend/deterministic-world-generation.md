# Deterministic Extraction World Generation

## Scenario: Versioned disposable match Worlds

### 1. Scope / Trigger

- Applies to `contracts/extraction/v1/manifest.json`, Rust/TypeScript manifest decoders, `engine_catalog`, `generation`, and dynamic World preparation in `engine_matchmaking`.
- Trigger this spec when changing resource or equipment IDs, map dimensions, terrain, ore topology, spawn/extraction candidates, generation/config versions, preloading, or map fingerprint tests.
- `generation-v1` and `balance-v1` are immutable once referenced by a match. Balance changes require a new named version and a new golden fingerprint; never update the V1 expectation to approve an in-place map change.

### 2. Signatures

```rust
pub(crate) fn EngineCatalog::from_manifest(
    manifest: &ExtractionManifest,
) -> Result<EngineCatalog, CatalogError>;

pub(crate) fn GenerationPlan::new(
    seed: u64,
    generation_version: &str,
    config_version: &str,
    resources: MatchResourceCatalog,
) -> Result<GenerationPlan, GenerationError>;

pub trait ChunkStage: Send + Sync {
    fn process(&self, chunk: Chunk, resources: Resources, space: Option<Space>) -> Chunk;
}

async fn EngineMatchWorldRuntime::prepare_world(
    &self,
    spec: MatchWorldSpec,
) -> Result<PreparedMatchWorld, MatchWorldRuntimeError>;
```

### 3. Contracts

- The manifest is the ID authority. V1 is `dirt=(voxel 1001,item 2001,stack 64,weight 1)`, `gold=(1002,2002,64,10)`, `diamond=(1003,2003,64,100)`, `basic_pickaxe=item 2101`, and `basic_melee_weapon=item 2102`.
- Voxel IDs are unique in `1..=65535`. Item IDs are globally unique in `1..=i32::MAX`, because engine held-item encoding negates item IDs into an `i32`. Manifest array order is not identity; catalog construction iterates `ResourceKey::ALL` and `EquipmentKey::ALL`.
- Server startup installs one global block `Registry`; every disposable World receives a cloned five-item `ItemRegistry`. Resource items carry `stackable { maxStack: 64 }`; fixed equipment does not.
- Generation resolves only an exact `(generation_version, config_version)` pair. V1 uses X/Z `[-150,150)`, height 64, solid surface Y 48, and a non-mineable floor at Y 0. Runtime mining must read the current Chunk voxel and atomically claim it; `GenerationPlan` describes only initial terrain.
- Every persisted bit of the full `u64 seed` affects generation. `WorldConfig.seed` is only the named `engine_seed_v1` 32-bit fold and must never be the map generator's sole seed source.
- `ExtractionTerrainStage` is a pure global-coordinate function. It owns immutable plan data, uses integer hashing, writes only the current Chunk, has no mutable RNG/cache, and never emits `extra_changes`. Chunk request order, Rayon scheduling, and prior matches cannot affect output.
- V1 has 90,000 dirt surface columns, five connected middle-depth gold deposits, and three connected deeper center-biased diamond deposits. Spawn points are a ten-point outer ring; gold anchors are rotated between adjacent spawns. Eight extraction candidates form one symmetric inner ring. Seed only shuffles stable candidate arrays.
- Dynamic Worlds use `saving(false)`, preload radius 10, and do not open the application attach generation until lifecycle `Ready`. Preparation polls the same generation for at most 60 seconds. Timeout, actor error, or generation mismatch starts a bounded five-second removal and clears both ownership and attach-generation maps.
- The V1 actual-Chunk SHA-256 fingerprint for seed `0x1122334455667788` is `234903c7af2917afb0e3b9aa643f5848c40f8e12b5494cd2b4d18187bb881df5`.
- Known production fairness gap: strict Worlds still accept client-selected Chunk `LOAD` coordinates. Before production PVP fairness is claimed, LOAD must be bounded by authoritative player position and a bounded visible radius, or an equivalent anti-Xray design must hide unrevealed ore. Client UI limits are not a security boundary.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Missing/duplicate resource or equipment key | Manifest validation error; abort startup |
| Duplicate/zero/out-of-range voxel or item ID | Manifest validation error; never enter registry panic paths |
| Unsupported catalog, generation, or config version | `CatalogError`/`GenerationError`; reject before `AddWorld` |
| Same seed and exact versions | Byte-identical canonical Chunk voxel fingerprint |
| Different full `u64` seeds with the same folded engine seed | Observably different map fingerprint |
| Stage attempts a cross-Chunk write | Test failure because `extra_changes` must stay empty |
| World is still `Preparing` | Keep attach generation closed and poll with the same World generation |
| Preparation exceeds 60 seconds or World generation changes | Remove the World, clear runtime maps, return `Unavailable` |
| A new match reuses a seed after an earlier Chunk was mutated | Generate fresh initial terrain; inherit no Chunk state |
| Client requests arbitrary distant LOAD in production | Current known gap; production readiness must remain blocked until bounded |

### 5. Good / Base / Bad Cases

- Good: the server validates the bundled manifest, builds explicit registries, constructs a fresh immutable plan from the complete match seed, preloads the spawn core, and publishes the generation only after `Ready`.
- Base: two independent matches with the same seed/version generate identical initial maps; mutations in the first in-memory World do not appear in the second.
- Bad: auto-assigning IDs from registration order, using `WorldConfig.seed` as the full seed, sharing an advancing RNG between Chunk jobs, writing neighboring Chunks, enabling saving for match Worlds, accepting an unknown version by falling back to latest, or updating the V1 golden hash after tuning V1.

### 6. Tests Required

- Rust and TypeScript contract tests pin all five IDs, stack limits, weights, duplicate handling, voxel bounds, and the signed item-ID upper bound.
- Catalog tests reverse manifest arrays and assert identical block/item identity, resource stack components, equipment non-stackability, and fail-closed unsupported versions.
- Process actual `ChunkStage` output for all 400 engine Chunks, hash only canonical gameplay coordinates, and assert the frozen SHA-256 value. Repeat with forward and reverse Chunk processing order.
- Use seeds `1` and `1 << 32`: their `engine_seed_v1` values are equal, while actual map fingerprints must differ.
- Scan actual generated voxels with six-neighbor BFS. For the frozen seed assert dirt `4,387,170`, gold `20,265` in five components of `4,053`, diamond `2,565` in three components of `855`, plus depth and center-bias constraints.
- Assert all 90,000 surface columns are dirt, gameplay padding is Air, Stage output equals the pure plan at Chunk boundaries, and `extra_changes` is empty.
- Assert ten unique safe spawns, fair nearest-gold distance, eight symmetric extraction candidates, multi-seed candidate coverage, and fresh-match isolation after mutating a prior Chunk.

### 7. Wrong vs Correct

#### Wrong

```rust
let mut rng = StdRng::seed_from_u64(u64::from(config.seed));
for chunk in requested_chunks {
    generate_with_shared_rng(chunk, &mut rng);
}
```

This truncates the persisted seed and makes terrain depend on request/scheduling order.

#### Correct

```rust
let plan = Arc::new(GenerationPlan::new(
    match_spec.seed,
    &match_spec.generation_version,
    &match_spec.config_version,
    catalog.resources(),
)?);
world.pipeline_mut().add_stage(ExtractionTerrainStage::new(plan));
```

The immutable plan captures the full seed and resolves every voxel from global integer coordinates, so independent Worlds and arbitrary Chunk schedules reproduce the same initial map.
