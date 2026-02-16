# Columba Interop Findings (Rust Port)

Date: 2026-02-16  
Scope: `columba/python/reticulum_wrapper.py` vs `LXMF-rs` + `Reticulum-rs` runtime/RPC transport behavior  
Goal: Rust runtime parity for cross-client compatibility (Columba/Sideband/RCH-class clients)

## Severity Guide
- `P0` critical compatibility break
- `P1` high-impact mismatch
- `P2` medium gap / likely behavioral drift
- `P3` low-priority parity gap

## Findings

### F-001 (`P0`) Outbound method selection is ignored in embedded runtime
- Rust always executes `link -> opportunistic -> propagated relay` fallback, regardless of requested method.
- Columba explicitly sets `desired_method` and `try_propagation_on_fail`.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1042`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1113`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1162`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1222`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4474`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4478`

### F-002 (`P0`) Transport-local send options are leaked onto wire (`_lxmf` field pollution)
- Rust merges `method/stamp_cost/include_ticket` into outbound message fields under `_lxmf`, then signs/sends those fields.
- Columba treats method as message transport intent (`desired_method`), not wire payload field content.
- Impact: remote clients receive non-canonical field keys and transport metadata that should stay local.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:283`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:315`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1053`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1799`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4468`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4474`

### F-003 (`P0`) Source identity/private key semantics are not preserved
- Columba send APIs accept `source_identity_private_key` and load/sign per provided key.
- Rust bridge signs with runtime signer identity (`self.signer`) and source hash from local destination, not per-request key material.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3334`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3371`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4219`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4295`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:754`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1049`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1054`

### F-004 (`P1`) Propagation fetch/ingest RPC is local-cache emulation, not network sync
- `propagation_ingest`/`propagation_fetch` only mutate/read daemon in-memory `propagation_payloads`.
- Columba flow performs real `request_messages_from_propagation_node(...)` using router transfer state machine.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:734`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:775`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4084`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4140`

### F-005 (`P1`) Missing propagation sync APIs/callback model used by Columba clients
- Columba has explicit sync trigger/state APIs (`request_messages_from_propagation_node`, `get_propagation_state`) plus callback loop logic.
- Rust advertised capabilities do not expose these method contracts.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4084`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4163`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1284`

### F-006 (`P1`) Announce ingestion drops critical metadata and only stores name/details
- Runtime forwards announce to daemon via `accept_announce_with_details(...)`, losing app-data metadata path.
- Daemon supports richer `accept_announce_with_metadata(...)` (app_data_hex/capabilities/rssi/snr/q) but runtime does not call it.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:938`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:151`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:172`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1975`

### F-007 (`P1`) Transport announce events lack aspect/hops/interface fields Columba logic depends on
- Announce event currently contains destination/app_data/ratchet only.
- Columba announce path relies on `aspect`, `hops`, and receiving `interface` (and uses them in UI/selection behavior).
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/transport/mod.rs:163`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/transport/announce.rs:80`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2149`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2249`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2252`

### F-008 (`P1`) `list_propagation_nodes` does not filter by propagation aspect/capability
- Current implementation builds node list from all announces.
- This can surface non-propagation peers as relay candidates.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:848`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:859`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:577`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:581`

### F-009 (`P1`) Capability extraction parser is schema-misaligned with current PN app-data
- Parser expects capability payload at array index `2`; router PN announce format uses index `2` for `node_state` boolean.
- Results: capabilities likely empty/incorrect for PN announces.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:356`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:380`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/router/announce.rs:50`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/router/announce.rs:53`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/router/announce.rs:57`

### F-010 (`P1`) Runtime only creates/announces `lxmf.delivery`, not `lxmf.propagation`
- Embedded transport registers a single local destination (`lxmf.delivery`) and announce app-data for that.
- No runtime-side propagation destination announce path exists.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:736`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:744`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:580`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:581`

### F-011 (`P1`) Timestamp unit mismatch (seconds vs milliseconds)
- Rust stores/forwards announce and message timestamps as seconds (`i64`), while Columba event contracts use milliseconds.
- This causes client-side ordering/UI drift if consumed as ms.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:933`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1664`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:319`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2251`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3102`

### F-012 (`P1`) Destination hash normalization is stricter than Columba call patterns
- Rust parse accepts only 16-byte hex destination hashes.
- Columba send path explicitly handles both 16-byte and 32-byte hash inputs.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1610`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1620`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3393`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3395`

### F-013 (`P2`) Inbound relaxed decode path accepts unverified wire structure
- If strict decode fails, runtime attempts relaxed parse that trusts header layout and msgpack payload without signature verification.
- This diverges from strict identity/signature semantics expected in cross-client flows.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1671`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1695`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/message/mod.rs:87`

