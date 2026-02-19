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

Rules:

1. Optional trait implementation must be discoverable through negotiation capability descriptors.
2. Absence of a capability must produce deterministic `Capability` errors for dependent commands.

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

## Evolution Rules

1. Core backend trait changes are breaking and require major version.
2. Additive behavior should be introduced as capability-gated extension traits and command variants.
3. Backend-specific data fields should live under `extensions` and remain optional.
