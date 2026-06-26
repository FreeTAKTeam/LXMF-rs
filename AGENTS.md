# Codex Agent Notes

## Project Structure

- This is a Cargo workspace using resolver `2`, Rust edition `2021`, and
  `rust-version = "1.85"`. Workspace lints forbid `unsafe_code`, deny
  `dbg_macro` and `todo`, and warn on `unwrap_used`.
- Main app crates live under `crates/apps`: `reticulumd` for the daemon,
  `lxmf-cli` for LXMF/LXMD command-line flows, and `rns-tools` for Reticulum
  utilities such as `rnx` and `rnsd`.
- Core library crates live under `crates/libs`: `lxmf-wire` is in
  `lxmf-core`, `lxmf-sdk` provides typed client/backend surfaces,
  `reticulum-rs-core` is in `rns-core`, `reticulum-rs-transport` is in
  `rns-transport`, and `reticulum-rs-rpc` is in `rns-rpc`.
- Embedded and mobile-adjacent crates are split into
  `rns-embedded-core`, `rns-embedded-runtime`, `rns-embedded-ffi`,
  `rns-embedded-mininode`, and `lxmf-embedded-mini`. Shared test fixtures and
  conformance helpers belong in `crates/libs/test-support`.
- Workspace dependency boundaries are intentional. Before adding a new
  workspace-local dependency edge, check `[workspace.metadata.boundaries]` in
  `Cargo.toml` and validate with `tools/scripts/check-boundaries.sh`.
- Build and release automation lives in `xtask` plus `tools/scripts`. Prefer
  existing `cargo xtask ...`, Makefile targets, and scripts over adding new
  ad hoc automation.

## Parity Workflow

- Treat `docs/status/current-roadmap.md` as the repository-level source of
  truth for parity posture, release confidence, and execution order.
  Row-level status lives in `docs/status/lxmf-parity-matrix.md` and
  `docs/status/reticulum-parity-matrix.md`.
- Parity is measured against Python LXMF and Reticulum behavior, not just Rust
  API completeness. A `partial` row can mean useful production behavior exists
  while Python edge behavior, interface breadth, or live evidence is still
  missing.
- When landing a parity increment, update the roadmap and the relevant parity
  matrix in the same change as the behavior and tests. Keep entries tied to
  concrete behavior, evidence, and remaining gaps.
- Prefer small TDD-backed parity deltas. Start from a documented gap, add or
  tighten the focused regression, implement the minimum behavior, then run the
  package-specific tests that cover the changed surface.
- For REM/RCH-facing work, prioritize SDK v2 and the typed ZeroMQ backend when
  the integration path allows it. Do not let HTTP RPC or CLI-only behavior
  become the main thread unless it affects SDK/ZeroMQ behavior, release gates,
  or live verification.
- When comparing against reference implementations, inspect only the specific
  dependency or reference paths needed for the current task. Do not scan or
  fetch sibling repositories broadly unless the user explicitly asks for a
  multi-repository operation.

## Rust Development Practices

- Follow the existing module and crate boundaries before introducing new
  abstractions. Keep shared protocol/domain logic in library crates and keep
  app crates focused on process, CLI, daemon, and integration behavior.
- Prefer `Result` for fallible behavior and reserve `Option` for real absence.
  Use `thiserror` for typed library errors and `anyhow` where application
  boundary code needs context-rich propagation.
- Do not hold mutex guards or other blocking locks across `.await`. Keep lock
  scopes short, document non-obvious lock ordering, and prefer message passing
  or scoped ownership for async worker coordination.
- Use Rust naming conventions consistently: `snake_case` for functions and
  variables, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants, and
  `as_` / `to_` / `into_` prefixes according to conversion cost and ownership.
- Prefer slices, iterators, newtypes, and explicit domain types over stringly
  typed plumbing. Pre-allocate `Vec` / `String` only when the size is known or
  hot-path evidence supports it.
- Avoid new `unwrap()` calls in production code. If an invariant is truly
  guaranteed, use `expect()` with a specific reason; otherwise propagate the
  error or log and recover intentionally.
- Keep files within the active module-size policy: 500 LOC for regular Rust
  modules and 1200 LOC for test/fuzz/bench files. Extract helpers instead of
  adding lint allowances or broad exceptions.
- Use focused validation first, then broaden as risk increases. Common gates
  are `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  `cargo test --workspace --tests`, `tools/scripts/check-boundaries.sh`,
  `cargo run -p xtask -- architecture-checks`, and `cargo xtask release-check`
  for release-facing changes.

## Diagnostics And Error Handling

- For work related to GitHub issue #369, treat silent failure handling as a
  correctness concern, not a style cleanup. Avoid replacing errors with `None`,
  empty byte buffers, or ignored send/write results unless the call site has an
  explicit, documented reason.
- Prefer the existing `log` / `tracing` stack already used by `reticulumd`,
  `lxmf-cli`, `rns-tools`, and the transport crates. Do not introduce a new
  logging framework for issue-driven diagnostics fixes.
- When touching mutex, UTF-8, msgpack, crypto, channel-send, stream-write, or
  resource-transfer paths, distinguish "absent" from "malformed" where callers
  need that difference. Use `Result`, contextual `warn!` / `error!` logging, or
  explicit lossy conversion instead of `.ok()` or `unwrap_or_default()` masking.
- Keep diagnostic logs actionable: include stable context such as peer,
  destination, resource hash, link ID, interface name, or RPC method when that
  context is available. Redact secrets and reuse `rpc_access_log.rs` patterns
  for RPC request metadata.
- After diagnostics/error-handling edits, run the focused regression scanner:
  `cargo test -p reticulumd --test code_quality_issue_369`.
- Logging additions can push Rust files over the active size policy. If touched
  files are near the limit, run `tools/scripts/check-module-size.sh` through
  Git Bash on Windows.