### F-014 (`P2`) Field map conversion is lossy for integer-keyed LXMF fields
- Inbound conversion maps integer keys to JSON string keys; outbound generic JSON path preserves them as strings unless `_lxmf_fields_msgpack_b64` wrapper is used.
- This can break round-trip fidelity for clients expecting integer field IDs.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1818`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1842`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1812`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/payload_fields.rs:78`

### F-015 (`P2`) Opportunistic constraints from Columba are not mirrored
- Columba enforces opportunistic content/attachment constraints before sending.
- Rust opportunistic fallback attempt is unconditional after link failure.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4274`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4282`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1162`

### F-016 (`P2`) No parity for Columba alternative-relay fallback control flow
- Columba has callback-driven alternative relay selection and retry mechanics.
- Rust capabilities do not expose equivalent callback/API flow for relay reselection.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:1041`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:5057`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1284`

### F-017 (`P2`) Missing high-level interop APIs present in Columba wrapper
- Columba exposes telemetry/reaction/path-link APIs that are absent from Rust capability contract.
- Examples: `send_location_telemetry`, `send_telemetry_request`, `send_reaction`, `has_path`, `request_path`, `establish_link`.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3681`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3860`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:4554`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:6116`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:6123`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:6363`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1284`

### F-018 (`P2`) Propagation "enabled" state is derived from transport presence, not propagation service state
- Runtime marks propagation enabled when transport exists, even without PN service configuration or sync logic.
- This can mislead clients into thinking propagation receive/sync is available.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:786`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:707`

### F-019 (`P2`) Inbound message metadata parity gap (hops/interface/signal)
- Columba captures and surfaces `hops`, `receiving_interface`, `rssi`, and `snr` on inbound messages.
- Rust inbound path receives only destination/data/ratchet and does not surface equivalent metadata.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/transport/mod.rs:125`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/transport/wire.rs:331`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:823`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2991`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3019`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3110`

### F-020 (`P2`) RMSP map-server announce/domain behavior has no Rust parity surface
- Columba registers `rmsp.maps` announce handlers and parses RMSP announce payloads into server registry APIs.
- Rust runtime/RPC capability surface currently has no RMSP method family.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:583`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2256`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:7902`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1284`

### F-021 (`P2`) Announce persistence schema cannot store several Columba-consumed fields
- Rust announce storage tracks `name`, `capabilities`, and signal values, but not `aspect`, `hops`, `interface`, `stamp_cost_flexibility`, or `peering_cost`.
- This prevents parity with Columba's announce event model and limits deterministic replay from persisted announce history.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/storage/messages.rs:17`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/storage/messages.rs:241`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2247`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2249`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2252`

### F-022 (`P2`) Telemetry domain handling is partial vs Columba behavior
- Columba has explicit handling for `FIELD_TELEMETRY_STREAM (0x03)` and `FIELD_COLUMBA_META (0x70)` semantics (location-only routing, cease handling, callbacks).
- Rust runtime only special-cases field key `"2"` during field conversion.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/constants.rs:3`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1850`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2719`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2758`

### F-023 (`P2`) Field 16 app-extension parity gap (reaction/reply extraction pipeline)
- Columba treats field `16` as app extensions, extracts `reaction_to/emoji/sender` and `reply_to`, and exposes dedicated event fields/callback behavior.
- Rust runtime currently surfaces raw field maps and has no corresponding extraction/event model.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2918`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:2926`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:6042`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:1818`

### F-024 (`P2`) Peer identity restore/persistence parity gap
- Columba provides explicit identity restoration paths (`store_peer_identity`, `restore_all_peer_identities`, bulk restore variants) to avoid cold-start path/recall failures.
- Rust runtime peer crypto state is in-memory map only in current flow.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:5569`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:5683`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:5747`
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:5830`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:673`

### F-025 (`P3`) Incoming transfer-size policy control missing from Rust capability surface
- Columba exposes runtime control for incoming message limits that affects direct and propagation transfer handling.
- No equivalent RPC capability is currently advertised by Rust daemon.
- Evidence:
  - `/Users/tommy/Documents/TAK/columba/python/reticulum_wrapper.py:3994`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1284`

### F-026 (`P2`) Stamp/ticket policy exists in RPC but is not integrated into send execution
- Rust exposes `stamp_policy_*` and `ticket_generate`, but outbound send path does not consume ticket cache/policy; send options are merged into fields metadata.
- This diverges from client expectations where stamp/ticket controls alter delivery mechanics, not payload decoration.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:943`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:979`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1215`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:305`

