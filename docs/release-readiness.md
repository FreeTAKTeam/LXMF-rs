# Release Readiness Checklist

This checklist is the publication gate for `lxmf-rs`.

## 1. Parity truth

- LXMF parity status is tracked in `docs/plans/lxmf-parity-matrix.md`.
- Reticulum parity status is tracked in `docs/plans/reticulum-parity-matrix.md`.
- Both matrices must be updated when features or tests change.

## 2. Interop gates

- Python fixture compatibility tests must pass (`tests/*parity*.rs`, `tests/fixture_loader.rs`, `tests/python_interop_gate.rs`).
- Live Python interop gate is enabled with `LXMF_PYTHON_INTEROP=1` and is required on Linux before release.
- Any wire/storage format changes require updated fixtures and parity tests.

## 3. API stability

- Public API surface is documented in `docs/lxmf-rs-api.md`.
- Breaking changes must be called out in release notes.

## 4. CI quality gates

- GitHub CI must pass on Linux and macOS.
- Required checks:
  - `git ls-files '*.rs' | xargs rustfmt --edition 2021 --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

## 5. Release metadata

- `Cargo.toml` version bumped intentionally.
- `Cargo.lock` committed for reproducible builds.
- Changelog/release notes summarize parity changes and migrations.
