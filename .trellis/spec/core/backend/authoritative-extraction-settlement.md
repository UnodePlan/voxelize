# Authoritative Extraction and Settlement

## Scenario: Deadline-bounded extraction and exactly-once permanent assets

### 1. Scope / Trigger

- Applies to extraction wire contracts, `gameplay/extraction`, `engine_gameplay`,
  hard-deadline World control, matchmaking settlement coordination, settlement
  repositories, PostgreSQL migrations, result HTTP routes, and warehouse reads.
- Trigger this spec when changing the extraction zone, hold duration, deadline
  boundaries, terminal outbox order, inventory digest, settlement retries,
  transaction locks, reconciliation, result statuses, or warehouse statistics.
- Match inventory is temporary World state. Only a committed settlement may
  create permanent warehouse or ledger assets.

### 2. Signatures

Wire state:

```text
server -> client: pvp:v1:extraction-state

hidden {}
open { zone, inside, elapsedMs, requiredMs, hardDeadlineUnixSeconds }
pending { zone, qualifiedAtUnixSeconds }
closed {}
```

Runtime and repository boundaries:

```rust
pub trait MatchWorldRuntime {
    async fn seal_hard_deadline(
        &self,
        world_name: &str,
        monotonic_deadline: Duration,
        utc_deadline: OffsetDateTime,
    ) -> Result<bool, MatchWorldRuntimeError>;
}

pub trait SettlementRepository {
    async fn mark_settlement_pending(
        &self,
        qualification: ExtractionQualification,
    ) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError>;

    async fn commit_settlement(
        &self,
        command: CommitSettlement,
    ) -> Result<TransitionOutcome<SettlementRecord>, SettlementRepositoryError>;

    async fn find_settlement(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<SettlementRecord>, SettlementRepositoryError>;
}
```

Authenticated result routes:

```text
GET /api/matches/{match_id}/result
GET /api/matches/latest-result
```

Stage 8 migration adds these participant terminal fields:

```sql
terminal_cause TEXT NULL
terminal_at TIMESTAMPTZ NULL
survived_ms BIGINT NULL CHECK (survived_ms BETWEEN 0 AND 4294967295)
```

### 3. Contracts

- Extraction remains hidden before `started_at + 8m`. One seed-selected zone is
  published when the persisted match reaches `ExtractionOpen`.
- Qualification requires one continuous server-observed eight-second window.
  Leaving the cylinder, dying, or detaching clears the unfinished window.
- The hard deadline is `started_at + 12m` and is inclusive: a qualification
  calculated exactly at the deadline is valid. Time after the deadline cannot
  add progress that was missing at the deadline.
- Combat and reconnect-timeout death resolve before extraction in the same
  tick. At the hard deadline, the World order is:

```text
close gate
-> resolve combat/death
-> resolve exact-boundary extraction
-> hard-terminalize every remaining player
-> Broadcast final Direct state
-> enqueue death/extraction outboxes
-> seal World terminal outboxes
-> stop World
-> persist Settling/Finished
```

- `seal_hard_deadline` must return `Ok(true)` before matchmaking can enter
  Settling. `Ok(false)`, an error, or a timeout means the World did not prove
  sealing; matchmaking fails closed and aborts the match. In contrast,
  `stop_world(...)=Ok(false)` is an idempotent no-op because an already absent
  World is stopped for lifecycle purposes.
- Qualification clones and freezes the exact twelve-slot inventory before
  committing the extraction record. Mining, attacks, drops, pickup, death, and
  rebind cannot mutate a `SettlementPending` participant.
- Permanent resources are the aggregated `dirt/gold/diamond` counts from that
  frozen inventory. Canonical JSON uses the fixed key order
  `diamond,dirt,gold`; its SHA-256 is the inventory digest. The idempotency key
  is `extract:v1:{match_id}:{account_id}`.
- The World tick never waits for SQL. It places an immutable qualification on
  the bounded matchmaking channel. Queue failure fails gameplay authority
  closed instead of dropping the asset command.
- Settlement writes use database `transaction_timestamp()` and set local
  `lock_timeout=5s` plus `statement_timeout=15s`. Lock order is always match,
  participant, account, then settlement/assets.
- One transaction inserts the settlement and nonzero items, appends one ledger
  row per item, increments warehouse balances, and transitions the participant
  to Extracted. Any error rolls back all five effects.
- A retry is identified by `(match_id, account_id)`, digest, config version,
  resources, qualification time, and idempotency key. It must not require a new
  process to regenerate the original `settlement_id`.
- Empty inventories are valid zero-value settlements with no item or ledger
  rows. Per-item balances, cumulative ledger resources, and cumulative
  settlement value are checked against `i64::MAX` before writes.
