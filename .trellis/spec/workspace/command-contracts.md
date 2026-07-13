# Workspace Command Contracts

## Scenario: Cross-Platform pnpm Workspace Filters

### 1. Scope / Trigger

- Applies whenever root `package.json`, `pnpm-workspace.yaml`, or CI changes package selection or aggregate extraction commands.
- The scripts must behave the same under POSIX shells and Windows `cmd.exe`.

### 2. Signatures

```json
{
  "build": "pnpm -r --fail-if-no-match --filter \"./packages/*\" --filter \"!@voxelize/core\" build && pnpm --filter @voxelize/core --fail-if-no-match build",
  "build:extraction": "cargo check --manifest-path apps/extraction-server/Cargo.toml --all-targets --features engine --locked && ...",
  "test:extraction": "cargo test --manifest-path apps/extraction-server/Cargo.toml --all-targets --features engine --locked && ..."
}
```

CI may use YAML single quotes because the runner is explicitly Ubuntu. JSON scripts must use escaped double quotes.

### 3. Contracts

- `apps/*` is a workspace package pattern.
- Root engine builds select only `./packages/*` before building `@voxelize/core`; extraction applications must not be included by an `@voxelize/*` wildcard accidentally.
- Positive filters use `--fail-if-no-match` so a quoting regression cannot silently skip every selected package.
- Every required single-package filter also uses `--fail-if-no-match`; a missing, renamed, or unregistered package must fail both local aggregates and CI.
- `build:extraction` checks the engine-linked Rust server plus client build and actor type check.
- `test:extraction` tests the engine-linked Rust server, client, and actor harness.
- Lockfile changes for a new application add only that importer unless an approved dependency change requires more.
- CI path filters include root `.npmrc`, `tsconfig*.json`, and `vitest*.ts` because they can change installation, build, or test behavior without touching an application directory.

### 4. Validation & Error Matrix

| Condition                                          | Required result                                        |
| -------------------------------------------------- | ------------------------------------------------------ |
| JSON script uses shell single quotes around a glob | Reject in review; Windows passes quotes literally      |
| Any required positive filter selects no package    | Command exits non-zero via `--fail-if-no-match`        |
| New app importer rewrites unrelated snapshots      | Restore unrelated lockfile entries                     |
| Extraction aggregate omits the Rust server         | Reject; aggregate name covers all Stage 0 applications |
| Extraction aggregate omits `--features engine`     | Reject; the default skeleton cannot prove engine links |
| CI regenerates ignored protocol source             | Require generation and downstream build/tests to pass  |

### 5. Good/Base/Bad Cases

- Good: escaped double quotes in JSON, explicit path/name filters, and a fail guard on every required positive filter.
- Base: YAML single quotes on the fixed Ubuntu CI runner.
- Bad: `--filter './packages/*'` inside JSON; Windows can report no matches and still appear successful without the fail guard.

### 6. Tests Required

- Run the positive package filter with `--fail-if-no-match` and confirm the expected legacy package count.
- Run `pnpm install --frozen-lockfile` after workspace changes.
- Run `pnpm build:extraction`, `pnpm test:extraction`, and `pnpm lint:extraction`.
- Inspect `pnpm-lock.yaml` and confirm unrelated package snapshots did not change.
- Generate the existing protocol, then build downstream packages and run tests. `packages/protocol/src/protocol.*` is intentionally ignored, so `git diff` is not a valid synchronization assertion.

### 7. Wrong vs Correct

#### Wrong

```json
"build": "pnpm -r --filter './packages/*' build && pnpm --filter @voxelize/core build"
```

#### Correct

```json
"build": "pnpm -r --fail-if-no-match --filter \"./packages/*\" build && pnpm --filter @voxelize/core --fail-if-no-match build"
```

## Scenario: Owned E2E Process Supervision

### 1. Scope / Trigger

- Applies to `apps/extraction-e2e/scripts/process-supervisor.mjs`, its POSIX/Windows process-tree helpers, and every live gameplay, reconnect, crash, capacity, or browser runner.
- Trigger this contract when a runner starts Cargo, Vite, Vitest, Playwright, or a test-controlled Engine, or when changing command timeouts, signal handling, database guards, or artifact paths.
- The goal is bounded cleanup of processes created by the current runner. It never authorizes terminating a user's service merely because it shares a name or port.

### 2. Signatures

```js
class ProcessSupervisor {
  startCommand(label, command, args, options = {}); // long-lived child
  async runCommand(label, command, args, options = {}); // must exit zero
  async supervise(operation);
  async shutdown();
}

class PosixProcessTreeTracker {
  start();
  async stop(); // { discoveryError, groups: Array<{ depth, group }> }
}
```

