# Authoritative Movement and Bounded World Visibility

## Scenario: Playable movement without client-owned position or remote-world leaks

### 1. Scope / Trigger

- Applies to extraction movement input, `engine_movement`, Voxelize client physics synchronization, client `LOAD` handling, server-side `ChunkProjection`, `INIT`, peer/entity projection, and the product client's real Voxelize World lifecycle.
- Trigger this spec when changing body dimensions, eye height, speed, jump, gravity, physics ordering, movement payloads, chunk loading, hidden-voxel projection/reveal, peer/entity visibility, reconnect initialization, or client World disposal.
- Legacy Worlds remain permissive by default. PVP Worlds must opt into both authoritative movement and bounded visibility.

### 2. Signatures

```rust
pub enum ChunkLoadPolicy {
    Legacy,
    AuthoritativeRadius { max_chunk_radius: u32 },
}

pub enum EntityVisibilityPolicy {
    Legacy,
    Bounded,
}

pub struct ChunkProjection { /* world-local replacement and reveal state */ }

impl ChunkProjection {
    pub fn obfuscating(
        replacements: impl IntoIterator<Item = (u32, u32)>,
    ) -> Result<Self, ChunkProjectionError>;
}

pub(crate) fn install_bounded_movement(
    world: &mut World,
    bounds: PlayableBounds,
    authority: GameplayAuthority,
) -> Result<(), DispatcherHookError>;
```

```ts
interface MovementInput {
  direction: [number, number, number];
  movement: { forward: number; right: number; jump: boolean };
}
```

### 3. Contracts