- A commit response error is `OutcomeUnknown`, never assumed to be rollback.
  The coordinator performs `find_settlement` before another write. A found
  matching record completes successfully; a conflicting record fails closed.
- The settlement write grace deadline is hard deadline plus thirty seconds.
  Writes may start at the exact deadline but never when database time is later.
  After grace, coordination performs only bounded reads and never mark, commit,
  abort, or compensating asset writes.
- PostgreSQL result reads use one repeatable-read, read-only snapshot. A
  post-grace pending participant with no settlement is projected as Aborted
  without changing the audit row. A committed settlement remains Extracted.
- Result routes derive the account only from the authenticated session. A
  missing match or a match owned by another account returns `200 null` to avoid
  enumeration. Responses and errors use `Cache-Control: no-store` and omit
  account IDs, digests, idempotency keys, and internal abort reasons.
- Result status is one of `pendingReconciliation`, `extracted`, `dead`,
  `timedOut`, or `aborted`. Dead and timed-out results require a terminal cause,
  terminal time, survival duration, and complete resource statistics.
- Warehouse balances and settlement/ledger aggregates are read in one
  repeatable-read, read-only snapshot. There is no mutation, consumption,
  transfer, or replay endpoint in the MVP.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Zone hidden before eight minutes | No zone coordinates in the payload |
| Leave, death, or detach during hold | Reset progress; require a new full eight seconds |
| Qualification exactly at hard deadline | Accept once and freeze inventory |
| Qualification requiring time after hard deadline | Reject and hard-terminalize |
| Hard-deadline seal returns `Ok(false)` | Fail closed and abort; never finish |
| Terminal outbox is not enqueued | Do not seal or stop as a successful finish |
| Duplicate identical qualification | Idempotent read/commit result; no extra assets |
| Same key with different digest/resources/time | Conflict; preserve the first truth |
| Commit response is lost | Read unique settlement before any retry |
| Database time is after grace | No write; read-only reconciliation only |
| Balance or aggregate would exceed `i64::MAX` | Roll back the complete transaction |
| Settlement items differ from ledger rows | Repository invariant error |
| Unauthenticated result request | `401` with no-store |
| Authenticated request for another account | `200 null` |
| Extracted result has no settlement | Service unavailable invariant failure |

### 5. Good / Base / Bad Cases

- Good: a player remains inside from `11:51.999` through the exact twelve-minute
  boundary, freezes once, receives one committed settlement, and later reads
  the same result after reconnecting.
- Base: a player leaves after seven seconds, re-enters, and must hold another
  complete eight seconds. A zero-resource extraction still commits once.
- Bad: treating `Result<bool>::is_ok()` as proof of a hard-deadline seal,
  awarding warehouse balances before commit, retrying blindly after an unknown
  commit, writing after grace, or exposing another account's result.

### 6. Tests Required

- Domain tests cover `8m`, continuous `8s`, inclusive `12m`, leave/death/detach
  reset, bounded cylinder geometry, frozen inventory, canonical digest, and
  revision overflow behavior.
- ECS tests cover combat-before-extraction, pending interaction rejection,
  exact-boundary qualification, hard-deadline terminalization, final Direct
  state, outbox-before-seal, and no seal while any notice remains unsent.
- Coordinator tests cover `seal=true/false/error/timeout`, duplicate and stale
  generation notices, exact qualification replay, response loss, read-before-
  write, no post-grace writes, bounded reconciliation, and old-match isolation.
- PostgreSQL tests cover concurrent identical commits, conflicting digest/time,
  empty settlement, transaction rollback, all overflow dimensions, item/ledger
  drift, terminal result round trips, and post-grace read projection. Compile
  these tests when no disposable database URL is available; do not claim run.
- HTTP tests cover authentication, malformed UUID, no-store, account isolation,
  all five statuses, terminal shape validation, and hidden internal fields.
- Shared Rust/TypeScript fixtures cover hidden/open/pending/closed strict shapes,
  nested match IDs, revisions, and frozen/dead gameplay snapshots.
- Run server format, engine all-target tests, `engine,db-tests` check/Clippy,
  client tests/build/lint, E2E actor tests, and `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
let sealed = runtime.seal_hard_deadline(world, mono, utc).await.is_ok();
repository.add_balance(account_id, inventory.resources()).await?;
```

`Ok(false)` is not a seal, and a standalone balance write can survive without
the settlement, ledger, item rows, or participant transition.

#### Correct

```rust
let sealed = matches!(
    runtime.seal_hard_deadline(world, mono, utc).await,
    Ok(true)
);
let outcome = repository.commit_settlement(frozen_command).await;
```

The explicit boolean proves World sealing, while the repository owns one
transactional permanent-asset boundary and reconciliation handles uncertainty.
