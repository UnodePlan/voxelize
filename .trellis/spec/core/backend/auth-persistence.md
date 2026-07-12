# Authentication Persistence

## Scenario: SIWE login and revocable cookie sessions

### 1. Scope / Trigger

- Applies to `apps/extraction-server/src/auth`, `ports/auth_repository.rs`, `persistence`, authentication HTTP routes, and migrations.
- Trigger this spec when changing SIWE fields, Ethereum network support, nonce/session storage, cookies, wallet account resolution, logout, or WebSocket session replacement.
- The engine remains independent of SQLx and SIWE. Application code connects authenticated sessions to the reusable `ConnectionPrincipal` and `CloseAuthenticatedSession` boundaries.

### 2. Signatures

```rust
pub trait AuthRepository: RepositoryProbe + Send + Sync {
    async fn complete_login(
        &self,
        command: LoginCommand,
    ) -> Result<LoginResult, AuthRepositoryError>;

    async fn revoke_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<Uuid>, AuthRepositoryError>;
}

pub struct LoginResult {
    pub session: SessionRecord,
    pub revoked_session_ids: Vec<Uuid>,
}

pub struct CloseAuthenticatedSession {
    pub session_id: String,
}
```

### 3. Contracts

- Ethereum Mainnet chain ID `1` is the only accepted login network. Wallet connection alone is never an authenticated session.
- A nonce is generated with a CSPRNG, expires after five minutes, and is stored only as SHA-256. Consumption is an atomic conditional database update inside the login transaction.
- Anonymous nonce and verification endpoints apply separate bounded per-source limits before database or RPC work. The in-process key is the directly observed peer address; deployments behind a reverse proxy must enforce equivalent per-client limits at the trusted edge and must not accept arbitrary forwarded-address headers.
- Expired nonce rows are pruned opportunistically every bounded issuance interval with a bounded batch and a repository hard cap. Cleanup runs as a best-effort single-flight background task and never participates in login correctness.
- SIWE validation covers domain, URI, effective scheme, nonce, chain ID, issued-at bounds, required expiration, and signature. The application binds the nonce again through the database compare-and-set; verifier options are not the replay boundary.
- EOA signatures are checked locally first. EIP-1271 uses a direct `isValidSignature(bytes32,bytes)` call so HTTP and JSON-RPC failures remain retryable; EIP-6492 uses the upstream universal-validator path. Both use Mainnet chain preflight, immediate bounded concurrency admission, and a short timeout. Missing or failed RPC never bypasses verification.
- Session tokens contain 32 random bytes and are sent only in an HttpOnly cookie. PostgreSQL stores SHA-256, not the bearer token. HTTPS always requires the `__Host-` prefix, `Secure`, `SameSite=Lax`, and `Path=/`; a non-Secure development cookie is allowed only for loopback HTTP origins. The public browser origin must match the SIWE URI scheme and authority, and the public `run(config)` entry point must validate programmatically constructed configuration before database or socket startup.
- A database operation that can wait on a row lock must acquire `FOR UPDATE` first and obtain authoritative database time after the lock is granted. Login takes the nonce, account, and currently active session locks before its final time check, then atomically revalidates both nonce expiry and the server-derived SIWE expiration/max-age deadline before consuming the nonce or replacing a session. Active-session touch follows the same lock-then-time rule. Caller-captured time is only a lower bound, and the authentication service also checks the returned record with a fresh application clock before authorizing an HTTP request.
- Login is single-session per account. The transaction returns every replaced session ID; the HTTP layer closes those sockets before returning the new login response. Explicit logout revokes the token and closes the returned session ID in the same way.
- A closed authenticated socket keeps its routing state until the existing token-aware `Disconnect` path runs. Active sockets must remain discoverable while pending Join, in a World, pending rebind, or waiting outside a World.
- WebSocket registration revalidates the same cookie-derived principal after the `Connect` actor message returns. Each active socket then enforces the principal's exact `valid_until` deadline, performs passive database revalidation at a bounded interval without extending idle lifetime, and refreshes activity before forwarding binary requests when required. An unchanged near deadline may trigger at most one active refresh per interval, so an absolute TTL cannot turn every game packet into a database write. Any revoked or expired session receives a policy close. Logout/replacement closure also tracks sockets during asynchronous Leave until its token-aware callback completes, so revocation cannot be bypassed in either transition window.
- Runtime startup never performs DDL. Migrations are additive and run through the dedicated migration binary or deployment step.
- Authentication-login and matchmaking feature flags may stop new login/queue traffic. They must not enable guest identity, shared-secret fallback, or client-supplied account fields.

