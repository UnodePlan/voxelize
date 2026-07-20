# Match Lifecycle Persistence

## Scenario: Capacity-N matches backed by disposable Worlds (production N=10)

### 1. Scope / Trigger

- Applies to `apps/extraction-server/src/matchmaking`, `ports/match_repository.rs`, `ports/match_world_runtime.rs`, PostgreSQL match persistence, queue HTTP routes, and the reusable engine lifecycle messages used by the application.
- Trigger this spec when changing queue admission, roster construction, match or participant states, match deadlines, disconnect/reconnect behavior, World creation/removal, process-start recovery, match table constraints, or **runtime match capacity**.
- The first release coordinates one open match in one process. A dedicated PostgreSQL advisory-lock connection rejects a second matchmaking process for the same database. It intentionally does not provide Redis coordination, cross-process World migration, or recovery of an in-memory gameplay World after process loss.
- **Production capacity is always `MATCH_SIZE = 10`.** Local DEV may lower capacity via env (see Contracts); production binaries must never default to N&lt;10.

### 2. Signatures

```rust
#[async_trait]
pub trait MatchRepository: RepositoryProbe + Send + Sync {
    async fn create_preparing(
        &self,
        command: CreatePreparingMatch,
    ) -> Result<StoredMatch, MatchRepositoryError>;

    async fn find_match(
        &self,
        match_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError>;

    async fn find_nonterminal_by_account(
        &self,
        account_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError>;

    async fn abort_unrecoverable_matches(
        &self,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<u64, MatchRepositoryError>;

    async fn activate(
        &self,
        match_id: Uuid,
        started_at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn mark_disconnected(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn reconnect(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn time_out(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn open_extraction(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn begin_settling(
        &self,
        match_id: Uuid,
        trigger: SettlingTrigger,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn finish(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn abort(
        &self,
        match_id: Uuid,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;
}

#[async_trait]
pub trait MatchWorldRuntime: Send + Sync {
    async fn prepare_world(
        &self,
        spec: MatchWorldSpec,
    ) -> Result<PreparedMatchWorld, MatchWorldRuntimeError>;
    async fn stop_world(&self, match_id: Uuid, world_name: &str)
        -> Result<bool, MatchWorldRuntimeError>;
    async fn despawn_detached(&self, world_name: &str, account_id: Uuid)
        -> Result<bool, MatchWorldRuntimeError>;
}

pub struct PreparedMatchWorld {
    pub world_generation: String,
}

pub trait ConnectionLifecycleObserver: Send + Sync {
    fn observe(&self, event: &ConnectionLifecycleEvent);
}

pub struct PrepareWorld {
    pub name: String,
    pub expected_generation: Option<String>,
}

pub struct DespawnDetachedPrincipal {
    pub account_id: String,
    pub world_name: String,
    pub world_generation: String,
}
```

`World::set_client_attach_guard` is the synchronous admission boundary for both `Join` and `Rebind`. The guard must be fast and derived from the coordinator's current immutable gate snapshot. Its only permitted mutation is registering the exact bounded `(account, world, generation, client, attach_attempt)` reservation used to linearize the later lifecycle receipt; it must not perform I/O or mutate match/domain state.

### 3. Contracts

- One bounded coordinator command queue serializes HTTP queue commands, engine lifecycle events, and clock ticks. No caller may mutate queue or live-match state directly. Ticker submission is single-flight: at most one ticker-owned Tick may be queued. Queue overflow fails the attach gate closed and schedules an abort of the current match.
- A new queue request requires an authenticated account, at least one currently connected game socket, a bound `MatchWorldRuntime`, and no persisted nonterminal seat. Wallet balance, warehouse contents, and client-supplied loadout fields never affect admission or initial stats.
- FIFO is ordered by `(enqueued_at, in_process_order)`. Repeated enqueue is idempotent. Waiting disconnect removes the account. A failed Preparing batch restores only still-connected players using their original enqueue time and order.
- **Runtime `match_size` (capacity N)** is fixed for the process lifetime:
  - Default / production: `N = MATCH_SIZE = 10`.
  - DEV only: if `EXTRACTION_DEV_MATCH_MODE=true`, read optional `EXTRACTION_DEV_MATCH_SIZE` as integer in `2..=10` (default **2** when mode on and size unset). If mode is false/unset, **ignore** `EXTRACTION_DEV_MATCH_SIZE` and keep `N=10`.
  - Queue freeze threshold, `FrozenRoster` length, Join activation count, and World max clients all use the same `N`. Never hard-code array length 10 in domain types that must honor capacity.
