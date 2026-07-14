# Extraction Product Client Contract

## Scope

This document records the implemented contracts for the extraction product client. It covers wallet identity, authenticated HTTP and WebSocket lifecycle, authoritative UI state, result recovery, and the production/test boundary.

## Identity and Session

- Reown AppKit uses the Ethers adapter, Ethereum Mainnet, and SIWE only.
- Wallet connection is not authentication. Warehouse, result, queue, and game requests require a server session bound to the exact current address and chain.
- Signing may take up to the server session window. Closing the wallet modal while still disconnected cancels the attempt; an open modal must not be failed by a short client timeout.
- Address changes, chain changes, and explicit logout invalidate the local auth generation before awaiting network cleanup. They call server logout, close the game socket, and clear privileged state.
- The product must never request a transaction, send assets, swap, onramp, or mutate on-chain state.

## Match Lifecycle

- Queue, result, and auth requests each use a generation guard so late responses cannot restore stale state.
- An initial Preparing assignment sends JOIN exactly once. Active and ExtractionOpen recovery relies on authenticated server rebind and must not send a duplicate JOIN.
- WebSocket reconnect is bounded to 60 seconds. A successful rebind requests a full gameplay snapshot before returning online.
- Policy close and authenticated HTTP `AUTH_REQUIRED` both invalidate the current session.
- A terminal result is monotonic. A late result from match A cannot leave or replace active match B.

## Authoritative Gameplay State

- One strict decoder and reducer own health, inventory, mining, extraction, and attack cursor state.
- The shared Core message decoder is side-effect free. Environment detection, decompression loading, Worker lifecycle, and fallback selection stay at the caller boundary so browser workers, main-thread fallback, and Node tests decode the same bytes without import-time global mutation.
- Revisions never move backward. Missing, unknown, malformed, unsafe-integer, or cross-match fields fail closed.
- Attack, mining, and drop intents share one global gameplay sequence. After refresh, the next sequence follows the maximum server-confirmed cursor, including `inventory.lastDropSequence`.
- UI must show a synchronization state until a complete authoritative snapshot exists. It must not synthesize full health, empty inventory, warehouse zeroes, or committed settlement amounts.
- Extracted assets become permanent only after the result endpoint reports committed extraction. Pending reconciliation is displayed as pending.

## Rendering and Accessibility

- The first screen is the product, not a marketing page.
- HUD anchors and dimensions remain stable across desktop, mobile portrait, and short landscape viewports.
- Ten Minecraft-style hearts represent 20 half-heart health; twelve inventory slots stack quantities by resource.
- Keyboard focus survives unrelated state refreshes. Focus indicators, text alternatives, and reduced-motion behavior are required.

## Production Boundary

- The E2E state bridge is loaded only in Vite `e2e` mode and a production artifact scan rejects its markers.
- Production creates the actual Voxelize World from decoded `INIT`, routes World/peer/entity messages, forwards bounded `LOAD`/`UNLOAD`, wires pointer-lock movement/mining/combat/drop controls, and reconciles server-owned movement. The decorative scene remains only before a match World exists.
- Every leave, policy close, reconnect expiry, logout, wallet/chain change, and replacement INIT disposes the prior World, decoder state, workers, timers, listeners, controls, and owned Three resources exactly once.

## Scenario: Development-only local single-player runtime

### 1. Scope / Trigger

- This contract applies only when Vite reports `import.meta.env.DEV` and the URL query contains `mode=single`.
- The local runtime exists to exercise mining and extraction without wallet, HTTP, WebSocket, or server authority. Its result is page-local and must never be presented as permanent settlement.

### 2. Signatures

- Entry point: `startSinglePlayerClient(root: HTMLElement): void`.
- World boot: `LocalWorldAdapter.initializeWorld(world: LocalWorldPort): Promise<void>` sends local `INIT` and `LOAD` messages through public World APIs.
- Local voxel mutation: `applyServerVoxelUpdate(world, voxel, type): void` uses `source: "server"` so the update enters remesh/light processing without becoming an outbound client intent.

### 3. Contracts

- Mode precedence remains `e2e` → `live-e2e` → `DEV single` → production controller.
- The local map is deterministic, bounded to 16 chunks, and all initial chunk data must be present before gameplay becomes ready.
- Production artifacts must not contain the runtime markers `single-player-mode` or `single-extraction-beacon`; `scripts/assert-production-boundary.mjs` enforces this after every production build.
- The local runtime must not create an E2E bridge or call wallet, authentication, queue, `/api`, `/health`, or `/ws` paths.

### 4. Validation & Error Matrix

- `mode=single` in a production build → start the normal production controller; never import the local runtime.
- Missing or malformed local initialization → render a retryable local error and dispose the partial runtime.
- A chunk has no mesh because its subchunks are empty → it can still be ready when `getChunkByCoords(cx, cz)` returns chunk data.
- Any initial chunk is absent → remain in loading state.
- A production bundle contains a local runtime marker → fail the build boundary scan.

### 5. Good / Base / Bad Cases

- Good: all 16 chunk objects exist, while empty subchunks produce no meshes; the runtime becomes playable.
- Base: the query is absent; the production controller starts unchanged.
- Bad: readiness depends on `chunk.meshes.size`; empty or worker-merged subchunks can keep the page loading forever.

### 6. Tests Required

- Unit-test deterministic map/chunk counts, supported resource reachability, and spawn/extraction support.
- Unit-test that readiness succeeds from chunk presence even when mesh maps are empty, and fails if one chunk is missing.
- Run client Vitest, typecheck, production build, and the artifact boundary scan.
- Browser-check the DEV URL for a non-empty World and absence of production network requests.

### 7. Wrong vs Correct

```typescript
// Wrong: mesh count is not a reliable chunk-data readiness signal.
return map.chunks.every((chunk) =>
  world.getChunkByCoords(chunk.x, chunk.z)?.meshes.size === subChunks,
);

// Correct: public chunk presence is sufficient for collision and raycasting.
return map.chunks.every(
  (chunk) => world.getChunkByCoords(chunk.x, chunk.z) !== undefined,
);
```

## Forbidden Patterns

- Treating a connected wallet as an authenticated account.
- Sending JOIN during server-side reconnect rebind.
- Replacing a terminal result with pending or null state.
- Rendering guessed gameplay or warehouse values before authoritative data arrives.
- Calling wallet transaction methods from the game lifecycle.
- Shipping test bridge identifiers in a production bundle.
- Loading the local single-player runtime without both the Vite DEV gate and the explicit query mode.
- Treating render-mesh count as proof that local chunk data is or is not ready.