### F-027 (`P3`) RPC capability advertisement is incomplete vs implemented method set
- Daemon implements additional methods not listed in `capabilities()` (`announce_received`, `receive_message`, `record_receipt`, `clear_*` family).

## Test Performance Findings

### T-001 (P3) Reticulum test suite was taking ~4m00 in full-target mode
- Root cause in local runs was `cargo test --workspace --all-targets --all-features`, which rebuilt examples/doc/binary targets and repeatedly invoked cargo inside tests.
- Remediation:
  - Reduced default test command to `cargo test --workspace --all-features` and kept full target sweep as opt-in.
  - Optimized `crates/reticulum/tests/cli_parity.rs` to execute prebuilt CLI binaries directly instead of `cargo run` per assertion.
  - Optimized `crates/reticulum/tests/examples_compile.rs` to avoid repeated `cargo build --examples` calls.
  - Updated CI/docs/process docs to use fast default test profile.
- Current behavior:
  - Full test command default now tracks the behavior under `--all-features` (fast mode).
  - Full-target run remains available as an explicit opt-in command for completeness checks.
- Clients relying on capability discovery may underutilize available methods or mis-detect contract behavior.
- Evidence:
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:569`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:594`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1043`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1074`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1101`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/daemon.rs:1284`

### T-002 (P2) Local gate workflows were doing full-target test runs by default
- `make release-gate-local` and CI validation examples invoked `cargo test --workspace --all-targets --all-features`, which is a noisy default even when only behavior tests are required.
- The interop path was also opaque, with no explicit "full target sweep" target and no documented fast-path command.
- Remediation:
  - Updated local release gate path to use `make test` (`cargo test --workspace --all-features`).
  - Added `make test-all-targets` as explicit, opt-in full-target coverage.
  - Updated CI and templates (`.github/workflows/ci.yml`, `.github/pull_request_template.md`, `CONTRIBUTING.md`, `docs/release-candidate-runbook.md`, `docs/release-readiness.md`, `docs/plans/*parity-matrix.md`) to default to `cargo test --workspace --all-features`.
  - Kept `cargo test --workspace --all-targets --all-features` as explicit documentation for parity-heavy checks.
- Current status:
  - Fast default gate is now the behavior-parity suite.
  - Full-target suite remains available when diagnosing binary/example compile regressions.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/.github/workflows/ci.yml`
  - `/Users/tommy/Documents/TAK/LXMF-rs/Makefile`
  - `/Users/tommy/Documents/TAK/LXMF-rs/CONTRIBUTING.md`

### T-003 (P3) Standard test docs still described all-features as the default
- After introducing `make test` as the fast default (`--features cli`), several docs and templates still described all-features runs as the standard local path.
- Impact: contributors and reviewers could still launch slower command paths when a fast local parity check was intended.
- Remediation:
  - Standardized local and release documentation to `cargo test --workspace --features cli` for fast command.
  - Kept explicit opt-in broader runs: `cargo test --workspace --all-features` and `cargo test --workspace --all-features --all-targets`.
  - Added `test-full` target alias to avoid confusing missing target names.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/Makefile`
  - `/Users/tommy/Documents/TAK/LXMF-rs/README.md`
  - `/Users/tommy/Documents/TAK/LXMF-rs/CONTRIBUTING.md`
  - `/Users/tommy/Documents/TAK/LXMF-rs/docs/release-candidate-runbook.md`
  - `/Users/tommy/Documents/TAK/LXMF-rs/docs/release-readiness.md`
  - `/Users/tommy/Documents/TAK/LXMF-rs/.github/pull_request_template.md`

### T-004 (P1) Daemon/iface startup tests were slower and one path flaked with aggressive timings
- Daemon/iface startup tests had deterministic timing assumptions that inflated suite runtime and allowed an `--all-features` flake in `daemon_supervisor_errors_when_process_exits_immediately`.
- Remediation:
  - Added `StartupTimingGuard` in `crates/lxmf/tests/lxmf_daemon_commands.rs` and `crates/lxmf/tests/lxmf_daemon_supervisor.rs` to drive startup checks to 150ms grace / 10ms poll for all relevant startup-path tests.
  - Kept the exit-fast regression test on default startup timing and removed the guard there to avoid false positive under some scheduler/load conditions.
  - Reduced transport inference polling in `crates/lxmf/tests/lxmf_daemon_supervisor.rs` from a 2s loop to 1s.
  - Reworked `crates/lxmf/tests/lxmf_iface_commands.rs` to a blocking accept path with 250ms stream read timeout and a shorter total listener window.
