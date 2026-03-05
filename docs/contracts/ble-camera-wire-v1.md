# BLE Camera Wire Protocol v1

Status: draft
Owners: `reticulumd` + `rns-tools` maintainers

## Scope

Defines the BLE peripheral/central frame contract between host bridge and ESP32-class camera firmware.

## Versioning

- Protocol version: `1`
- Unsupported/unknown major versions must fail handshake.

## Frame Types

- `HELLO`
- `CAPTURE_REQ`
- `CAPTURE_ACK`
- `CHUNK`
- `CHUNK_ACK`
- `NACK`
- `DONE`
- `ERROR`
- `HEARTBEAT`

## CHUNK Frame

Required fields:

- `transfer_id` (`u32`)
- `seq` (`u16`)
- `total_chunks` (`u16`)
- `payload_len` (`u16`)
- `crc32` (`u32`)
- `payload` (`bytes`)

Rules:

- `seq` is non-wrapping in v1.
- Transfers requiring `total_chunks > 65535` must fail pre-start with `SDK_VALIDATION_INVALID_ARGUMENT`.

## MTU and Payload Rules

Constants:

- `frame_overhead = 16`

Computation:

1. `att_payload = max(20, negotiated_mtu - 3)`
2. `computed_payload = att_payload - frame_overhead`
3. `max_payload = clamp(computed_payload, 64, 180)`

Fallbacks:

- If negotiated MTU is missing, use `max_payload = 120`.
- If peer advertises `max_payload_supported < 64`, fail with `ERR_UNSUPPORTED_MTU`.

Handshake failure policy:

- `HELLO` timeout is 2s.
- Retry `HELLO` once.
- Two consecutive timeouts fail with `ERR_UNSUPPORTED_MTU`.

## Sequence and ACK/NACK

Receiver state:

- `expected_seq` starts at `0`.

Behavior:

- If `seq == expected_seq`: accept payload, emit `CHUNK_ACK(seq)`, increment `expected_seq`.
- If `seq < expected_seq`: treat as duplicate, re-ACK `expected_seq - 1`, do not reapply payload.
- If `seq > expected_seq`: emit `NACK(expected_seq)`, do not advance state.

NACK streak policy:

- Counter key: `(transfer_id, expected_seq)`.
- Sliding window: 5s using monotonic clock.
- Increment on gap (`seq > expected_seq`) only.
- Clear when `expected_seq` advances.
- If streak reaches 3 within the window: emit `ERROR_RESTART_REQUIRED` and abort transfer.

## Retry Policy

Per-sequence send budget:

- `max_attempts_per_seq = 6` total transmissions (initial + 5 retries).

Backoff schedule (ms):

- `200`, `400`, `800`, `1200`, `1600`

Jitter:

- Uniform `+/-10%` per retry.

Reset conditions:

- Retry budget resets only when ACK advances `expected_seq`.

Exhaustion result:

- `SDK_RUNTIME_BACKPRESSURE_TIMEOUT`

## Error Codes (Protocol-Level)

- `ERR_UNSUPPORTED_MTU`
- `ERR_PAYLOAD_TOO_LARGE_MIN64`
- `ERROR_RESTART_REQUIRED`

SDK-layer machine codes are defined by SDK contracts; firmware error codes are translated at the bridge.