`options.environment` overrides the supervisor environment for one command. `options.timeoutMs` is a positive bounded deadline for a finite command.

### 3. Contracts

- Each top-level POSIX command starts in an independent process group whose root PID equals its PGID. Windows cleanup starts from the exact owned child PID.
- While a POSIX root is alive, the tracker periodically reads only `pid,ppid,pgid`, walks that root's PPID descendant closure, and accumulates the discovered PGIDs with their child depth. This history preserves ownership after a nested detached Engine is reparented when pnpm/Vitest/Playwright exits.
- Cleanup stops discovery, merges the final root closure with previously proven groups, signals child groups before parent groups, sends `SIGTERM`, waits five seconds, then sends `SIGKILL` only to surviving owned groups and waits two more seconds. `ESRCH` is already-stopped success; other errors are reported.
- Process names, executable paths, listening ports, and broad pattern matches are diagnostics only and must never select a process for termination. A PGID is killable only when it came from the runner's detached root or a PPID chain rooted there.
- `SIGINT`, `SIGTERM`, orchestration timeout, command failure, and normal completion all converge through the same shutdown path. After an interrupt, no new child may start.
- A failed owner cleanup retains that owner, clears its in-flight stopping promise, and lets the final shutdown retry once. Operation and cleanup failures are both preserved in the returned error; cleanup failure must not be hidden by the original test failure.
- Live runners accept only the local exclusive database name `voxelize_extraction_e2e`, clear unrelated settlement crash injection unless the controlled server explicitly selects it, and use unique non-destructive artifact paths. Runners do not delete prior artifacts.
- Smoke orchestration remains `migration -> optional Engine -> Vite -> Playwright`; multi-round uses 4215/5215 and starts its controlled Engine inside the test. Gameplay/reconnect orchestration remains `migration -> Vitest`. Finite commands use a 600-second deadline; total smoke and gameplay-family orchestration use bounded 15/30-minute deadlines.

### 4. Validation & Error Matrix

| Condition                                               | Required result                                                              |
| ------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Spawned POSIX root is not leader of its own group       | Fail ownership validation; do not infer another PGID                         |
| Detached descendant is observed before its parent exits | Accumulate its PGID and clean it even after reparenting                      |
| Process table snapshot fails                            | Clean already proven groups, report discovery failure, never widen selection |
| Unrelated process has the same command or port          | Exclude it because no owned PPID lineage exists                              |
| Child group ignores `SIGTERM`                           | Escalate only that surviving owned group to `SIGKILL`                        |
| First cleanup attempt fails                             | Retain owner and retry once during final shutdown                            |
| Signal/timeout arrives while a command is running       | Reject orchestration, forbid new spawn, then perform bounded cleanup         |
| Database URL is remote or names another database        | Reject before migration or server startup                                    |
| Finite child exits non-zero or exceeds its deadline     | Fail the gate and still clean all long-lived owned children                  |

### 5. Good / Base / Bad Cases

- Good: Vitest starts a detached controlled Engine, the tracker records its PGID, Vitest crashes, and shutdown terminates the Engine group before reporting both the test and any cleanup error.
- Base: migration exits zero and its process group is already absent; cleanup treats `ESRCH` as success and proceeds to the next command.
- Bad: `pkill cargo`, killing whatever listens on 4215, tracking only the immediate pnpm group, forgetting an owner after the first cleanup error, or deleting the artifact directory before a run.

### 6. Tests Required

- Pure Node tests parse process tables, reject malformed rows/root group mismatch, collect only recursive descendants, preserve child-first order, merge historical groups, and exclude unrelated users' groups.
- Supervisor tests cover normal exit, spawn error, command timeout, signal interruption, detached descendant reparenting from historical discovery, TERM-to-KILL escalation, process-table failure fallback, cleanup retry, combined errors, and Windows PID-tree dispatch.
- Smoke orchestration tests use a fake supervisor to pin migration/optional Engine/Vite/Playwright ordering, gate-specific ports/specs, environment overrides, exclusive database validation, and unique artifact paths.
- Before release, run both Node test files, ESLint, Prettier, `node --check`, then execute each live runner serially and verify its declared ports have no owned listeners afterward while unrelated user services remain alive.

### 7. Wrong vs Correct

#### Wrong

```js
await playwright;
execFileSync("pkill", ["-f", "voxelize-extraction-server"]);
```

The immediate parent has already exited, the detached Engine may be reparented, and the name match can terminate an unrelated user service.