- Measured outcomes (post-change):
  - `cargo test --workspace --features cli`: ~5.76s warm run (clean baseline: ~48.76s).
  - `cargo test --workspace --all-features`: ~5.72s warm run.
  - `cargo test --workspace --all-targets --all-features`: ~4.15s warm run.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/tests/lxmf_daemon_commands.rs`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/tests/lxmf_daemon_supervisor.rs`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/tests/lxmf_iface_commands.rs`

### T-005 (P3) Test command was doing full integration-slice runs for local iteration
- The default local test command (`make test`) ran all integration binaries, so compile/run noise dominated local feedback.
- Remediation:
  - Split test targets in `/Users/tommy/Documents/TAK/LXMF-rs/Makefile` so `make test` now runs:
    - unit tests (`cargo test --workspace --features cli --lib`)
    - core CLI/parity smoke targets (`api_surface`, `error_smoke`, `smoke`, `lxmf_cli_args`, `lxmf_daemon_commands`, `lxmf_daemon_supervisor`, `lxmf_iface_commands`, `lxmf_message_commands`, `lxmf_peer_commands`, `lxmf_profile`, `lxmf_rpc_client`, `lxmf_runtime_context`)
  - Kept explicit full compatibility and all-target sweeps (`make test-all`, `make test-all-targets`) as opt-in commands.
  - Updated `release-gate-local` to run `make test-all` so release checks remain comprehensive.
- Measured outcomes:
  - `make test` (fast path): `real 1.49s` warm.
  - `make test-all` (full matrix path): `real 4.81s` warm, `~57s` clean baseline observed previously.

### F-028 (`P2`) `send_message_v2` fallback can silently drop method/stamp/ticket intent
- Runtime falls back to legacy `send_message` if `send_message_v2` fails.
- Legacy params schema lacks `method/stamp_cost/include_ticket`, so those controls are silently discarded in fallback path.
- Evidence:
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:324`
  - `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/src/runtime/mod.rs:326`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:132`
  - `/Users/tommy/Documents/TAK/Reticulum-rs/crates/reticulum/src/rpc/mod.rs:143`

## Notes
- Existing working-tree change observed before this audit: `/Users/tommy/Documents/TAK/LXMF-rs/crates/lxmf/Cargo.toml`.
- This file captures confirmed gaps only; additional pass is still in progress for lower-level wire/receipt edge cases.

## Implementation Update (2026-02-16)

### Resolved
- `F-001`: Outbound method intent is now enforced in runtime bridge (`direct` / `opportunistic` / `propagated` / `auto`) instead of always forcing full fallback chain.
- `F-002`: `_lxmf` transport options are no longer merged into wire payload fields.
- `F-006`: Runtime now forwards announce metadata through `accept_announce_with_metadata(...)` with app-data/aspect/hops/interface context.
- `F-007`: Transport announce events now carry `hops`, `interface`, and `name_hash` for aspect derivation.
- `F-008`: `list_propagation_nodes` now filters to propagation-aspect/capability announces only.
- `F-009`: Capability derivation now handles current PN app-data schema and active node-state semantics.
- `F-012`: Destination hash parsing now accepts both 16-byte and 32-byte hex inputs (32-byte values normalize to first 16 bytes).
- `F-015`: Opportunistic constraints are enforced (size + attachment checks) with controlled fallback behavior.
- `F-018`: Embedded runtime no longer reports propagation service enabled by transport presence alone.
- `F-021`: Announce persistence schema now includes additional parity fields (`aspect`, `hops`, `interface`, stamp/peering cost metadata).
- `F-026`: Stamp/ticket/method controls are retained as delivery options and traces, not payload decoration.
- `F-027`: RPC capability advertisement now includes implemented methods (`announce_received`, `receive_message`, `record_receipt`, `clear_*`).
- `F-028`: `send_message_v2` fallback now preserves v2 semantics by refusing legacy fallback when v2-only options are present.

### Partially Resolved
- `F-003`: Added per-message `source_private_key` support across RPC/runtime send path; source hash now derives from key when supplied. Remaining gap: full parity with all external source-identity restore flows.
- `F-011`: Added `timestamp_ms` to announce/inbound/announce_sent events while preserving stored `timestamp` compatibility fields.
- `F-019`: Inbound transport metadata (`hops`, `interface`, `ratchet_used`) is now surfaced under `_transport` in message fields; full top-level parity fields remain outstanding.

### Still Open
- `F-004`, `F-005`: Propagation fetch/sync still uses local-cache semantics and lacks full network sync state machine parity.
- `F-010`: Runtime still does not expose full `lxmf.propagation` destination service behavior.
- `F-013`: Relaxed decode fallback remains available and can parse structure without full strict verification path parity.
- `F-014`: Integer-key field-map fidelity remains partially lossy in generic JSON conversions.
- `F-016`, `F-017`: Callback-driven alternative-relay and high-level wrapper API parity remain incomplete.
- `F-020`: RMSP announce/domain API family is still absent.
- `F-022`, `F-023`: Telemetry/app-extension domain extraction parity remains partial.
- `F-024`, `F-025`: Identity restore persistence and transfer-size control APIs remain incomplete.

## Implementation Update (2026-02-16, Pass 2)

### Resolved
- `F-010`: Embedded runtime now registers/announces both `lxmf.delivery` and `lxmf.propagation` destinations.
- `F-013`: Relaxed decode fallback is now disabled by default and gated by explicit env opt-in.
- `F-023`: Field `16` app extensions now feed dedicated reaction/reply extraction fields on inbound conversion.
- `F-024`: Peer identity restore/persistence paths are now implemented (RPC identity restore methods + runtime peer identity cache load/save).
- `F-025`: Incoming transfer-size controls are now exposed through RPC (`set/get_incoming_message_size_limit`).

### Partially Resolved
- `F-004`: `request_messages_from_propagation_node` now exists with deterministic transfer-state transitions, but backing fetch remains local store-backed rather than full network transfer parity.
- `F-005`: Propagation sync APIs/state (`request_messages_from_propagation_node`, `get_propagation_state`, propagation state events) are now exposed; callback-loop parity is still simplified.
- `F-014`: Outbound JSON->msgpack conversion now restores integer map keys where representable; full arbitrary round-trip fidelity still depends on `_lxmf_fields_msgpack_b64`.
- `F-017`: Added high-level RPC surfaces (`has_path`, `request_path`, `establish_link`, `send_location_telemetry`, `send_telemetry_request`, `send_reaction`), but richer client-side callback parity remains limited.
- `F-020`: RMSP parity surface added (`parse_rmsp_announce`, `get_rmsp_servers`, `get_rmsp_servers_for_geohash`) plus `rmsp.maps` announce aspect recognition; full map-client domain parity remains partial.
- `F-022`: Telemetry domain handling expanded for stream/meta fields and outbound telemetry request/location helper methods, but full collector-callback parity remains partial.

### Still Open
- `F-004`, `F-005`: Full propagation network sync semantics (path/link/transfer state machine parity with real propagation node exchange) remain incomplete.
- `F-016`: Callback-driven alternative-relay fallback/reselection control flow parity remains incomplete.

_Superseded by Pass 3 update below._

## Implementation Update (2026-02-16, Pass 3)

### Resolved
- `F-004`: Embedded runtime `request_messages_from_propagation_node` now performs real propagation-node sync over transport link request/response flow (`/get`) instead of cache-only emulation.
  - Includes path request, link establishment, link identify, message-list request, message fetch request, and sync acknowledgement (`have` list) behavior.
  - Ingested propagation payloads are decoded into inbound messages when possible, with fallback ingest persistence when decode is not possible.
- `F-005`: Propagation sync API/state model now matches Columba transfer-state semantics more closely in embedded runtime.
  - Full state progression support added for `path_requested`, `link_establishing`, `link_established`, `request_sent`, `response_received`, `receiving`, and `complete`.
  - Error-state mapping added for `no_path`, `link_failed`, `transfer_failed`, `no_identity_rcvd`, and `no_access`.
  - State updates are emitted through `propagation_state` events on each transition.
- `F-016`: Alternative relay callback/reselection flow now has explicit parity surfaces.
  - Runtime emits machine-readable relay fallback requests with excluded relays via status + `alternative_relay_request` event emission.
  - Runtime now waits for externally selected new relay candidates and retries propagation with that relay before terminal failure.
  - RPC surface now includes `request_alternative_propagation_relay` to deterministically choose/select next relay excluding already-tried relays.

### Partially Resolved
- `F-016`: External relay retry window is bounded (currently timed wait) and not an unbounded callback loop; behavior is deterministic but intentionally constrained.

### Residual Risk
- Embedded runtime path now uses live propagation sync semantics; standalone daemon-only flows without embedded transport still retain local fallback behavior for compatibility/testing contexts.