- `FrozenRoster` contains exactly **N** unique account IDs, **N** unique public player IDs, and seats `0..=N-1` (stored as a length-N collection, not a fixed `[T; 10]`). The first `N-1` remain queued; the **Nth** freezes the roster. While that match is nonterminal, an outsider receives `Full` and cannot attach. Terminal cleanup permits a new open match.
- Creating a match persists the complete frozen roster before preparing the World. Match creation locks account IDs in UUID order. If the create response is uncertain, retry the exact same match ID, roster, seed, and versions; an exact persisted command is idempotent, while any mismatch is a conflict. Lifecycle writes lock the match row before participant rows. Multi-row reads that must agree use `REPEATABLE READ`.
- Match state is one-way: `Waiting -> Preparing -> Active -> ExtractionOpen -> Settling -> Finished`, with `Aborted` allowed only from a nonterminal state. Participant state follows the explicit domain transition table; terminal participant states never return to Active.
- `Preparing` becomes `Active` only after **all N** authoritative `JoinCommitted` events. A disconnect carrying the current World, generation, public client ID, and attach attempt aborts that Preparing batch even if the account has another lobby socket. Connected accounts are restored with their original FIFO keys. It never creates gameplay results or permanent assets.
- Each match gets a disposable `saving(false)` World named from its match UUID. The engine chunk envelope is `[-10,-10]..[9,9]` (320 blocks), while authoritative playable X/Z bounds are exactly `[-150,150)` (300 by 300). Capacity **N** (production 10), backpack capacity twelve, twenty half-hearts, and the fixed loadout are immutable match configuration. Installing those values as authoritative player ECS components belongs to inventory/combat stages and must not be claimed from metadata-only tests.
- **Local multiplayer gotcha**: after a protocol/browser smoke leaves a match nonterminal, the next N-player queue receives `MATCH_FULL` / lobby stall until reconnect timeout (~60s) or terminal finish. Automation must wait or abort before reusing seats. Client DEV entry `?mode=dev-mp&seat=` is gated by `import.meta.env.DEV` and must not ship in production bundles.
- Persist `seed`, `generation_version`, `gameplay_version`, and `config_version`. The engine seed is derived only by the named `engine_seed_v1` fold. Reusing a World name must create a new generation; delayed prepare, detach, rebind, or despawn work must compare that generation before mutating state.
- Activation persists absolute UTC deadlines at `started_at + 8m`, `started_at + 12m`, and hard deadline `+ 30s` settlement grace. Runtime scheduling derives monotonic deadlines from the persisted UTC values after the write returns. A separate watchdog closes the synchronous gate and stops the World at the hard deadline even while the coordinator awaits database I/O; its `(world, generation)` closure is latched so a stale Active sync cannot reopen gameplay. The queued Tick later persists Settling/Finished.
- Active disconnect detaches the entity and records a reconnect deadline. Rebind is allowed only for the same account and World before the deadline (`now < deadline`). Rebind admission and timeout claim share one account-level lock: an attach reserved before the deadline may commit after it, while a timeout claim that linearizes first prevents a stale snapshot from adding a reservation. A rejected/disconnected attach clears only its exact reservation. At exactly sixty seconds (`now >= deadline`), a participant without an admitted attempt is claimed once for terminal timeout and generation-safe despawn, with retry allowed only while persistence/despawn remains pending.
- A socket close may arrive after hard-deadline eviction or another terminal transition. Only an `Active` participant may enter `mark_disconnected`; `Dead`, `TimedOut`, `SettlementPending`, `Extracted`, and `Aborted` closes are terminal no-ops. A late close must never turn a completed result into `connection_event_failed` or abort the next lobby flow.
- Settling and abort paths close the gate and stop the World before awaiting lifecycle persistence. Every runtime stop attempt has a finite timeout; the hard-deadline watchdog retries a timed-out/error result, while a later coordinator Tick retries an incomplete stop. If persistence fails, later Ticks retry the write without reopening gameplay or stopping the World twice.
- `World::stop` clears clients and transports, closes its background-task tracker, and waits until every accepted generation, meshing, encoded-message, and dispatcher job releases its RAII permit before emitting the stopped acknowledgement. Merely signaling an actor or Arbiter to stop is not physical World release, and late work is rejected after the tracker closes.
- Disposable Worlds share one process-wide Rayon pool instead of creating pools per match. E2E lifecycle diagnostics expose live World instances and in-flight World background tasks; both must be zero before a match is considered released.
- If an uncertain create may already be committed, cancel/disconnect must first look up and abort that exact prepare attempt before mutating its queue entry. On lookup/abort failure, retain the attempt and fail closed; never clear the attempt and orphan a persisted Preparing match.
- `Finished` and `Aborted` close admission, remove the World generation route, pending World ticks/requests, connection reservations, and all match-local coordinator state. Stop/remove/despawn operations are idempotent.
- Before matchmaking starts, process startup first obtains the process advisory lock, then atomically marks every persisted nonterminal match and nonterminal participant Aborted with a restart reason. Terminal participants and committed settlements are preserved. Recovery uses bounded PostgreSQL lock/statement timeouts; failure aborts service startup, repeated recovery returns zero, and a second live process receives `AddrInUse` before recovery can abort the first process's match.
- The advisory lock connection is deliberately held for the server lifetime. Until lock-health monitoring is implemented, deployments must avoid overlapping replacement processes after a PostgreSQL/network session loss; losing that dedicated session is an operational stop-and-restart condition, not permission to run two live gameplay processes.

