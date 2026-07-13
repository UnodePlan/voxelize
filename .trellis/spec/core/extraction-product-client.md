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

## Forbidden Patterns

- Treating a connected wallet as an authenticated account.
- Sending JOIN during server-side reconnect rebind.
- Replacing a terminal result with pending or null state.
- Rendering guessed gameplay or warehouse values before authoritative data arrives.
- Calling wallet transaction methods from the game lifecycle.
- Shipping test bridge identifiers in a production bundle.
