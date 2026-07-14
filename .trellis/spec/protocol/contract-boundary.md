# Extraction Contract Boundary

## Scenario: Versioned Rust/TypeScript Wire Contract

### 1. Scope / Trigger

- Applies to `contracts/extraction/v1`, the extraction server DTOs, `/health/ready`, `/api/bootstrap`, and every client or actor that consumes these payloads.
- Trigger this contract whenever a manifest field, envelope field, stable key, numeric ID, error code, or decoder behavior changes.
- Raw network data remains `unknown` until the canonical boundary decoder accepts it.

### 2. Signatures

```typescript
decodeExtractionManifest(value: unknown): ExtractionManifest
decodeProtocolEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): ProtocolEnvelope
fetchExtractionManifest(
  request?: typeof fetch,
  timeoutMs?: number,
): Promise<ExtractionManifest>
```

```rust
pub fn bundled_manifest() -> Result<ExtractionManifest, ContractError>;
pub fn decode_protocol_envelope(
    value: serde_json::Value,
    manifest: &ExtractionManifest,
) -> Result<ProtocolEnvelope, ContractError>;
```

```text
GET /health/ready -> 200 ready | 503 unavailable
GET /api/bootstrap -> ExtractionManifest
```

### 3. Contracts

- `protocolVersion` is exactly `1`; `catalogVersion` is a positive `u32`.
- Version names are non-empty strings.
- Resources are exactly `dirt`, `gold`, and `diamond`; equipment is exactly `basic_pickaxe` and `basic_melee_weapon`.
- Voxel IDs are unique in `1..=65535`; item IDs are positive and globally unique; every resource stack limit is `64`; score weights are positive.
- `errorCodes` is the complete, unique `ERROR_CODES` taxonomy. Unknown or missing entries are invalid.
- Envelopes reject unknown fields, require UUID request IDs, and require `outcome.data` even when its value is `null`.
- The client must receive a successful readiness response before bootstrap and uses one five-second abort budget for both requests.
- Server readiness probes fail closed after two seconds by default.
- `EXTRACTION_SERVER_BIND` defaults to `127.0.0.1:4100`; `VITE_EXTRACTION_API_URL` is optional and must be an absolute URL when present.

### 4. Validation & Error Matrix

| Condition                                             | Required result                   |
| ----------------------------------------------------- | --------------------------------- |
| Unknown object field                                  | Reject at the decoder boundary    |
| Unsupported protocol version                          | Reject before domain handling     |
| Zero catalog version                                  | Reject                            |
| Missing, duplicate, or unknown stable key/error code  | Reject                            |
| Duplicate/out-of-range ID or stack size other than 64 | Reject                            |
| Missing `outcome.data`                                | Reject; explicit `null` is valid  |
| Repository error or probe timeout                     | `/health/ready` returns 503       |
| Readiness non-2xx                                     | Client does not request bootstrap |
| Client request timeout                                | Abort and expose the retry state  |

### 5. Good/Base/Bad Cases

- Good: canonical manifest plus a version-1 envelope using a known error code.
- Base: successful outcome with `data: null`; this is present and valid.
- Bad: `catalogVersion: 0`, an empty error taxonomy, `CLIENT_INVENTED_ERROR`, a missing success `data` field, or any client-invented top-level field.

### 6. Tests Required

- Rust and TypeScript must evaluate every case in `fixtures/envelopes.json` to the same accept/reject result.
- Both manifest validators must reject zero catalog versions and incomplete or unknown error taxonomies.
- Rust HTTP tests assert ready, unavailable, and pending-probe timeout behavior.
- Client API tests assert readiness-before-bootstrap ordering, no bootstrap after readiness failure, timeout abort, and manifest decoding.
- Production UI tests must consume decoded values only; no component may cast raw JSON.

### 7. Wrong vs Correct

#### Wrong

```typescript
const manifest = (await response.json()) as ExtractionManifest;
versions.innerHTML = manifest.configVersion;
```

#### Correct

```typescript
const manifest = decodeExtractionManifest(await response.json());
value.textContent = manifest.configVersion;
```

Rust callers use `decode_protocol_envelope`; they do not deserialize the wire enum directly and forget semantic validation.
