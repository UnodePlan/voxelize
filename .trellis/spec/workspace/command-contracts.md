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
