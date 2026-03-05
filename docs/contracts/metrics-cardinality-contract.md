# Metrics Cardinality Contract

Status: draft
Owners: runtime observability maintainers

## Scope

Locks canonical metric names, label sets, and cardinality limits for BLE camera transfer flows.

## Required Metrics

- `ble_connect_failures_total{iface}`
- `ble_chunk_retries_total{iface,reason}`
- `ble_nacks_total{iface}`
- `ble_tx_queue_timeout_total{iface}`
- `attachment_upload_offset_reject_total{code}`
- `attachment_upload_checksum_mismatch_total{}`
- `capture_success_total{camera_id}`
- `capture_failure_total{camera_id,reason}`

## Label Enum Rules

`reason` enum:

- `ack_timeout`
- `nack_gap`
- `disconnect`
- `queue_timeout`
- `peer_error`
- `checksum_mismatch`
- `unsupported_mtu`

`code` enum:

- `SDK_RUNTIME_INVALID_CURSOR`
- `SDK_RUNTIME_NOT_FOUND`
- `SDK_VALIDATION_INVALID_ARGUMENT`
- `SDK_VALIDATION_IDEMPOTENCY_CONFLICT`

Unknown labels:

- Unknown `reason` or `code` must be normalized to `other`.
- Companion counters (`*_other_total`) must capture normalized unknowns.

## Cardinality Caps

- `iface <= 8`
- `camera_id <= 16`
- `reason` and `code` are bounded enums only.
