# SDK Contract v2.5 (Core)

Status: Draft, implementation target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Purpose

This document defines the stable embeddable SDK contract for host applications using LXMF-rs.
It is event-first, backend-neutral, and profile-aware.

This contract is the authoritative source for SDK behavior. Wire-level payload rules remain defined by:

- `docs/contracts/payload-contract.md`
- `docs/contracts/rpc-contract.md`

## Scope

In scope:

- SDK lifecycle and command API
- capability/version negotiation
- profile behavior
- cursored event consumption behavior
- idempotency and terminality rules

Out of scope:

- low-level transport packet formats
- external interop test harness ownership

## Versioning Model

The SDK reports:

- `contract_release` (example: `v2.5`)
- `schema_namespace` (example: `v2`)
- `active_contract_version` (numeric negotiation value)

Rules:

1. Negotiation chooses the highest common `active_contract_version`.
2. If no common version exists, startup fails with `SDK_CAPABILITY_CONTRACT_INCOMPATIBLE`.
3. `schema_namespace` is stable for additive changes only.
4. `schema_namespace` must bump when required-field semantics, cursor encoding, or ordering guarantees change.
5. If profile-required APIs/capabilities are not available after negotiation, startup fails with `SDK_CAPABILITY_CONTRACT_INCOMPATIBLE`.

## Runtime Profiles

Profiles:

1. `desktop-full`
2. `desktop-local-runtime`
3. `embedded-alloc`

Profile declaration is required at startup. Effective capabilities and limits are frozen for the runtime session.

## Capability Negotiation

Startup handshake request includes:

- `supported_contract_versions: Vec<u16>`
- requested capabilities

Startup handshake response includes:

- `runtime_id`
- `active_contract_version`
- `effective_capabilities`
- `effective_limits`
- `contract_release`
- `schema_namespace`

Capability descriptor fields:

- `id`
- `version`
- `state` (`enabled|disabled|experimental|deprecated`)
- `since_contract`
- `deprecated_after_contract` optional

`effective_limits` must expose:

- `max_poll_events`
- `max_event_bytes`
- `max_batch_bytes`
- `max_extension_keys`
- `idempotency_ttl_ms` when `sdk.capability.idempotency_ttl` is enabled

## Public API

All methods are fallible and return typed SDK errors.

Required API:

- `start(config) -> Result<ClientHandle, SdkError>`
- `send(req) -> Result<MessageId, SdkError>`
- `cancel(id) -> Result<CancelResult, SdkError>`
- `status(id) -> Result<Option<DeliverySnapshot>, SdkError>`
- `configure(patch) -> Result<Ack, SdkError>`
- `poll_events(cursor, max) -> Result<EventBatch, SdkError>`
- `snapshot() -> Result<RuntimeSnapshot, SdkError>`
- `shutdown(mode) -> Result<Ack, SdkError>`

Capability-gated API:

- `tick(budget) -> Result<TickResult, SdkError>` (requires `sdk.capability.manual_tick`)

Async extension (feature-gated):

- `subscribe_events(start) -> Result<EventSubscription, SdkError>` (requires `sdk.capability.async_events`)

## Trait Evolution Policy

Rules:

1. Core API trait method sets are frozen for `v2.x`.
2. Additive minor features must use capability-gated command variants and extension traits.
3. Public contract structs/enums should be `#[non_exhaustive]`.

## Lifecycle State Machine

States:

- `New`
- `Starting`
- `Running`
- `Draining`
- `Stopped`
- `Failed`

Rules:

1. Each API call must define legal states.
2. Illegal-state call returns `SDK_RUNTIME_INVALID_STATE`.
3. `shutdown()` is idempotent.
4. `start()` in `Running` returns existing handle.
5. `start(config)` with different active config revision fails with `SDK_RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG`.

Method legality matrix:

