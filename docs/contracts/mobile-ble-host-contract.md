# Mobile BLE Host Contract

Status: draft
Owners: `lxmf-sdk` maintainers

## Scope

This contract defines the Rust-side integration boundary for mobile BLE host adapters
(Android and iOS) consumed by `lxmf-sdk`.

## Required Types

The host adapter surface is implemented by `MobileBleHostAdapter` in:

- `crates/libs/lxmf-sdk/src/backend/mobile_ble.rs`

Required request/response/event types:

- `MobileBleConnectRequest`
- `MobileBleWriteRequest`
- `MobileBleReadRequest`
- `MobileBleSessionDescriptor`
- `MobileBleWriteAck`
- `MobileBleReadResult`
- `MobileBleEvent`
- `MobileBleEventKind`

## Contract Rules

1. Sequence ordering:
- `MobileBleEvent.sequence_no` must be strictly increasing.
- Duplicate or out-of-order sequence numbers are contract violations.

2. Session ordering:
- `Connected` must precede `Notification`, `WriteComplete`, and `Disconnected` for a session.
- `Disconnected` closes a session and no further session-bound events are valid.

3. Cancellation:
- `cancel_operation` is optional.
- If unsupported, adapter must return capability-disabled semantics.

4. Backpressure:
- Adapter must expose queue limits via `MobileBleCapabilities.max_notification_queue`.
- Producers must fail fast or apply bounded buffering; unbounded growth is forbidden.

5. Timeout semantics:
- `connect_timeout_ms`, `write_timeout_ms`, and `read_timeout_ms` are caller-specified caps.
- Exceeding timeout must resolve as `Timeout` event or error return.

6. Error mapping:
- Adapter errors must map to stable `SdkError` categories/machine codes.
- Validation errors (ordering/shape violations) use `SDK_VALIDATION_INVALID_ARGUMENT`.

## Conformance

Reference validation helpers:

- `validate_event_sequence`
- `validate_capabilities`
- `validate_event_payload_bounds`

These are covered by tests in:

- `crates/libs/lxmf-sdk/tests/mobile_ble_contract.rs`

Mobile integrations should run equivalent conformance vectors in Android/iOS CI and publish
artifacts that include:

- ordered event transcript
- capability snapshot
- pass/fail summary for ordering, timeout, and cancellation behavior
