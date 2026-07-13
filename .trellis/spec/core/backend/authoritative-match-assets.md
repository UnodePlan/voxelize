# Authoritative Match Inventory and Loot

## Scenario: Private match assets, deterministic drops, and automatic pickup

### 1. Scope / Trigger

- Applies to `contracts/extraction/v1`, `apps/extraction-server/src/gameplay`, `engine_gameplay`, dynamic match World setup, and client inventory-state decoding.
- Trigger this spec when changing resource slots, stack limits, inventory revision behavior, fixed equipment, manual drop requests, drop IDs, pending ownership, loot merging, automatic pickup, private snapshots, or gameplay dispatcher integration.
- Match assets are in-memory and World-scoped. They are not warehouse rows and must never read initial quantities from permanent storage.

### 2. Signatures

```rust
pub fn decode_drop_slot_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<DropSlotPayload>, ContractError>;

pub(crate) fn MatchInventory::insert_batch(
    &mut self,
    entries: &[(ResourceKey, u32)],
) -> Result<Vec<InsertOutcome>, InventoryError>;

pub(crate) fn drop_slot_atomically(
    request: ManualDropRequest,
    config: &GameplayConfig,
    assets: ManualDropAssets<'_>,
) -> Result<ManualDropReceipt, ManualDropError>;

pub fn World::extend_dispatcher<F>(&mut self, extend: F)
where
    F: Fn(TimedDispatcherBuilder<'static, 'static>)
        -> TimedDispatcherBuilder<'static, 'static>
        + Send
        + Sync
        + 'static;

pub fn World::add_client_modifier<F>(&mut self, modifier: F)
where
    F: Fn(&mut World, Entity) + Send + Sync + 'static;
```

The strict match World allows only the implemented gameplay methods. Stage 5 adds:

```text
pvp:v1:drop-slot
pvp:v1:get-state
```

### 3. Contracts

- `MatchInventory` has exactly 12 private resource slots. A slot is empty or one `ResourceStack { resource, quantity }`; each resource stack is capped at 64.
- Fixed `basic_pickaxe` and `basic_melee_weapon` equipment is a separate immutable component. It never occupies a resource slot, enters loot, or depends on permanent warehouse contents.
- Insertions fill existing same-resource stacks in slot order, then empty slots in slot order. They return exact `accepted/remainder`. A multi-resource loot transfer is calculated on an inventory clone and commits with one checked inventory revision increment.
- Inventory and loot revisions are `u32`. Actual mutations increment once; no-op full-inventory attempts do not. Exhaustion fails before partial mutation and never wraps.
- An asset belongs to exactly one active owner: player inventory, `PendingDropQueue`, or a World `LootDropComp`. Future settlement may add committed warehouse ownership, but no Stage 5 path writes the database.
- `PendingDropQueue` is a sorted authoritative owner, not a transient notification. The same ID and identical payload is idempotent; the same ID with different contents is a conflict. A pending item is acknowledged only after a successful merge or complete ECS entity creation.
- Manual drop IDs are `drop:v1:{match_id}:seat:{seat_id}:manual:{sequence}`. The authenticated World client determines account, public player, and seat. The client cannot submit identity, resource key, quantity, position, or target.
- The wire sequence is the top-level `ProtocolEnvelope.sequence: u32`. `drop-slot` payload is exactly `{ slot: 0..11, expectedInventoryRevision: u32 }`; unknown fields, a client quantity, and slot 12 are malformed.
- A valid manual drop first moves the entire slot into pending ownership, then increments inventory revision. It is placed in front of the player and excludes only the authenticated owner while `now < created_at + 2s`; the exact two-second boundary is eligible.
- World loot has no TTL. Unprotected compatible loot may merge only in the same versioned spatial bucket. Active protected loot cannot merge; expired protection may be cleared by a later merge.
- Automatic pickup sorts World drops by stable `DropId`. For each drop it sorts eligible players by squared distance and then stable `SeatId`. A full nearest player does not block the next candidate. Each accepted quantity is added to one inventory and subtracted from the same loot in one mutable ECS critical section.
- Private inventory/equipment state is sent only with `ClientFilter::Direct`. It never enters `MetadataComp`; public loot metadata contains only renderable drop ID, contents, and revision.
- Dynamic Worlds compose player initialization with `add_client_modifier`, preserving Stage 4 spawn assignment. Rebind retains the original entity and does not install a second inventory.
- Stage 5 appends one aggregate gameplay system after all default dispatcher leaves through `extend_dispatcher`. Direct messages and entity projection may appear on the following World tick. Stage 6 mining still requires an explicit pre-`ChunkUpdatingSystem` integration point; do not claim the append-only hook solves that order.
- Match World removal owns final cleanup. Dropping the World releases inventories, pending assets, spawned-ID sets, loot entities, and intent queues together; code must not add a TTL or cross-match singleton.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Unsupported gameplay/config pair or loadout/catalog stack mismatch | Reject before `AddWorld` |
| Client is detached, unauthenticated, not Active, or not joined | `GAME_INVALID_STATE`; no queue or asset mutation |
| Envelope has a valid request ID but malformed structure, unknown payload fields, or slot outside `0..11` | `REQUEST_MALFORMED`; no intent queue entry |
| Envelope uses an unsupported protocol version and has a valid request ID | `PROTOCOL_UNSUPPORTED_VERSION`; no intent queue entry |
| Raw JSON or request ID cannot be parsed | No protocol result can be correlated; silently reject before the intent queue |
| Intent queue is full | `SERVICE_UNAVAILABLE`, retryable; no asset mutation |
| Sequence is duplicate, older, or already produced a deterministic drop | `GAME_STALE_SEQUENCE`; never create another asset |
| Expected revision differs from current inventory revision | Consume the new sequence, return `GAME_STALE_REVISION`, keep inventory unchanged |
| Slot is empty or outside the resource inventory | `INVENTORY_SLOT_INVALID`; fixed equipment is not addressable |
| Inventory has partial capacity | Accept only capacity and retain exact loot remainder |
| Inventory has no capacity | No revision change; leave all loot on the ground |
| Owner pickup at `1.999s` / exactly `2.000s` | Reject before boundary / allow at boundary |
| Two pending entries have one ID but different payloads | Conflict; never overwrite or sum them |
| ECS drop creation fails | Re-enqueue the same pending asset; do not mark its ID spawned |
| Loot quantity or revision would overflow | Fail before mutation; preserve both owners |