| API | Legal states |
| --- | --- |
| `start` | `New`, `Running` |
| `send` | `Running` |
| `cancel` | `Running`, `Draining` |
| `status` | `Running`, `Draining` |
| `configure` | `Running` |
| `tick` | `Running`, `Draining` |
| `poll_events` | `Running`, `Draining` |
| `snapshot` | `Running`, `Draining` |
| `shutdown` | `Starting`, `Running`, `Draining`, `Stopped`, `Failed` |
| `subscribe_events` | `Running`, `Draining` |

`shutdown()` behavior in `Stopped` must return success/no-op.

## Delivery State Semantics

States:

- declared delivery states: `queued`, `dispatching`, `in_flight`, `sent`, `delivered`, `failed`, `cancelled`, `expired`, `rejected`

Rules:

1. Exactly one terminal state per message.
2. Terminal transition is CAS-protected in storage.
3. Post-terminal transitions fail with `SDK_RUNTIME_ALREADY_TERMINAL`.
4. Without `sdk.capability.receipt_terminality`: terminal states are `sent`, `failed`, `cancelled`, `expired`, `rejected`.
5. With `sdk.capability.receipt_terminality`: terminal states are `delivered`, `failed`, `cancelled`, `expired`, `rejected`; `sent` is non-terminal.

## Idempotency and Cancel

`SendRequest` includes optional `idempotency_key`.

Dedupe scope:

- `(source, destination, idempotency_key)`

Rules:

1. TTL is `idempotency_ttl_ms` from negotiated `effective_limits`.
2. TTL clock source is runtime monotonic clock; wall-clock changes must not alter dedupe validity.
3. Same key + same payload hash within TTL returns original `MessageId`.
4. Same key + different payload hash within TTL returns `SDK_VALIDATION_IDEMPOTENCY_CONFLICT`.
5. Reuse after TTL expiry creates a new message identity.
6. Cancel result is one of:
- `Accepted`
- `AlreadyTerminal`
- `NotFound`
- `TooLateToCancel`
7. Cancel/send races resolve by first terminal CAS commit.
8. Conformant `v2.5` profiles must not return `Unsupported` for `cancel`.

## Config and Policy Mutation

Rules:

1. Patch semantics: RFC7396 over typed config.
2. Validate before commit.
3. Apply atomically.
4. Unknown config keys are rejected with `SDK_CONFIG_UNKNOWN_KEY`.
5. Concurrent config updates use revision CAS (`SDK_CONFIG_CONFLICT` on mismatch).

## Config Layering

`SdkCoreConfig` is backend-neutral.  
`RpcBackendConfig` contains RPC-specific controls:

- header/body limits
- read/write timeouts
- auth mode and token verifier config

Non-RPC backends may ignore `RpcBackendConfig`.

## Security Baseline

Auth mode defaults and requirements:

1. `local_only` is default bind mode.
2. `local_trusted` is required baseline auth mode for loopback/local-only operation.
3. Remote bind requires explicit auth mode: `token` or `mtls`.
4. Remote bind without an explicit auth mode fails with `SDK_SECURITY_AUTH_REQUIRED`.
5. Token mode must reject replayed `jti` (`SDK_SECURITY_TOKEN_REPLAYED`).

## Compatibility Rules

1. Additive minor changes only.
2. Renames/removals/required behavior changes require major version.
3. Unknown fields in event payloads must be ignored.
4. Unknown fields in command payloads must be rejected with `SDK_VALIDATION_UNKNOWN_FIELD`, unless explicitly marked as extension fields for that command.
5. Unknown enum variants map to `Unknown`.
6. All major payloads support optional `extensions`.

## Relationship to Other Contracts

- Event envelope and stream behavior: `docs/contracts/sdk-v2-events.md`
- Error taxonomy: `docs/contracts/sdk-v2-errors.md`
- Backend SPI: `docs/contracts/sdk-v2-backends.md`
- Migration and cutover: `docs/contracts/sdk-v2-migration.md`
- Capability/profile matrix: `docs/contracts/sdk-v2-feature-matrix.md`
