# Release Readiness Checklist

This checklist is the publication gate for `lxmf-rs`.

## 1. Parity truth

- LXMF parity status is tracked in `docs/plans/lxmf-parity-matrix.md`.
- Reticulum parity status is tracked in `docs/plans/reticulum-parity-matrix.md`.
- Both matrices must be updated when features or tests change.

## 2. Interop gates

- Python fixture compatibility tests must pass (`tests/*parity*.rs`, `tests/fixture_loader.rs`, `tests/python_interop_gate.rs`).
- Live Python interop gate is enabled with `LXMF_PYTHON_INTEROP=1` and is required on Linux before release.
- Sideband interoperability gate must pass (`make sideband-e2e`) for release candidates.
- Any wire/storage format changes require updated fixtures and parity tests.
- Semantic replay gate must pass in `tests/python_client_replay_gate.rs`:
  - reply linkage (`reply_to`)
  - reactions (`reaction_to`, `reaction_emoji`, `reaction_sender`)
  - telemetry location extraction (`lat`, `lon`, optional `alt`)
  - command field ID preservation (`0x09`)
  - extension capability list normalization (`0x10`)
- Strict desktop interop gate must pass for core payload classes:
  - text
  - attachments (`attachments` public key, `0x05` internal wire field)
  - paper URI workflows
  - commands (`0x09`)
  - reply/reaction app extensions (`0x10`)
  - location telemetry (`0x02`)
  - announce metadata/capabilities parity

## 3. Async contract conformance

- Async client contract is documented in `docs/lxmf-async-api.yaml`.
- Scenario matrix and migration gates are tracked in `docs/async-conformance-matrix.md`.
- Contract harness baseline scenarios (`C01-C03`) must pass in `tests/contract_harness.rs`.
- Before defaulting clients to Rust backend, required matrix lanes in `docs/async-conformance-matrix.md` must be marked `done`.

## 4. API stability

- Public API surface is documented in `docs/lxmf-rs-api.md`.
- CLI daemon RPC method contract is documented in `docs/rpc-contract.md`.
- Message/announce payload contract is documented in `docs/payload-contract.md`.
- Contract v2 schema artifacts are present and mirrored in Weft:
  - `docs/schemas/contract-v2/payload-envelope.schema.json`
  - `docs/schemas/contract-v2/event-payload.schema.json`
- RPC contract tests must pass (`tests/rpc_contract_methods.rs`).
- Breaking changes must be called out in release notes.

## 5. CI quality gates

- GitHub CI must pass on Linux and macOS.
- Linux CI installs pinned Python `Reticulum` and `LXMF` commits and runs interop gate with `LXMF_PYTHON_INTEROP=1`.
- Sideband end-to-end workflow (`.github/workflows/sideband-e2e.yml`) must pass for release candidates.
- Required checks:
  - `git ls-files '*.rs' | xargs rustfmt --edition 2021 --check`
  - `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`
  - `make test`
  - `make test-all` (compatibility pass)
  - `make test-full-targets` (full-target sweep when validating release binaries/examples)

## 6. Release metadata

- `Cargo.toml` version bumped intentionally.
- `Cargo.lock` committed for reproducible builds.
- Changelog/release notes summarize parity changes and migrations.
- Clean-break migration note is updated: `docs/migrations/2026-clean-break-unification.md`.
- RC execution and tagging follow `docs/release-candidate-runbook.md`.