### 5. Good / Base / Bad Cases

- Good: a 65-gold insertion creates stacks `64,1` with one revision; a later automatic pickup fills the `1` stack before allocating another slot.
- Base: a full player is skipped and the same drop is offered to the next distance/seat candidate; if every candidate is full, the loot remains unchanged indefinitely.
- Bad: using `SlotContent` as the authoritative balance, mutating public metadata as inventory, draining pending before successful ECS ownership, random UUID drop IDs, using entity iteration order as a tiebreaker, or deleting loot after a timer.

### 6. Tests Required

- Domain tests assert 64/65 stacking, stable slot order, full and partial acceptance, one revision for multi-resource pickup, freeze behavior, overflow rollback, and per-resource conservation.
- Manual-drop tests assert whole-slot transfer, top-level sequence monotonicity, expected revision, empty/out-of-range slots, deterministic ID, exact owner exclusion boundary, and no duplicate pending/World asset.
- Pickup tests reverse candidate/entity construction order, assert distance then seat ordering, assert a full nearest candidate cannot block the next, and assert each quantity enters at most one inventory.
- Pending/loot tests assert identical-ID idempotency, conflicting-ID rejection, stable drain order, protected merge rejection, merge after expiry, spatial bucket validation, no TTL, and checked quantity overflow.
- ECS tests run the real Specs gameplay system and assert `inventory + pending + LootDropComp` conservation after pending spawn, automatic pickup, manual drop, and duplicate sequence.
- Core tests assert dispatcher extension preserves the current factory and invalidates the built cache, and client modifier extension runs after the existing modifier.
- Shared Rust/TypeScript fixture tests assert slot `0/11/12`, `u32::MAX`, oversized sequence, missing revision, and client-invented quantity behavior.
- Run extraction-server default and `engine` all-target tests, application Clippy with `--no-deps -D warnings`, root World tests, extraction-client tests/build, and `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
let stack = inventory.slots[slot].take();
spawn_loot_entity(Uuid::new_v4(), stack);
```

This exposes arbitrary slot mutation and leaves no authoritative owner if entity creation fails. A retry can also generate another random ID and duplicate the resource.

#### Correct

```rust
let receipt = drop_slot_atomically(
    request,
    &gameplay_config,
    ManualDropAssets {
        inventory: inventory.inventory_mut(),
        pending: &mut pending,
        spawned: &spawned_ids,
    },
)?;
```

The domain transaction validates sequence/revision/slot, derives one stable ID, transfers the whole stack to pending ownership before clearing the slot, and can roll back pending if the final inventory commit fails.