### 4. Validation Matrix

| Condition | Required result |
| --- | --- |
| Reused, expired, wrong-domain, or wrong-URI nonce | Reject without creating an account or session |
| Non-Mainnet SIWE message | `AUTH_WRONG_NETWORK` |
| Invalid EOA/contract signature | Reject; never create a cookie |
| Smart-wallet RPC timeout or wrong chain | Fail closed with a retryable service error |
| Valid existing cookie while RPC is unavailable | Continue session/warehouse/match operations without RPC |
| Second login for the same wallet | Reuse account, revoke old session, close old sockets, create one new session |
| Logout with an active socket | Revoke token, send policy close, remove cookie |
| Old cookie after logout/replacement | Reject protected HTTP and future WebSocket handshakes |
| Existing socket after revocation, absolute expiry, or idle expiry | Close with policy status without forwarding another game request |
| Concurrent consumption of one nonce | Exactly one login succeeds |
| Nonce or session expires while waiting for a database row lock | Reject after the lock is granted; never use the pre-wait timestamp to authorize |
| SIWE expiration/max-age passes while waiting for the account lock | Roll back nonce consumption and session replacement; return an invalid-SIWE result |
| SIWE expiration/max-age passes while waiting for the old session lock | Preserve the old session and unconsumed nonce; never create the replacement |
| Programmatic HTTPS config disables Secure or changes public origin | Reject at `run(config)` before database or socket startup |
| High-rate WS traffic near an unchanged absolute deadline | At most one active session refresh per configured interval |
| Anonymous auth burst from one source | Bounded before nonce insert or signature/RPC work; retryable service response after the limit |
| Expired nonce accumulation | Opportunistic bounded pruning; live and unexpired rows remain |
| Feature flag disabled | Stop new login/queue; never fall back to legacy identity |

### 5. Tests Required

- Official EOA vector plus invalid domain, URI, chain, time, expiration, and signature cases.
- Mock Mainnet RPC coverage for EIP-1271, EIP-6492, invalid magic values, and timeouts.
- PostgreSQL concurrency tests for one-time nonce consumption and same-wallet session replacement.
- PostgreSQL tests for timestamp-skewed replacement, nonce/session/SIWE expiry while blocked on nonce, account, or session row locks, monotonic session timestamps, bounded expired-nonce pruning, and anonymous endpoint rate limits.
- HTTP flow tests for nonce, verify, session, warehouse, FIFO queue, logout, cookie attributes, and rollout flags.
- Engine tests for Cookie-to-principal authentication and socket closure on logout or replaced login.
- Root actor tests for lost, pending Join, in-world, pending rebind, repeated close, legacy isolation, and RemoveWorld overlap.

### 6. Forbidden Patterns

- Storing raw nonce or session bearer tokens in PostgreSQL, logs, URLs, or localStorage.
- Treating a connected wallet address, query parameter, or request JSON account ID as authenticated identity.
- Running migrations automatically during application startup.
- Marking a nonce consumed before signature validation without a final atomic compare-and-set.
- Falling back to guest/shared-secret after SIWE or RPC failure.
- Revoking a database session without notifying active WebSocket connections.
- Trusting arbitrary `X-Forwarded-For`/`Forwarded` values as an anonymous rate-limit identity.
- Authorizing with a timestamp captured before a database lock wait, or allowing passive WebSocket revalidation to extend the idle deadline.
- Relying on `from_env()` as the only configuration validation point, or refreshing the same unchanged near-expiry WebSocket deadline once per packet.
