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
- RNode/LXMF sessions must not emit `Connected` until the host has negotiated and reported a
  usable notification payload size.

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

7. RNode/LXMF MTU readiness:
- RNode firmware delivers a complete KISS frame in one BLE notification, so adapters must
  negotiate/report at least `170` notification payload bytes (`ATT MTU >= 173`) before a
  session is considered ready.
- Hosts that remain at the Bluetooth default `ATT MTU 23` (`20` payload bytes) must fail
  readiness with validation semantics instead of surfacing a healthy-but-silent session.
- Native `btleplug` backends cannot request a larger MTU in version `0.11.8`; platform hosts
  that can negotiate MTU must do so before enabling notifications.

## Conformance

Reference validation helpers:

- `validate_event_sequence`
- `validate_capabilities`
- `validate_event_payload_bounds`
- `validate_rnode_lxmf_readiness`

These are covered by tests in:

- `crates/libs/lxmf-sdk/tests/mobile_ble_contract.rs`

Mobile integrations should run equivalent conformance vectors in Android/iOS CI and publish
artifacts that include:

- ordered event transcript
- capability snapshot
- pass/fail summary for ordering, timeout, and cancellation behavior
