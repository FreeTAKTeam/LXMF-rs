# SDK Contract v2.5 (Backend SPI)

Status: Draft, implementation target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Design Intent

The SDK facade remains backend-neutral. Backends are adapters behind object-safe traits.

Core guarantees:

- command behavior stability
- event semantics consistency
- no storage model leakage in public API

Core RPC request/response contracts live under `docs/schemas/sdk/v2/rpc/` and are CI-validated fixtures.

## Core Backend Trait

The core backend contract must support:

- `negotiate`
- `execute`
- `poll_events`
- `snapshot`
- `shutdown`

Object safety is required for backend registry and test harnesses.

## Capability-Gated Optional Traits

Optional backend capabilities are extension traits keyed by capability ID.

Examples:

- `sdk.capability.async_events`
- `sdk.capability.cursor_replay`
- `sdk.capability.token_auth`
- `sdk.capability.mtls_auth`
- `sdk.capability.event_sink_bridge`

Rules:

1. Optional trait implementation must be discoverable through negotiation capability descriptors.
2. Absence of a capability must produce deterministic `Capability` errors for dependent commands.

## Extension and Plugin Model

Optional domain modules are negotiated as plugins on top of capability negotiation.

Plugin negotiation surface:

- SDK types: `PluginDescriptor`, `PluginState`
- negotiation helper: `negotiate_plugins(requested, available, effective_capabilities)`

Policy:

1. Plugin activation is additive and must not alter core command semantics.
2. A plugin may activate only when all `required_capabilities` are present in
   `effective_capabilities`.
3. Unknown plugin IDs are ignored (forward-compatible behavior).
4. Duplicate plugin requests must collapse to a single activation decision.
5. Plugin-specific wire fields must remain under explicit extension namespaces.

Current lifecycle:

- `stable`: release-gated and backward-compatibility managed.
- `experimental`: available but not release-blocking.
- `deprecated`: supported for migration window only.

Conformance gate:

- `cargo run -p xtask -- plugin-negotiation-check`

## Storage Backend Invariants

Required behavior:

1. Atomic append/upsert by message identity.
2. Atomic event append/upsert by `event_id`.
3. CAS-protected terminal transition semantics.
4. Point-in-time snapshot reads.
5. Idempotent write behavior on replay/retry.

## Transport/Runtime Backend Invariants

Required behavior:

1. Exactly one terminal send outcome per send handle.
2. Delivery transition order must obey core state machine.
3. Cancellation race handling must preserve single-terminal guarantee.

## Attachment Streaming Backend Invariants

When `sdk.capability.attachment_streaming` is enabled:

1. Upload sessions must expose deterministic `next_offset` progression.
2. Commit must validate both declared byte length and declared SHA-256 checksum.
3. Download chunk must support caller-provided offsets for resumable transfer.
4. Out-of-range upload/download offsets must return `SDK_RUNTIME_INVALID_CURSOR`.

## Event Sink Adapter Contract

When `sdk.capability.event_sink_bridge` is enabled, `rns-rpc` supports optional sink adapters
behind `EventSinkBridge`.

Current adapter surface contracts:

- webhook adapter: `WebhookEventSinkBridge` + `WebhookEventPublisher`
- mqtt adapter: `MqttEventSinkBridge` + `MqttEventPublisher`

Required semantics:

1. Sink dispatch happens only for `event_sink.enabled=true`.
2. Sink dispatch always uses already-redacted event payloads.
3. `event_sink.allow_kinds` acts as a deterministic allowlist (`webhook|mqtt|custom`).
4. Oversized sink envelopes (`event_sink.max_event_bytes`) are skipped, not partially delivered.
5. Sink publish failures must not block local runtime event progress.
6. Sink publish success/error/skip counters must be exported via runtime metrics.

## Embedded Link Adapter Contract

Constrained link adapters (serial, BLE, LoRa) are modeled via:

- `rns_transport::embedded_link::EmbeddedLinkAdapter`
- `rns_transport::embedded_link::EmbeddedLinkCapabilities`
- `rns_transport::embedded_link::EmbeddedLinkConfig`

Required embedded-link semantics:

1. Adapter ID must be stable and unique in a runtime process.
2. `send_frame` must reject frames over adapter MTU with deterministic `FrameTooLarge`.
3. `poll_frame` must be non-blocking and return `Ok(None)` when idle.
4. Capability flags must truthfully describe ordering/ack/fragmentation behavior.
5. Adapter implementations must preserve raw frame bytes without implicit transcoding.

Mock conformance gate:

- `cargo run -p xtask -- embedded-link-check`

## Key Management Backend Contract

When `sdk.capability.key_management` is enabled, the backend must provide deterministic key
storage semantics through `SdkBackendKeyManagement`.

Key management scope:

- backend class discovery (`in_memory`, `file`, `os_keystore`, `hsm`, `custom`)
- key CRUD by `key_id`
- stable key listing for operational audits
- secure fallback wiring (`FallbackKeyManager<Primary, Secondary>`)

Required semantics:

1. `key_id` is canonical and path-safe (`[A-Za-z0-9._-]+`).
2. `key_get` returns exact key bytes previously stored by `key_put` without mutation.
3. `key_delete` is idempotent.
4. `key_list_ids` returns stable sorted identifiers.
5. Fallback behavior uses secondary backend when primary read/write/list paths fail.
6. Primary and secondary backend failures must return explicit backend errors (no silent success).

Backend hook interfaces:

- OS keystore hook adapter: `rns_core::key_manager::OsKeyStoreHook`
- HSM hook adapter: `rns_core::key_manager::HsmKeyStoreHook`

Conformance gate:

- `cargo run -p xtask -- key-management-check`

## Config Layering

`SdkCoreConfig`:

- shared behavior controls independent of backend type

`RpcBackendConfig`:

- listener and framing limits
- HTTP timeout settings
- auth mode and token verifier settings

Non-RPC backends may ignore RPC-specific config without violating contract.

## Security Controls at Backend Boundary

Minimum backend responsibilities:

1. Enforce authn/authz on all command types (read and mutating).
2. Support `local_trusted` mode for loopback/local-only operation.
3. Reject remote bind when auth mode is not explicitly `token` or `mtls`.
4. Enforce replay rejection for token mode.
5. Enforce per-principal and per-IP rate limits.
6. Apply redaction policy before event/log emission.
7. Enforce `mtls` authorization from TLS transport metadata (peer certificate/SAN), not user-controlled headers.

## Evolution Rules

1. Core backend trait changes are breaking and require major version.
2. Additive behavior should be introduced as capability-gated extension traits and command variants.
3. Backend-specific data fields should live under `extensions` and remain optional.