### 4. Validation & Error Matrix

| Condition                                                              | Required result                                                                             |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Authenticated HTTP session has no connected game socket                | `MATCH_ROSTER_LOCKED`; do not queue                                                         |
| No bound World runtime or coordinator unavailable                      | `503` / `MATCHMAKING_UNAVAILABLE`; do not queue                                             |
| Same account queues again while waiting                                | Return its existing position and timestamp                                                  |
| Account already has a nonterminal persisted seat                       | `MATCH_ROSTER_LOCKED`                                                                       |
| Nth connected unique account queues (production N=10)                  | Persist exactly N seats, freeze roster, prepare one World                                   |
| (N+1)th account while a match is nonterminal                           | `MATCH_FULL`; never enter the World gate                                                    |
| `EXTRACTION_DEV_MATCH_MODE` unset/false                                | Effective capacity 10; ignore `EXTRACTION_DEV_MATCH_SIZE`                                   |
| DEV mode true, size unset                                              | Effective capacity 2                                                                        |
| DEV mode true, size outside 2..=10                                     | Config error at startup                                                                     |
| Roster account disconnects during Preparing                            | Abort and stop World; restore connected peers in original FIFO order                        |
| Preparing attach socket disconnects while a backup lobby socket exists | Abort the old batch; the connected account may enter a new batch with its original FIFO key |
| Join or rebind is not present in the frozen gate snapshot              | Deny before World client state changes                                                      |
| Rebind admitted at `deadline - 1ms`, receipt arrives later             | Commit the exact reserved attempt; timeout claim must not overtake it                       |
| Fresh Rebind at `deadline` or later with no reservation                | Deny; claim once, transition to TimedOut, and despawn once                                  |
| Delayed/duplicate Join or Rebound receipt                              | Consume only its exact attempt; stale or repeated receipts are no-ops                       |
| Delayed callback carries a stale World generation                      | Ignore/reject without touching the replacement World                                        |
| Socket close arrives after participant became terminal                 | Ignore it; preserve the existing terminal result and match outcome                          |
| Hard deadline reached                                                  | Enter Settling, time out remaining active/disconnected participants, finish, stop World     |
| Settling persistence fails after local stop                            | Keep gate closed and World stopped; retry the same transition on a later Tick               |
| Queue changes after an uncertain create result                         | Resolve and abort the exact prepare attempt before removing the queue entry                 |
| Process starts with persisted nonterminal matches                      | Atomically abort them before opening matchmaking; preserve terminal asset results           |
| A second process targets the same database                             | Fail startup before recovery or matchmaking mutation                                        |
| Repeated lifecycle transition or cleanup                               | Return `AlreadyApplied`/`false` or equivalent idempotent success                            |
| Stop acknowledgement is requested while World work is in flight        | Close admission to new work, wait for all accepted permits, then acknowledge                |
| Logical route maps are empty but a World/task remains physically live  | Keep the release gate closed and report the non-zero lifecycle diagnostic                   |

