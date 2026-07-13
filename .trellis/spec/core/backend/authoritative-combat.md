# Authoritative Match Combat

## Scenario: Server-selected melee damage and one terminal asset outcome

### 1. Scope / Trigger

- Applies to `contracts/extraction/v1`, client combat reducers, `gameplay/combat`, `gameplay/death`, `engine_gameplay`, match World eviction, matchmaking terminal notices, and participant persistence.
- Trigger this spec when changing health, melee timing, target selection, player AABBs, block occlusion, death drops, reconnect timeout, participant terminal states, result snapshots, or combat dispatcher ordering.
- Combat is World-authoritative. A client submits a sequence and fixed weapon slot; it never submits a target, damage, health, killer, death event, team exemption, or loot contents.

### 2. Signatures and Wire Methods

```rust
pub fn decode_attack_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<AttackPayload>, ContractError>;

pub(crate) fn drop_inventory_on_death_atomically(
    request: DeathDropRequest,
    assets: DeathDropAssets<'_>,
) -> Result<DeathDropReceipt, DeathDropError>;

pub fn World::install_before_broadcast_system<T, F>(
    &mut self,
    name: &'static str,
    factory: F,
) -> Result<(), DispatcherHookError>;
```

Wire methods and Direct state streams:

```text
client -> server: pvp:v1:attack
client -> server: pvp:v1:get-state
server -> client: health
server -> client: deathResult
```

### 3. Contracts

- Every new match installs `20` integer half-hearts. Basic melee deals `2`, has a `600ms` inclusive cooldown boundary, and has `3.0` blocks maximum reach. There is no healing path. Rebind retains the entity and health; only a new match creates a fresh health component.
- Attack payload is exactly `{ weaponSlot: "melee" }` plus the top-level envelope sequence. Unknown fields, target IDs, damage, health, team/role metadata, and death claims are malformed.
- A new sequence is consumed before targeting. A miss consumes cooldown. A duplicate or older sequence never changes combat, health, stats, inventory, or loot. A cooldown rejection consumes the new sequence but does not move the last accepted swing time.
- Same-tick attacks are stably grouped by seat only. Each player's network arrival order is preserved; sorting by client sequence would incorrectly make a late old message valid.
- The server validates the attached authenticated attacker, account/public-player binding, live participant gate, alive health, no elimination record, and fixed basic melee equipment before sequence consumption.
- Candidate targets are every other alive, non-eliminated match player, including detached players during their reconnect window. There is no team or friendly-fire exemption.
- Target selection normalizes authoritative direction, intersects a `0.8 x 1.8` player AABB derived from the authoritative eye position, and chooses the smallest ray distance with stable seat ID as the tie breaker.
- Origin and direction must be finite and inside the match bounds. Missing or non-Ready chunks fail closed. Only a registered full unit-cube block occludes melee; unknown voxel types fail closed.
- Damage and terminal processing occur in the pre-Broadcast combat hook. Forced reconnect-timeout eliminations are drained before attacks, so timeout and melee in one tick observe one deterministic terminal result.
- Nonlethal damage commits one health revision and queues a Direct full health snapshot. Lethal damage first prepares health, mining reset, survival/resource stats, killer stats, and death result on clones.
- Terminal inventory drain aggregates every resource stack, freezes and clears the inventory, and transfers nonempty contents to pending ownership with ID `drop:v1:{match_id}:seat:{seat_id}:death`. Fixed equipment is outside the inventory and never drops.
- Pending insertion occurs before inventory commit and is rolled back if inventory commit fails. After the asset transaction succeeds, later component assignments must be infallible; code must not return an error after ownership moved to pending.
- `EliminationComp` is the World-local `Alive -> terminal` CAS. Its record owns the exact death result and a notice-sent flag. Later attacks or timeout requests skip terminal players and cannot produce another drop.
- Combat runs before Broadcast; the aggregate gameplay system runs after Broadcast. The victim receives final health/death Direct state before the death outbox can persist and evict them. Pending death loot is spawned and offered to auto-pickup after death; eliminated players are not pickup candidates.
- The World tick never waits for SQL. The death outbox submits a bounded internal notice containing match/account identity and resource statistics. A successful enqueue marks the local notice sent; queue overflow fails the match authority closed.
- Matchmaking validates match ID, world name, and world generation, closes gameplay/rebind gates, and generation-safely evicts the principal before waiting on SQL so an online victim cannot observe the match during database latency. Eviction has a finite timeout; failure becomes a Tick retry and never revives the participant. The exact idempotent repository CAS then records the terminal result.
- `mark_dead` accepts only `Active|Disconnected -> Dead`; `mark_timed_out` accepts only `Disconnected -> TimedOut` at `reconnect_deadline <= now`. Exact retries return `AlreadyApplied`; a different killer or statistics conflict. Dead and TimedOut cannot overwrite each other.
- A reconnect deadline only queues a forced World elimination. Matchmaking must not clear inventory or persist TimedOut before the World creates the unique terminal drop and reports the result.
- Dead/TimedOut/Extracted/Aborted participants may join the next waiting queue while the old match still exists. The queued snapshot takes precedence, but a full next roster cannot prepare another World until the current match is cleared.
- Final health and death result use the same terminal health revision. Client reducers ignore another match, stale revisions, and any later alive snapshot after a terminal result. Heart projection is always ten `full|half|empty` slots derived from server half-hearts.

