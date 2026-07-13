# Authoritative Match Mining

## Scenario: Server-timed voxel harvesting with private progress state

### 1. Scope / Trigger

- Applies to `contracts/extraction/v1`, extraction client mining intents/state, `gameplay/mining`, `gameplay/harvest`, `engine_gameplay`, and the default World dispatcher.
- Trigger this spec when changing mining input, reach/raycast rules, resource durations, heartbeat grace, mining revisions, harvested claims, AIR updates, or the pre-`ChunkUpdating` hook.
- Mining is match-scoped and server-authoritative. A client submits an operation candidate; it never submits a resource, voxel ID, duration, progress, completion, or quantity.

### 2. Signatures

```rust
pub fn decode_mining_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<MiningPayload>, ContractError>;

pub enum MiningPayload {
    Start { voxel: [i32; 3] },
    Maintain {},
    Cancel {},
}

pub fn World::install_before_chunk_updating_system<T, F>(
    &mut self,
    name: &'static str,
    factory: F,
) -> Result<(), DispatcherHookError>;

pub(crate) fn award_harvest_atomically(
    request: HarvestRequest,
    assets: HarvestAssets<'_>,
) -> Result<HarvestDestination, HarvestError>;
```

Wire methods:

```text
client -> server: pvp:v1:mining
server -> client: pvp:v1:mining-state
```

### 3. Contracts

- Mining uses the top-level `ProtocolEnvelope.sequence: u32`. It must increase strictly across start, maintain, and cancel. `requestId` is a UUID used only to correlate the Result.
- Start payload is exactly `{ action: "start", voxel: [i32, i32, i32] }`. Maintain and cancel contain only `action`. Unknown fields are malformed.
- The server validates participant authority, fixed basic pickaxe, exact 300 by 300 bounds, mineable Y, Ready chunk, unclaimed resource block, finite eye position/direction, nonzero direction, 4.5-block reach, and first-solid-voxel equality.
- All non-Air voxels occlude. Missing or non-Ready chunks fail closed; `Chunks::get_voxel` returning Air for missing data is not evidence of empty space.
- Version `pvp-mvp-v1 / balance-v1` uses dirt 500ms, gold 1500ms, diamond 3000ms, maintain grace 350ms, and progress sampling 50ms. Durations are absolute monotonic server time, not accumulated tick delta.
- Releasing input, starting any target, timeout after the inclusive grace boundary, invalid authority, missing component/tool, range/visibility failure, changed block, or another claim clears unfinished progress. A new start always begins at zero.
- A completion first prepares an infallible completed `MiningState` clone, then atomically claims `VoxelCoordinate`, transfers exactly one resource to inventory or deterministic pending drop, stages AIR, and installs the completed state. A failed asset transaction releases the claim and does not stage AIR.
- Mining drop IDs are `drop:v1:{match_id}:mined:{x}:{y}:{z}`. A full or revision-exhausted inventory routes quantity one to public pending ownership at the voxel center.
- Completion candidates sort by `ready_at`, then stable seat ID. `HarvestedVoxelSet` permits only one winner while AIR remains in Chunk staging.
- The default dispatcher fixes `CurrentChunk -> named mining hook -> ChunkUpdating`. The hook has no caller-provided dependencies, is installable once, rejects default-name conflicts/custom dispatchers, rebuilds cached dispatchers, and preserves post extensions.
- `pvp:v1:mining-state` is a Direct full snapshot with `matchId`, stream `mining`, monotonic `revision`, and either idle or mining data. The client ignores another match and any revision not newer than its current snapshot. Because the server may coalesce multiple mutations in one tick, any higher revision is valid; snapshot gaps do not imply lost delta state.
- UI progress reads only server `elapsedMs / requiredMs`. A local timer may animate toward the received value but cannot advance authoritative progress, remove a voxel, or add inventory.
- `retryable=true` means the client may issue a fresh envelope. If an intent reached authoritative state validation, retry uses a new request ID and higher sequence; queue-full rejection occurs before sequence consumption.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Bad version, UUID, action shape, coordinate tuple, i32/u32 bound, or invented field | Protocol error; no mining intent queued |
| Detached, dead, non-participant, inactive phase, missing component, or invalid direction | `GAME_INVALID_STATE`; new valid sequence clears active progress |
| Target outside bounds/reach or first solid hit differs | `GAME_OUT_OF_RANGE`; clear active progress |
| Target chunk missing/non-Ready | Retryable `SERVICE_UNAVAILABLE`; clear active progress and retry with a higher sequence |
| Air, unknown voxel, changed voxel, or already harvested coordinate | `GAME_INVALID_STATE`; no asset or AIR mutation |
| Duplicate or older sequence | `GAME_STALE_SEQUENCE`; preserve current state |
| Intent queue full | Retryable `SERVICE_UNAVAILABLE`; sequence is not consumed |
| Heartbeat at exactly 350ms / after 350ms | Keep attempt / reset to zero |
| Progress at 499/500, 1499/1500, or 2999/3000ms | Not ready / ready exactly once |
| Sync or resource duration is nonzero but below 1ms | Reject config or return `InvalidDuration`; never divide by zero |
| Two completions claim the same voxel in one tick | Lower `(ready_at, seat)` wins; total resource output is one |
| Inventory cannot accept quantity one | Claim once, stage AIR once, create one public pending resource |
| Client sends raw UPDATE in strict match World | Reject before Chunk staging |

