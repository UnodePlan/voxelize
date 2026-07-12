# Public Network Trust Boundary

## Scenario: Authenticated WebSocket and authoritative World admission

### 1. Scope / Trigger

- Applies to `server/http.rs`, `server/http/auth.rs`, `server/server/*`, and `server/world/*` whenever a public client can open a connection, join a World, send a Method/Event, or reconnect.
- Trigger this spec for changes to authentication, Origin/CORS, connection queues, Join capacity, strict request policy, detach/rebind, or World lifecycle.
- Legacy Demo behavior remains the default. Public gameplay must opt into `HttpConfig::authenticated(...)` and `WorldRequestPolicy::strict()` explicitly.

### 2. Signatures

```rust
pub trait ConnectionAuthenticator: Send + Sync {
    fn authenticate(&self, request: ConnectionAuthRequest) -> ConnectionAuthFuture;
}

pub fn HttpConfig::authenticated<A>(authenticator: A) -> Self
where
    A: ConnectionAuthenticator + 'static;

pub struct ClientJoinRequest {
    pub id: String,
    pub sender: WsSender,
    pub principal: Option<ConnectionPrincipal>,
    pub join_attempt_id: String,
    // username and preferences are also carried, but public mode does not trust username.
}

pub enum WorldRequestPolicy {
    Legacy,
    Strict { allowed_methods: Vec<String>, allowed_events: Vec<String> },
}

pub struct ClientRebindRequest {
    pub id: String,
    pub sender: WsSender,
    pub principal: ConnectionPrincipal,
}
```

### 3. Contracts

- `ConnectionPrincipal { account_id, session_id }` is authenticated server state. Public query `client_id` is never an identity source; the server generates the per-match public client ID.
- Public mode requires an authenticator, exact allow-listed Origin, bounded HTTP/WS payloads, bounded outbound and World request queues, and finite authentication/message timeouts. Authentication or overload failures close/reject the connection; they do not become guest access.
- Join admission is a two-phase operation: the World actor validates lifecycle, capacity, duplicate client, and duplicate principal, then returns `ClientJoinReceipt`; only a successful receipt lets `Server` commit its connection table.
- `WorldRequestPolicy::Strict` denies raw voxel updates, client movement flags, commands, and unlisted Methods/Events. Names are normalized to lowercase before comparison.
- A detached client may be rebound only with the owning principal and a valid current World generation. Leave/despawn, cancel-Join, detach, and rebind are distinct operations and must preserve actor mailbox ordering.
- World lifecycle is `Created -> Preparing -> Ready -> Stopping -> Stopped`; only `Ready` accepts clients. Prepare/preload/stop are idempotent, and removal clears routes, pending work, and timing state before the old World is stopped.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Public mode has no authenticator or auth times out | Reject/close; never create a guest principal |
| Missing, duplicate, malformed, or non-allow-listed Origin | Reject before application routes |
| Query `client_id` differs from server identity | Ignore it in public mode |
| World is not `Ready` | Return `WorldNotReady`; do not add a client |
| Concurrent Join would exceed capacity | Return `WorldFull`; Server connection table stays unchanged |
| Principal already occupies the World | Return `DuplicatePrincipal` |
| Strict raw UPDATE, movement flag, command, or unlisted Method/Event | Reject before custom handler |
| Malformed protobuf/JSON or invalid enum | Return a protocol error or close with a bounded error; never panic |
| Full outbound or World request queue | Reject/close as overloaded; never silently drop authoritative state |
| Rebind with wrong principal or stale generation | Return `PrincipalMismatch`/`NotFound`; retain the existing owner |
| Remove an unknown World | Idempotent success with `removed: false` |

### 5. Good/Base/Bad Cases

- Good: an authenticated connection with an allowed Origin joins a `Ready` World, receives a server-issued public ID, and reconnects through a matching detach lease.
- Base: legacy mode uses the existing query ID and permissive behavior; strict mode with an empty allow-list accepts no client Method/Event.
- Bad: trusting a query ID as the account, forwarding a Join before the World receipt, falling back to guest after auth failure, or using `do_send` for cleanup where ordering matters.

### 6. Tests Required

- HTTP tests assert exact Origin behavior, public-auth failure, disabled legacy RTC, payload limits, and route isolation.
- Server/World actor tests assert 10 accepted / 11th rejected under concurrent Join, cancelled Join retry, strict rejection, dynamic AddWorld, idempotent RemoveWorld, and old-World address closure.
- WebSocket tests assert empty legacy ID compatibility, malformed input closure, duplicate Origin rejection, authentication timeout, and bounded output overload.
- Lifecycle tests assert detach/rebind ownership, stale lease cleanup, explicit Leave cleanup before rejoin, and no duplicate despawn.
- Run `cargo test --lib --tests`, extraction-server tests, extraction-client tests, extraction E2E actor tests, and TypeScript unit tests for every boundary change.

### 7. Wrong vs Correct

#### Wrong

```rust
let client_id = query.client_id.clone();
server.connections.insert(connection_id, world);
world.do_send(ClientJoinRequest { client_id });
```

#### Correct

```rust
let principal = authenticator.authenticate(request).await?;
let receipt = world
    .send(ClientJoinRequest { principal: Some(principal), join_attempt_id, ..request })
    .await??;
server.commit_join(connection_id, receipt);
```

The World receipt is the commit boundary. Any failed, cancelled, stale, or over-capacity attempt must leave the Server connection table and public inventory unchanged.