### 4. Validation and Error Matrix

| Condition | Required result |
| --- | --- |
| Malformed envelope, weapon slot, or invented target/damage field | Reject before attack queue |
| Detached, mismatched identity, inactive, dead, or missing fixed weapon | `GAME_INVALID_STATE`; do not consume sequence |
| Duplicate/older sequence | Stale-sequence rejection; no state mutation |
| New sequence before 600ms | Cooldown rejection; consume sequence, preserve accepted swing time |
| No target within 3 blocks | Miss result; cooldown remains consumed |
| Nearest target behind full block | Miss; no damage |
| Missing/non-Ready ray chunk or unknown solid type | Retryable unavailable; fail closed |
| Nonlethal hit | Subtract exactly 2 half-hearts once and send full health state |
| Tenth full-health hit | Commit one Dead result and at most one death drop |
| Timeout and lethal attack in one tick | Timeout resolves first; one terminal record/drop |
| Duplicate death/timeout notice with exact data | `AlreadyApplied` |
| Duplicate terminal notice with different killer/stats | Conflict; preserve first terminal row |
| Old World generation notice or eviction callback | Ignore; do not mutate the current match |
| Dead player tries attack, mining, drop, pickup, extraction, or rebind | Reject without gameplay/asset mutation |

### 5. Tests Required

- Domain tests assert ten `2`-unit hits from `20`, no healing, exact `599/600ms` cooldown, sequence exhaustion/duplicates, finite ray-AABB behavior, and round-stat overflow rollback.
- Death transaction tests assert empty/nonempty inventory freeze, aggregate resource conservation, deterministic ID, pending collision, rollback, and duplicate prevention.
- ECS tests assert identity rejection, nearest target selection, full-block occlusion, detached target damage, one lethal drop, dead-target exclusion, and timeout-before-attack uniqueness.
- Dispatcher tests assert the named combat hook is installable once, runs before Broadcast, conflicts with reserved names, rejects custom dispatchers, and preserves later extensions.
- Matchmaking tests assert online/disconnected death, timeout request-before-persistence, exact deadline, stats persistence, failed and hanging eviction retry, rebind denial, fail-closed recovery, stale generation isolation, and terminal requeue without a parallel World.
- PostgreSQL tests assert row-lock/CAS idempotency, exact timeout boundary, killer/resource-stat round trip, conflicting retry rejection, and Dead/TimedOut mutual exclusion. Compile DB tests when no disposable database URL is available; do not claim execution.
- Shared Rust/TypeScript tests assert strict attack shape, full health/death snapshots, killer/cause consistency, and get-state round trips. Client tests assert stale/wrong-match rejection, terminal latching, and ten-heart projection.
- Run extraction-server engine all-target tests, application `--no-deps -D warnings` Clippy, root library tests, extraction-client tests/build, E2E typecheck/actor tests, and `git diff --check`.

### 6. Wrong vs Correct

#### Wrong

```rust
let target = payload.target_id;
target.health -= payload.damage;
spawn_drop(Uuid::new_v4(), target.inventory.drain());
```

#### Correct

```rust
let target = select_combat_target(context, chunks, registry, origin, direction, &candidates)?;
let result = resolve_melee_death(MeleeDeathAccess { /* authoritative assets */ })?;
queue_death_result(queues, victim_client_id, &result);
```

The server derives the target and damage, and the terminal transaction transfers inventory ownership once before the generation-safe persistence and eviction path runs.
