# Native Embedded Interop Profile v1

## Scope

Defines the normative embedded-node interoperability profile for ESP32-class devices participating in LXMF/Reticulum-compatible exchanges.

## Normative Encoding Rules

- Packet framing and field serialization must be canonical and deterministic.
- Signature/hash validation uses the same accepted algorithm set as host compatibility targets for this profile.
- Unknown mandatory fields are reject conditions.
- Version mismatch behavior is deterministic: reject with profile-mapped error code.

## Transport Invariants

- Transport may drop/reorder/duplicate; core layer must dedupe/replay-guard.
- Fragmentation/reassembly for attachments belongs to attachment layer, not transport adapter.
- ACK/NACK sequence, timeout, and retry budget are fixed by profile configuration.
- Backpressure handling must emit deterministic machine codes.

## Error Code Mapping

- Native embedded machine codes must map to the canonical set in:
  - `docs/contracts/failure-injection-matrix.md`
  - `docs/contracts/sdk-v2-errors.md`
- Unknown/unmapped codes are CI failures for this profile.

## Fixture Set

- Required fixture IDs for byte-level conformance are tracked under `docs/fixtures` and referenced by HIL reports.
- All fixture IDs used by native embedded tests must be stable and versioned with this profile.