- A movement packet is an intent, never a transform. The server accepts finite normalized movement axes and a finite non-zero look direction. A legacy `position` field may be parsed for wire compatibility but is ignored. Unknown fields are rejected.
- Input admission uses a server-monotonic token bucket of 30 packets per second with burst 6. Accepted control becomes stale after 250 ms. Invalid, over-budget, stale, unauthorized, dead, or settlement-pending control stops horizontal input and clears a pending jump.
- Jump is edge-triggered. Holding jump cannot automatically jump again after landing, and an airborne jump edge cannot add a second impulse.
- The canonical player body is `0.8 x 1.8 x 0.8`. `RigidBodyComp` and Rapier use the body center. `PositionComp` is the public eye position at body-center Y plus `0.72`, equivalent to a `1.62` eye height above the feet.
- Horizontal speed is fixed at 6 blocks per second, jump impulse at 8, gravity multiplier at 1, auto-step disabled, mass 1, and restitution 0. Client sprint, crouch, flying, ghost, step height, position, or velocity cannot change these values.
- Movement reuses Voxelize `Physics::iterate_body` swept-AABB collision. Its named default-dispatcher hook runs after `update-stats` and before `peers-meta`, `current-chunk`, mining, default physics/Rapier synchronization, combat, extraction, and network projection. A tick is accepted only for finite positive delta up to 50 ms and when the current 3 by 3 chunk neighborhood is ready.
- The complete AABB must remain inside X/Z `[-150,150)` and Y `[0,max_height)`. Invalid transforms, dimensions, velocity, missing chunks, or authority time fail closed. Rapier synchronization and collision displacement compare body center to body center; eye position is never used as a physics center.
- PVP chunk `LOAD` uses only authoritative `PositionComp` and `DirectionComp`. Requested chunks must be inside both the configured World chunk bounds and an inclusive Euclidean radius of 6 chunks. Client `center` and `direction` remain required only for legacy wire compatibility.
- Every World owns a default identity `ChunkProjection`. Extraction Worlds replace unrevealed gold and diamond voxel IDs with dirt only in outbound `LOAD`/`UPDATE` payloads; authoritative `Chunks`, collision, raycasts, mining claims, inventory output, and persistence always retain the real voxel ID.
- A hidden voxel is revealed when one of its six faces borders a ready transparent voxel. Missing neighbor chunks and coordinates below the World fail closed; the open boundary above `max_height` is exposed. Reveal state is monotonic for one disposable World and is discarded with that World.
- Projection runs on both full/data-only `LOAD` paths and after the complete authoritative update batch has been applied. An authoritative Air update reveals adjacent hidden voxels, including across Chunk boundaries, and emits deterministic synthetic updates only to clients interested in the affected Chunk. An ore update remains dirt until exposure.
- Non-identity projection forbids server-generated meshes because their material IDs would bypass voxel projection. Extraction Worlds enable client-only meshing, and the client worker must build geometry from the projected voxel payload.
- PVP `INIT`, peer updates, and entity updates use a 96-block visibility radius. Entering peers receive encoded `JOIN` then `PEER`; stable batching preserves each merge group's first queue position. Leaving peers receive `LEAVE`. Entering entities receive `CREATE`; leaving entities receive `DELETE` without metadata. Missing or non-finite entity positions are invisible.
- In bounded mode, the viewer's own authoritative `PEER` snapshot is delivered directly every tick, even when its metadata cache is unchanged, so a rejected or rolled-back movement can still reconcile continued local prediction. Remote metadata outside the visibility radius is never attached to leave/delete messages.
- A test or automation client treats movement as acknowledged only after observing a fresh own-`PEER` revision produced after the input. Equality with a cached direction is not an acknowledgement. Before switching from movement to a ray-based action, it requires consecutive fresh authoritative samples with the full eye origin `(x,y,z)` stable and recomputes the look direction from each sample; horizontal-only stability is invalid while gravity or jumping can still change Y.
- Automated mining aims at a visible point on the exposed target face rather than blindly at the voxel center, which may be occluded by a nearer surface voxel. This only supplies an intent: the server raycast, reach, readiness, tool, duration, and atomic voxel claim remain authoritative.
- `ChunkLoadPolicy::Legacy` and `EntityVisibilityPolicy::Legacy` preserve legacy behavior. Existing demos and transport INIT messages do not become bounded merely because the builder resolves a default entity render radius.
- The product client creates a real Voxelize `World` only from a decoded `INIT`, routes `LOAD`, `UNLOAD`, `UPDATE`, `PEER`, `ENTITY`, `JOIN`, and `LEAVE` protocol messages into it, sends movement at no more than 20 Hz, and forwards only client `LOAD`/`UNLOAD` World packets.
- Leaving, policy close, reconnect expiry, logout, wallet/chain change, or a replacement `INIT` disposes the old World, worker, controls, listeners, timers, geometries, and instance materials. A disposed asynchronous initialization cannot recreate workers or scene objects.
- Test facades and E2E bridges are Vitest/Vite-mode dependencies only. Production artifact scanning rejects their identifiers.

### 4. Validation & Error Matrix

| Condition                                                                  | Required result                                                            |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Client submits a position, velocity, flying, ghost, or sprint value        | Ignore compatible position; reject all unsupported fields and privileges   |
| Axes exceed unit length, direction is zero/non-finite, or rate is exceeded | Stop input; do not mutate authoritative transform from client data         |
| Input is older than 250 ms                                                 | Stop horizontal control and pending jump                                   |
| Jump remains held through landing                                          | Do not jump again until a release and a new press                          |
| Swept body reaches a solid voxel                                           | Stop at the collision plane; never cross through repeated legal steps      |
| Delta is invalid/too large or required chunks are unavailable              | Stop motion for the tick; do not treat missing voxels as Air               |
| Full AABB would cross gameplay or height bounds                            | Roll back the tick and zero motion                                         |
| Client requests a distant or out-of-world chunk                            | Reject that coordinate even if client center/direction claims it is nearby |
| Hidden gold/diamond has no ready transparent face                          | Send dirt ID in every outbound LOAD/UPDATE; keep authoritative ore intact  |
| Air update exposes adjacent ore, including across a Chunk boundary         | Reveal once and send real ore only to clients interested in that Chunk     |
| Neighbor Chunk needed for exposure is unavailable                          | Treat the face as closed; do not reveal                                    |
| Projection is non-identity while server meshing is configured              | Suppress server mesh and send projected voxel data for client-only meshing |
| Peer/entity exits 96 blocks                                                | Send only bounded leave/delete state; no far coordinate metadata           |
| Viewer position is unchanged after server rejects or rolls back movement   | Still deliver direct authoritative `PEER` for prediction reconciliation    |
| Cached direction already equals the requested direction                    | Wait for a newer own-`PEER` revision; never accept the stale cache         |
| X/Z is stable while gravity still changes authoritative eye Y              | Keep sampling and recompute the ray from the fresh full 3D origin          |
| World is left, replaced, or reconnect expires                              | Dispose exactly once and clear all old protocol/render state               |

