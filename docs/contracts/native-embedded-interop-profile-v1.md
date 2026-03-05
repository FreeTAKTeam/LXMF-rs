# Native Embedded Interop Profile v1

## Scope

Defines the normative embedded-node interoperability profile for ESP32-class devices participating in LXMF/Reticulum-compatible exchanges.

## Lab Profile Reference

- Release-gated measurements use `docs/contracts/native-embedded-lab-profile-v1.md`.
- Release branches must pin this profile by revision through the native embedded lockfile.

## Normative Encoding Rules

- Packet framing and field serialization must be canonical and deterministic.
- Signature/hash validation uses the same accepted algorithm set as host compatibility targets for this profile.
- Unknown mandatory fields are reject conditions.
- Version mismatch behavior is deterministic: reject with profile-mapped error code.
- TCP transport carries one encoded `RNE1...` packet frame per length-prefixed message:
  - `u16` big-endian payload length
  - payload bytes = encoded packet frame

## Transport Invariants

- Transport may drop/reorder/duplicate; core layer must dedupe/replay-guard.
- Fragmentation/reassembly for attachments belongs to attachment layer, not transport adapter.
- ACK/NACK sequence, timeout, and retry budget are fixed by profile configuration.
- Backpressure handling must emit deterministic machine codes.

## Canonical Transport Parameters

### TCP

- Single active peer in TCP server mode
- Read timeout: `8s`
- Heartbeat interval: `30000ms`
- Reconnect backoff sequence for client mode:
  - `1000ms`
  - `2000ms`
  - `5000ms`
  - `10000ms`
  - `30000ms` max

### BLE recovery transport

- BLE is provisioning/recovery only in this profile
- `0x23` wraps exactly one encoded runtime packet frame
- No adapter-specific fragmentation heuristics are allowed

### Capture transfer

- Default capture max bytes: `1048576`
- Hard capture max bytes: `2097152`
- Images larger than `2097152` bytes must be rejected with machine status `too_large`
- `2097152` bytes exact boundary must be accepted if capture succeeds
- Per-chunk CRC32 is mandatory
- Final SHA-256 completion record is mandatory

## Lifecycle Ownership

- BLE remains enabled for provisioning/recovery while TCP is active
- TCP is the primary runtime transport in normal operation
- BLE may not take ownership of the primary runtime session while TCP is up
- Unknown config schema or transport startup failure must fall back into BLE recovery mode

## Success Response Schemas

- Raw ping:
  - inbound `0x45`
  - outbound `0x46`
  - payload = `pong:` + inbound payload
- LXMF ping:
  - inbound `0x31`
  - outbound `0x31`
  - reply body = `pong:` + inbound body
- Capture success:
  - `0x42` metadata
  - `0x43` chunk stream
  - `0x44` completion
- Capture error statuses:
  - `ok`
  - `busy`
  - `camera_error`
  - `too_large`
  - `timeout`
  - `unsupported`
  - `invalid_request`

## Error Code Mapping

- Native embedded machine codes must map to the canonical set in:
  - `docs/contracts/failure-injection-matrix.md`
  - `docs/contracts/sdk-v2-errors.md`
- Unknown/unmapped codes are CI failures for this profile.

## Fixture Set

- Required fixture IDs for byte-level conformance are tracked under `docs/fixtures` and referenced by HIL reports.
- All fixture IDs used by native embedded tests must be stable and versioned with this profile.
- Success-path fixtures for raw ping, LXMF ping, and capture metadata/chunk/done responses are required for release-gated compatibility.
