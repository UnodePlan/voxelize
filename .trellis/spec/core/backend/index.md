# Backend Development Guidelines

> Best practices for backend development in this project.

---

## Overview

This directory contains guidelines for backend development. Fill in each file with your project's specific conventions.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | To fill |
| [Database Guidelines](./database-guidelines.md) | ORM patterns, queries, migrations | To fill |
| [Error Handling](./error-handling.md) | Error types, handling strategies | To fill |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns | To fill |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels | To fill |
| [Public Network Trust Boundary](./network-trust-boundary.md) | Authentication, Join admission, strict policy, and World lifecycle contracts | Active |
| [Authentication Persistence](./auth-persistence.md) | SIWE, nonce/session storage, cookie, RPC, and session-revocation contracts | Active |
| [Match Lifecycle Persistence](./match-lifecycle-persistence.md) | Exact-ten matchmaking, frozen rosters, World generations, deadlines, and reconnect contracts | Active |
| [Deterministic World Generation](./deterministic-world-generation.md) | Stable catalogs, versioned terrain, actual-Chunk fingerprints, and preload readiness | Active |

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
