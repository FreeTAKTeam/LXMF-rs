# Contributing to LXMF-rs

## Goals
- Keep the library lightweight and portable by default.
- Prioritise reliability, deterministic behavior, and testability.
- Keep operational tooling separate from core protocol logic.

## Development Setup
- Rust stable (MSRV `1.75` is tracked in `crates/lxmf/Cargo.toml`).
- `cargo install cargo-deny cargo-audit cargo-udeps`

## Local Quality Gates
Run these before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
make test
# Optional compatibility coverage:
# make test-all
# Full-target sweep:
# make test-full-targets
cargo doc --workspace --no-deps
cargo deny check
cargo audit
cargo +nightly udeps --workspace --all-targets
```

## Pull Requests
- One logical change per PR.
- Include tests for behavior changes.
- Mark breaking changes clearly.
- Update docs and compatibility matrix when interfaces or wire behavior changes.