### 5. Good / Base / Bad Cases

- Good: the client predicts locally, submits fixed-rate axes and look direction, the server sweeps the canonical body through ready voxels, sends the authoritative eye position back, and projects enclosed ore to dirt until an authoritative excavation exposes it.
- Base: a legacy demo keeps client-directed chunk loading and global peer behavior because it did not opt into the PVP policies.
- Bad: trusting client position, moving from peer metadata, applying speed after sweep, mapping Rapier to eye position, accepting missing chunks as Air, sending an authoritative server mesh from a projected World, broadcasting every peer/entity globally, or deleting a World without terminating its workers and listeners.

### 6. Tests Required

- Movement tests cover exact wire schema, ignored position, unknown fields, axes/direction bounds, token bucket, 250 ms staleness, gravity/landing, jump edge behavior, wall collision, unavailable chunks, ghost/body repair, and complete World bounds. Dispatcher tests assert movement precedes peer metadata, current-chunk-dependent mining, and default physics consumers.
- Physics regression tests assert Rapier displacement uses body center, not `PositionComp` eye height.
- World tests cover legacy and authoritative `LOAD`, negative coordinates, World bounds, radius edges, non-finite authoritative state, bounded `INIT`, peer enter/update/leave, encoded `JOIN` before first `PEER`, unchanged own-peer reconciliation, entity create/update/delete, and metadata-free bounded deletion.
- Projection tests use actual Chunks and registry transparency to cover identity mode, invalid mappings, enclosed LOAD masking, exposed ore, cross-Chunk exposure, missing-neighbor fail closure, same-batch final-state handling, deterministic reveal ordering, interested-client-only UPDATE routing, and fresh-World reveal reset.
- Client tests cover decoder worker fallback, protocol routing, 20 Hz movement, world reset on every terminal socket path, disposal during asynchronous initialization, input listener cleanup, object disposal, and production-boundary scanning.
- E2E actor tests start with a cached direction already equal to the requested direction, then change authoritative Y and prove that no mining or combat action advances until consecutive fresh full-3D `PEER` samples stabilize. Real gameplay evidence must consume the server's health state after every hit, including the terminal `dead, health=0` frame, rather than synthesizing the final value.
- Run root World tests, extraction-server engine all-target tests, application Clippy with `--no-deps -D warnings`, Core/client tests and typechecks, a production build with real WASM/Core artifacts, responsive browser screenshots, canvas pixel checks, and `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
let payload = authoritative_chunk.to_model(true, true, levels);
queue.send_all(payload);
```

This leaks hidden material IDs through both voxel data and server-generated mesh geometry.

#### Correct

```rust
let payload = projection.project_chunk(chunk, chunks, registry, false, levels);
queue.send_interested(client_id, payload);
```

The authoritative Chunk remains unchanged, projected Worlds never send authoritative meshes, and later exposure is derived from final server state. This prevents static ore-map inspection inside the legal load radius; it does not attempt to erase information that was legitimately revealed earlier in the same match.
