# Internal Read-Only Operations

## Process Boundary

- Operations run in the dedicated `ops` binary and are disabled unless `EXTRACTION_OPS_ENABLED=true`.
- The listener must bind to a loopback address. It must never be mounted into the public game HTTP server.
- The operations database URL must use credentials dedicated to this process. Reusing the game runtime or migration role is forbidden.
- Enabling or disabling operations must not affect authentication, matchmaking, active worlds, or settlement processing.

## Fixed Query Surface

The HTTP surface contains only these projections:

- account by ID;
- match by ID;
- paginated participants by match ID;
- settlement and its item rows by settlement ID;
- warehouse balances by account ID;
- paginated ledger entries by account ID.

Arbitrary SQL, mutation routes, balance adjustment, settlement replay, and player-session authentication are forbidden. Unknown write methods must return the same read-only rejection without reaching the repository.

## Database Role Contract

Startup must fail closed unless the connected role satisfies all of these conditions:

- `current_user` equals `session_user`;
- every fixed target table is readable;
- the login role and every directly or indirectly reachable member role have no table-level `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `TRIGGER`, or `REFERENCES` privilege on a target;
- those roles have no column-level `INSERT`, `UPDATE`, or `REFERENCES` privilege on a target;
- none of those roles is superuser, has `CREATEDB`, `CREATEROLE`, `REPLICATION`, or `BYPASSRLS`, or is a PostgreSQL predefined `pg_*` role with server-wide capabilities.

`default_transaction_read_only` and explicit read-only transactions are defense in depth, not substitutes for least privilege. Every query uses a repeatable-read, read-only snapshot with bounded statement and lock timeouts.

## HTTP and Audit Contract

- Require exactly one `Authorization: Bearer ...` value. Hash the configured token and compare hashes in constant time.
- Enforce loopback deployment, bounded payloads, per-peer rate limiting, query timeout, and pagination of at most 100 records.
- Successful and error responses must be non-cacheable and must not expose internal repository errors.
- Emit one structured audit event for every request outcome, including rejected routes and methods.
- Audit events may contain operation kind, outcome, method, status, result count, request ID, timestamp, and duration only. Never log account/match IDs, wallet addresses, credentials, query strings, SQL, signatures, sessions, or secrets.

## Verification

- HTTP integration tests must prove fixed reads, redacted models, anonymous/player/write rejection, pagination, timeout, rate limiting, and one audit event per request.
- SQL contract tests must retain inherited-role and column-level privilege checks.
- A controlled deployment test must prove that a real SELECT-only role starts successfully and roles with direct, inherited, or column-level writes fail startup. Do not claim this gate passed when no disposable PostgreSQL environment was used.
