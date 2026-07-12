# Rust Toolchain Contracts

## Scenario: Extraction Server Engine Linkage

### 1. Scope / Trigger

- Applies when changing the root Rust dependency graph, `rust-toolchain.toml`, a Cargo manifest, the extraction aggregate scripts, or extraction CI.
- The extraction server's default feature set is intentionally lightweight. A successful default build does not prove that the root `voxelize` engine can link.
- `kiddo 4.2.1` resolves `fixed 1.31.0`, whose declared MSRV is Rust 1.93. Toolchain declarations must stay aligned with that resolved graph.

### 2. Signatures

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.93.0"
components = ["clippy", "rustfmt"]
profile = "minimal"

# Cargo.toml and apps/extraction-server/Cargo.toml
rust-version = "1.93"
```

```bash
cargo check --manifest-path apps/extraction-server/Cargo.toml \
  --all-targets --features engine --locked
cargo test --manifest-path apps/extraction-server/Cargo.toml \
  --all-targets --features engine --locked
```

CI installs `dtolnay/rust-toolchain@1.93.0` and exact `protoc` 25.3 through the commit-pinned `arduino/setup-protoc` action. Its path filters include `.cargo/**`, root Cargo manifest and lock, `build.rs`, `rust-toolchain.toml`, `crates/**`, and `server/**` because each can break the engine-linked application.

### 3. Contracts

- `rust-toolchain.toml` is the canonical local toolchain declaration and includes the formatter and linter required by CI.
- The root package and extraction server both declare `rust-version = "1.93"`; a published or path consumer receives an explicit MSRV instead of relying on developer-local state.
- `engine = ["dep:voxelize"]` stays opt-in for fast skeleton linting, but every extraction aggregate build/test and CI test enables it.
- Root `build.rs` invokes system `protoc`; clean CI must install exact version 25.3 before any engine compile instead of relying on runner image contents.
- Application Clippy runs on the default skeleton with `-D warnings`. The engine-linked test may report existing root warnings, but it must compile and pass without suppressing them.
- Toolchain correction must not rewrite Cargo locks or upgrade dependency versions unless that dependency change was separately reviewed.
- This contract introduces no environment variables and makes no network or database call at runtime.

### 4. Validation & Error Matrix

| Condition                                                      | Required result                                                              |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Active `rustc` is older than 1.93                              | Stop before implementation; install the pinned toolchain                     |
| `protoc` is absent on a clean CI runner                        | Stage 0 gate fails before compiling generated Rust protocol code             |
| `fixed` rejects the compiler MSRV                              | Treat as a toolchain/manifest contract failure, not an engine source failure |
| Default server build passes but engine build fails             | Stage 0 gate fails                                                           |
| CI action version differs from `rust-toolchain.toml`           | Reject the change                                                            |
| Root or app `rust-version` differs from the pinned major/minor | Reject the change                                                            |
| Engine check changes either Cargo lock unexpectedly            | Review and restore unrelated lock drift                                      |
| Engine test emits existing root warnings but exits zero        | Record baseline; do not hide warnings in the application                     |

### 5. Good/Base/Bad Cases

- Good: Rust 1.93.0 and protoc 25.3 are selected explicitly, locked engine checks pass, and CI runs after any engine build input changes.
- Base: default-feature Clippy remains fast and warning-free while the separate engine test compiles the full dependency graph.
- Bad: only test the default feature set, or use `stable` locally while CI silently installs a different compiler.

### 6. Tests Required

- Assert `rustc --version --verbose` reports 1.93.0 in the repository.
- Run the locked engine `cargo check` and `cargo test`; assert exit zero and all extraction server tests pass.
- Run root `cargo check --all-targets` and `cargo test --lib --tests`; assert the resolved engine graph remains buildable.
- Run application `cargo fmt --check` and default-feature Clippy with `-D warnings`.
- Inspect `git diff -- Cargo.lock apps/extraction-server/Cargo.lock`; assert no unreviewed dependency drift.
- Inspect CI and aggregate scripts; assert both enable `engine` and CI watches all engine source paths.
- On a clean CI runner, assert the setup action reports protoc 25.3 before `build.rs` runs.

### 7. Wrong vs Correct

#### Wrong

```yaml
- uses: dtolnay/rust-toolchain@stable
- run: cargo test --manifest-path apps/extraction-server/Cargo.toml
```

This can pass the lightweight skeleton while never compiling the root engine, and `stable` can differ across runs.

#### Correct

```yaml
- uses: dtolnay/rust-toolchain@1.93.0
- uses: arduino/setup-protoc@c65c819552d16ad3c9b72d9dfd5ba5237b9c906b # v3.0.0
  with:
    version: "25.3"
- run: cargo test --manifest-path apps/extraction-server/Cargo.toml --all-targets --features engine --locked
```