### 5. Good / Base / Bad Cases

- Good: a player sends start and periodic maintain intents while the server continuously sees the same first-hit gold block; at exactly 1500ms one gold enters the inventory and AIR is consumed by `ChunkUpdating` in the same dispatch.
- Base: a full backpack completes a valid dirt block; the block becomes AIR once and one public dirt pending drop remains at its center.
- Bad: accepting a client `resource`, trusting a local completion timer, treating a missing chunk as Air, writing the Chunk directly with `set_voxel`, awarding before claim, or requiring contiguous revisions for full snapshots.

### 6. Tests Required

- Shared Rust/TypeScript fixtures assert valid start/maintain/cancel, strict unknown-field rejection, exact three-element i32 coordinates, forged duration/resource/completion rejection, and mining-state discriminants/ranges.
- Domain tests assert strict sequence ordering, target-switch restart, cancel, grace before/equal/after, exact three-resource duration boundaries, sub-millisecond rejection, completion-clone preflight, claim uniqueness, rollback, and inventory/pending conservation.
- Ray/ECS tests assert Ready/non-Ready chunks, occlusion by any solid voxel, zero/NaN direction without panic, exact completion, two-player stable competition, and full-inventory pending output.
- Dispatcher tests build a real Ready chunk, establish the cache, install the hook, and assert a post-`ChunkUpdating` observer sees AIR in the same tick while existing extensions still run.
- Client tests assert stale/wrong-match snapshot rejection, higher non-contiguous full-snapshot acceptance, local timer non-authority, monotonic intent generation, i32 validation, and sequence exhaustion without wrap.
- Run extraction-server engine all-target tests, application `--no-deps -D warnings` Clippy, root library tests, extraction-client tests/build, and `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
if client_elapsed_ms >= client_duration_ms {
    inventory.insert(client_resource, client_quantity)?;
    chunk.set_voxel(x, y, z, AIR);
}
```

#### Correct

```rust
let target = validate_mining_target(requested, authoritative_access)?;
let completed_state = mining.state().completed()?;
let _destination = award_harvest_atomically(request, assets)?;
chunks.update_voxel(
    &Vec3(target.voxel.x, target.voxel.y, target.voxel.z),
    AIR,
);
*mining.state_mut() = completed_state;
```

The server derives the resource and timing, the claim prevents duplicate output while AIR is staged, and `ChunkUpdating` remains the only path that applies and broadcasts the voxel mutation.