### 5. Good / Base / Bad Cases

- Good (production): ten connected accounts queue in FIFO order, persistence freezes seats `0..9`, one disposable World reaches Ready, all ten Join receipts commit, and only then the match activates.
- Good (DEV N=2): two SIWE sessions with DEV mode queue, freeze seats `0..1`, activate after both Join receipts; unit test `dev_match_size_two_forms_roster_without_ten_players`.
- Base: fewer than N accounts stay Waiting; a repeated queue call returns the same position; a waiting disconnect simply removes that account.
- Bad: accepting queue traffic before a World runtime is bound, activating after only some Join callbacks, reconstructing FIFO timestamps after an abort, generating a new match ID after an uncertain create, trusting client loadout fields, despawning by World name without generation comparison, or shipping production defaults with N&lt;10 / leaking `?mode=dev-mp` into production bundles.

### 6. Tests Required

- Domain tests for exact roster size at capacity N (including N=2 and N=10), duplicate account/public ID rejection, seats, match transitions, participant transitions, and `+8m/+12m/+30s` deadlines.
- Coordinator tests with an injectable UTC/monotonic clock for sequential and concurrent (N-1)/N/(N+1) admission (production 9/10/11), no-runtime failure closure, uncertain-create retry, Preparing disconnect restoration with backup sockets, all-N activation, terminal requeue, stop-before-persist retry, and repeated cleanup.
- Config tests: DEV mode off → size ignored; DEV mode on → default 2 and clamp/reject invalid sizes.
- Boundary tests at `59.999s` and `60.000s`, including both linearization orders between an admitted Rebind and timeout claim, exact rejection cleanup, one terminal timeout, and one generation-safe despawn despite repeated ticks.
- Reconnect tests must close the physical socket after hard deadline and after death, then assert the terminal result remains unique, no `connection_event_failed` abort is recorded, and a later queue attempt starts from lobby state.
- World specification tests for the 320-block engine envelope, exact 300-by-300 playable bounds, fixed capacity/health/backpack/loadout, disabled saving, and deterministic named seed fold.
- PostgreSQL tests for exact-ten atomic creation, duplicate/nonterminal seat exclusion, UUID lock order behavior, idempotent transitions, reconnect boundary, hard-deadline settlement, process-lock exclusion/reacquisition, startup recovery/asset preservation, and transaction rollback. Tests that invoke global startup recovery or the process advisory lock must be serialized within their shared test database.
- Root engine actor tests for attach-guard denial before mutation, lifecycle observer events carrying the exact attach attempt, generation-checked prepare, detach/rebind rejection restoration, idempotent remove, and stale-generation despawn isolation.
- Background lifecycle tests cover one shared pool, close-and-wait behavior, late-job rejection, RAII release, and physical World/task counters. Multi-round E2E must observe three consecutive released snapshots after each round before starting the next.

### 7. Wrong vs Correct

#### Wrong

```rust
queue.push(account_id);
if queue.len() >= 10 {
    world.add_clients(queue.drain(..10));
    repository.save_match_later();
}
```

This hard-codes capacity 10, exposes an unfrozen roster, mutates the World before durable admission, and gives concurrent callers no single ordering boundary.

#### Correct

```rust
let snapshot = matchmaking.enqueue(authenticated_account_id).await?;
// The coordinator serializes the command, freezes when queue.len() == match_size,
// persists an exact FrozenRoster of length N, prepares one generation-tagged World,
// and publishes an immutable attach gate.
return Ok(snapshot);
```

The coordinator command is the ordering boundary, the frozen database roster is the durable admission boundary, and `JoinCommitted` from all **N** World receipts is the activation boundary.
