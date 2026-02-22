# Mobile BLE Conformance Runbook (Android + iOS)

## Purpose

This runbook defines how Android and iOS host integrations validate conformance to the shared
mobile BLE host contract.

## Contract Source

- Contract doc: `docs/contracts/mobile-ble-host-contract.md`
- Rust contract types: `crates/libs/lxmf-sdk/src/backend/mobile_ble.rs`
- Shared validator: `validate_mobile_ble_event_sequence`

## Required Artifacts Per Platform

1. Event transcript JSON (`events.sample.json` compatible shape).
2. Capability snapshot including:
- `supports_background_restore`
- `supports_write_without_response`
- `supports_operation_cancel`
- queue/payload limits
3. Pass/fail summary for ordering, timeout, and cancellation checks.

Reference fixture directories:

- `docs/fixtures/mobile-ble/android/`
- `docs/fixtures/mobile-ble/ios/`
- `docs/fixtures/mobile-ble/shared/`

## Mandatory Contract Checks

1. `sequence_no` is strictly monotonic.
2. Session lifecycle ordering is valid (`connected` precedes session-bound events).
3. `disconnected` terminates session event eligibility.
4. Timeout/cancel semantics are explicit and deterministic.
5. Queue/backpressure behavior is bounded and declared via capability values.

## CI Commands

```bash
cargo test -p lxmf-sdk --test mobile_ble_contract
cargo test -p test-support --test mobile_ble_android_conformance --test mobile_ble_ios_conformance
```

## Failure Handling

1. Preserve the failing transcript artifact and capability snapshot.
2. Diff transcript against passing baseline fixtures.
3. Resolve event ordering drift before merging runtime changes.
4. Re-run both platform conformance tests before release promotion.

## Release Gate

No release candidate should proceed when either Android or iOS conformance suite fails.
