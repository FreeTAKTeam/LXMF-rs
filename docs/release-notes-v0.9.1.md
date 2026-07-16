# LXMF-rs v0.9.1

`v0.9.1` is a backwards-compatible patch release that adds packet
delivery-proof correlation to the typed SDK API.

## Highlights

- Packet snapshots now expose the proof hash associated with successful
  delivery, allowing SDK clients to correlate delivery proofs with packets
  without accessing internal router state.
- Typed SDK, in-process backend, TCP RPC, and ZeroMQ behavior remain aligned
  for the new optional proof-hash field.
- Existing clients remain compatible because the field is additive and
  absent until a delivery proof has been observed.

## Compatibility

The `0.9.0` wire formats and public Rust items remain supported. This release
contains no migration requirements or intentional breaking changes.

## Published crates

The public LXMF-rs workspace crates and binaries are aligned at `0.9.1` and
are published from the same release tag as the GitHub platform bundles.

## Validation

The release commit must pass the repository CI, `cargo xtask release-check`,
the `rnx` end-to-end smoke test, package dry-runs, and release bundle gates.