#### Correct

```js
const supervisor = new ProcessSupervisor(options);
await supervisor.supervise((owned) =>
  owned.runCommand("Playwright", "pnpm", args),
);
```

The supervisor records descendant groups while ownership is still provable, then performs bounded child-first cleanup without using names or ports as authority.

## Scenario: Live Lifecycle and Memory Release Gates

### 1. Scope / Trigger

- Applies to the E2E resource endpoint, resource snapshot decoder, five-round gameplay gate, same-page multi-round gate, and their JSON artifacts.
- Trigger this contract when adding a server resource counter, changing World shutdown, changing allocator instrumentation, or changing cross-round comparisons.

### 2. Signatures

```ts
interface MemoryResourceCounts {
  liveAllocatedBytes: number;
  peakAllocatedBytes: number;
  liveAllocations: number;
  allocationCount: number;
  deallocationCount: number;
  reallocationCount: number;
  liveWorldInstances: number;
  worldBackgroundTasks: number;
}

function releaseLifecycleProfile(
  snapshot: LiveResourceSnapshot,
): ReleaseLifecycleProfile;
```

### 3. Contracts

- `e2e-control` installs a tracking wrapper around the system allocator and accounts for `alloc`, `alloc_zeroed`, `dealloc`, and `realloc`. Production builds without that feature retain the normal allocator.
- A release poll requires three consecutive valid snapshots, 125 ms apart. World lists, server/coordinator routes and tasks, live World instances, and World background tasks must be at their exact released values before memory is sampled.
- The five-round primary leak signal is `liveAllocatedBytes` after full release. Relative to the first released warmup round, growth over 16 MiB fails sustained-growth validation and growth over 64 MiB fails the hard cap. The thresholds must not be moved to make a run pass.
- RSS remains in the artifact as a capacity/high-water diagnostic. System allocators may retain fully freed pages, so RSS growth alone must not be labeled a live-object leak or override a stable tracked live allocation count.
- The same-page multi-round gate compares browser Worker/WebGL/socket/input baselines and `releaseLifecycleProfile`. That profile includes logical server/coordinator/World state plus `liveWorldInstances` and `worldBackgroundTasks`; it deliberately excludes cumulative allocation counters, peak bytes, and volatile live-byte diagnostics.
- Artifact schema `extraction.e2e.resource-gate.v2` records thresholds, the live-memory assessment, RSS diagnostics, and every round's released snapshots. Live runners execute serially because they share an exclusive local test database and declared ports.

### 4. Validation & Error Matrix

| Condition                                                   | Required result                                                     |
| ----------------------------------------------------------- | ------------------------------------------------------------------- |
| Logical routes are zero but a World/background task is live | Keep polling and fail at the deadline                               |
| Live allocations exceed either unchanged threshold          | Fail the five-round release gate                                    |
| RSS rises while live allocations and lifecycle stay bounded | Preserve the RSS diagnostic; do not report a live-object leak       |
| Allocation/reallocation totals rise between valid rounds    | Ignore for lifecycle equality; retain them in diagnostics           |
| Multi-round browser Worker/WebGL/socket count grows         | Fail even when server lifecycle counters are zero                   |
| Resource endpoint adds or omits an expected field           | Strict decoder failure; update the contract and tests intentionally |

### 5. Tests Required

- Actor tests reject unknown/negative counters, require physical World/task release, reset the three-snapshot stability window after a rebound, and distinguish allocator diagnostics from the cross-round lifecycle profile.
- Five real gameplay rounds must release all server resources and stay within the unchanged live-allocation thresholds. Record RSS without using it as the primary leak verdict.
- Three same-page browser rounds must return Worker roles, live Worker count, WebGL contexts, sockets, input isolation, and the server release profile to baseline.
- Run smoke, ten-browser, multi-round, gameplay, reconnect, crash-recovery, and settlement-uncertainty gates serially. After every runner, verify its owned ports are free and an unrelated user service remains alive.

### 6. Wrong vs Correct

#### Wrong

```ts
expect(round2.resources).toEqual(round1.resources);
assertLeak(round2.rssBytes - round1.rssBytes);
```

This compares cumulative diagnostics as if they were lifecycle state and mistakes allocator-retained pages for reachable objects.

#### Correct

```ts
expect(releaseLifecycleProfile(round2)).toEqual(
  releaseLifecycleProfile(round1),
);
assertStableLiveMemory(rounds.map((round) => round.liveAllocatedBytes));
```

Logical and physical lifecycle state returns to baseline, while the dedicated live-allocation gate measures memory still owned by the process.
