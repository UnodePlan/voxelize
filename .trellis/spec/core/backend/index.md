# Backend Development Guidelines

> Best practices for backend development in this project.

---

## Overview

This directory contains guidelines for backend development. Fill in each file with your project's specific conventions.

---

## Guidelines Index

| Guide                                                                                   | Description                                                                                                                 | Status  |
| --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ------- |
| [Directory Structure](./directory-structure.md)                                         | Module organization and file layout                                                                                         | To fill |
| [Database Guidelines](./database-guidelines.md)                                         | ORM patterns, queries, migrations                                                                                           | To fill |
| [Error Handling](./error-handling.md)                                                   | Error types, handling strategies                                                                                            | To fill |
| [Quality Guidelines](./quality-guidelines.md)                                           | Code standards, forbidden patterns                                                                                          | To fill |
| [Logging Guidelines](./logging-guidelines.md)                                           | Structured logging, log levels                                                                                              | To fill |
| [Public Network Trust Boundary](./network-trust-boundary.md)                            | Authentication, Join admission, strict policy, and World lifecycle contracts                                                | Active  |
| [Authentication Persistence](./auth-persistence.md)                                     | SIWE, nonce/session storage, cookie, RPC, and session-revocation contracts                                                  | Active  |
| [Match Lifecycle Persistence](./match-lifecycle-persistence.md)                         | Capacity-N matchmaking (prod N=10; DEV env 2..=10), frozen rosters, World generations, deadlines, reconnect                 | Active  |
| [Deterministic World Generation](./deterministic-world-generation.md)                   | Stable catalogs, versioned terrain, actual-Chunk fingerprints, and preload readiness                                        | Active  |
| [Authoritative Match Inventory and Loot](./authoritative-match-assets.md)               | Private inventory, pending ownership, deterministic drops, auto-pickup, and Direct state                                    | Active  |
| [Authoritative Match Mining](./authoritative-mining.md)                                 | Strict mining intents, server timing/raycast, atomic harvest, AIR ordering, and full progress snapshots                     | Active  |
| [Authoritative Match Combat](./authoritative-combat.md)                                 | Half-heart health, server-selected melee targets, terminal inventory drops, timeout arbitration, and death persistence      | Active  |
| [Authoritative Extraction and Settlement](./authoritative-extraction-settlement.md)     | Extraction qualification, hard-deadline sealing, atomic settlement, reconciliation, results, and warehouse projections      | Active  |
| [Authoritative Movement and Bounded Visibility](./authoritative-movement-visibility.md) | Server-owned swept movement, body/eye coordinates, bounded chunk loading, peer/entity projection, and client World disposal | Active  |
| [Internal Read-Only Operations](./internal-read-only-operations.md)                     | Isolated loopback listener, fixed projections, credential boundary, role probe, and redacted audit contracts                | Active  |

---

## How to Fill These Guidelines

For each guideline file:

1. Document your project's **actual conventions** (not ideals)
2. Include **code examples** from your codebase
3. List **forbidden patterns** and why
4. Add **common mistakes** your team has made

The goal is to help AI assistants and new team members understand how YOUR project works.

---

**Language**: All documentation should be written in **English**.
