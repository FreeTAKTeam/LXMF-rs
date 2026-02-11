# LXMF-rs <-> Reticulum-rs Compatibility Contract

## Version Mapping
- `lxmf` `0.2.x` currently supports `reticulum-rs` `0.1.x` (tested against `0.1.2`).
- During active refactor development, integration CI may pin exact git revisions.

## Wire/RPC Invariants
- Message packing/unpacking round-trip must be deterministic.
- Invalid frames must return typed errors; no panics in runtime paths.
- Receipt and delivery semantics must remain test-backed and reproducible.

## Release Gate
A release is valid only if:
1. LXMF core tests pass.
2. Cross-repo integration tests pass against pinned reticulum revision.
3. Compatibility matrix is updated with exact versions.
