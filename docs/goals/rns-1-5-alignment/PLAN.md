# RNS 1.5 Alignment and Release Readiness Implementation Plan

**Intent:** Make LXMF-rs behaviorally aligned with the authoritative Python RNS 1.5.0 release and leave one coherent, reviewable release-readiness pull request with complete local and hosted evidence.
**Current Behavior:** `main` pins RNS 1.4.2 at `b48b96e61676504e0a4e527b33b9a0b4495c6872` and reports 1,810 complete applicable entries plus one provenance-only entry. A disposable 1.5.0 inventory reports 28 added callables, 15 partial callables, and 13 newly added callables hidden by broad `complete` wildcard rules. The runtime drains one interface receive queue without RNS 1.5 traffic-class prioritisation or the new queue/violation telemetry contract.
**Expected Outcome:** RNS 1.5.0 at `e32d4df754a7b87b1bf1bb0d08675d12ff505ae6` is the sole Python Reticulum truth source; every upstream release-note behavior is classified in a committed delta ledger; all applicable software behavior is implemented with focused Rust and pinned-Python evidence; strict inventory, architecture, and release gates are green.
**Target-Perspective Output:** A maintainer can inspect the PR, the RNS 1.5 delta ledger, generated inventory, focused interop artifacts, local release-check output, and hosted checks and conclude that the exact PR head is ready to become the next LXMF-rs release candidate. An operator can start `reticulumd`, run `rnstatus-rs --json` and human output against it, and observe configured queue limits, queue pressure/drops, active/total links, transport/interface counters, and adaptive timeout inputs without reading source code.
**Truth Owner:** Python Reticulum tag `1.5.0` for protocol/reference behavior; `tools/interop/independent-implementations.toml` for the repository-local Python RNS/LXMF version and revision pins; `docs/status/current-roadmap.md` and the parity matrices for repository posture; `docs/status/python-surface-mapping.json` plus `tools/scripts/python_surface_inventory.py` for callable classification; Rust library crates for implementation. All pin mirrors are checked against the canonical TOML by the release gate.
**Contract Boundary:** Parsed Reticulum packets cross interface receive channels into `reticulum-rs-transport`; transport/config/status contracts cross `reticulumd` and `reticulum-rs-rpc`; operator-facing status crosses `rns-tools`; generated parity constants cross `lxmf-reference` and SDK/runtime status.
**Cutover:** Replace the 1.4.2 pin and active parity claim with 1.5.0 atomically. Historical 1.4.2 release records remain historical; no current-looking 1.4.2 pin, count, or release-gate comment may remain.
**Displaced Path:** The single undifferentiated inbound drain path is replaced by a bounded, priority-aware ingress path. Broad wildcard classification is demoted behind explicit 1.5 rules and manual behavioral contracts. Fixed CLI timeouts are retained only as lower bounds where the 1.5 medium timeout is larger.
**Value Density:** One PR is preferred because runtime behavior, pins, generated inventory, and release evidence share one atomic compatibility boundary. Split only if an independently releasable slice has disjoint files and tests.
**Acceptance Evidence:** Exact-tag source comparison; release-note-to-Rust delta ledger with no unclassified applicable row; focused queue ordering/drop/batching, timeout, status, discovery, Channel/Buffer, Resource/Link, and config tests; pinned-Python 1.5 probes; regenerated strict inventory with zero partial/unmapped applicable entries; workspace formatting, strict Clippy, tests, boundaries, architecture checks, issue-369 scanner, and `cargo xtask release-check`; hosted PR checks on the exact head.
**Evidence Lane:** Unit and deterministic simulated tests first, pinned-Python interop second, full local release gate third, hosted CI and PR review last. Hardware-specific rows may remain `hardware-unverified` only when the software implementation is complete and the boundary is explicit.
**Kill Criteria:** No duplicated inbound scheduler, no fallback to 1.4.2 for active checks, no wildcard-only promotion of a 1.5 callable, no queue with unbounded growth, no lock guard held across `.await`, and no release-readiness claim while a branch-caused local or hosted gate is red.
**Architecture Slice:** `reticulum-rs-transport` owns packet scheduling/routing/link/resource semantics; `reticulumd` owns configuration and runtime wiring; `reticulum-rs-rpc` owns typed/status exposure; `rns-tools` owns CLI presentation; status documents and generated inventory own evidence, not behavior.
**Plan Review Gate:** Requires PRE review before execution.

## Architecture Map

Files to create:
- `docs/status/rns-1.5-delta.md`
- `tools/scripts/check_python_reference_pins.py` and its focused test.
- `crates/libs/rns-transport/src/transport/inbound_queues.rs` and focused tests under the existing transport test tree.
- Small focused Rust modules/tests under the existing `rns-transport` module tree when needed to respect the 500 LOC policy.

Files to modify:
- Pin/evidence owners: `tools/interop/independent-implementations.toml`, `crates/libs/lxmf-reference/src/lib.rs`, `.github/workflows/verify.yml`, relevant HIL/performance workflows, and `xtask/src/hil/*`.
- Inventory owners: `tools/scripts/python_surface_inventory.py`, `docs/status/python-surface-mapping.json`, generated `docs/status/python-surface-parity.json`, and generated `crates/libs/lxmf-reference/src/python_software_parity.rs`.
- Runtime owners: the displaced receive loop in `crates/libs/rns-transport/src/transport/jobs.rs`; scheduler state in `transport/mod.rs`; construction in `transport/core.rs`; configuration in `transport/config.rs`; the new `transport/inbound_queues.rs`; path-request handling in `transport/path.rs` and `transport/path_requests*`; interface policy/state in `iface_parts/txmessagetype.rs` and `iface_parts/interfacemanager*.rs`; focused `channel`, `channel_buffer`, `resource`, `ratchets`, and discovery modules.
- Boundary owners: focused config/runtime/status files in `crates/apps/reticulumd`, `crates/libs/rns-rpc`, and `crates/apps/rns-tools`.
- Active status owners: `docs/status/current-roadmap.md`, `docs/status/reticulum-parity-matrix.md`, `docs/status/software-parity-ledger.md`, `docs/interop/README.md`, and current generated fixtures/contracts that expose Python reference metadata.

Files to avoid:
- User-owned `/home/pgiuseppe/Documents/LXMF-rs/artifacts/` and the stale checkout branch.
- Historical `docs/status/v0.9.*-release-candidate.md` and historical release notes, except links that deliberately identify their old baseline.
- PR #574 licensing files and unrelated dependency/format churn.

Source of truth: Python Reticulum `1.5.0` tag dereferenced to `e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`; repository pin mirrors derive from or are checked against `tools/interop/independent-implementations.toml`.
Read path: exact upstream tag and changelog -> delta ledger -> explicit mapping/manual contracts -> generated inventory -> runtime status.
Write path: typed daemon config -> `TransportConfig` -> transport scheduler/interface policy -> typed snapshot/RPC -> `rnstatus`.
Integration points: interface RX channels, path-request handling, announce validation/filtering, link Channel outlet MDU, discovery announce codec, daemon status bridge, SDK reference metadata, and release workflows.
Migration/cutover: one atomic PR re-pins all active consumers and regenerates all derived artifacts from the same exact source revision.
Acceptance evidence gate: the exact PR head must pass local `cargo xtask release-check` and required hosted PR workflows; infrastructure or permission failures are reported separately and never called green.

## Task Board

### 1. Establish the authoritative delta and reference cutover

- Files allowed: `docs/status/rns-1.5-delta.md`, pin owners, inventory script/mapping, focused metadata tests.
- Files forbidden: runtime implementation files.
- Output: exact 1.4.2..1.5.0 behavior matrix, explicit rules for all 28 new callables, manual contracts for non-callable release behaviors, canonical TOML pin changed to `e32d4df...`, every enumerated mirror changed, a consistency checker wired into `cargo xtask release-check`, and the new strict expected summary. Mirrors include `lxmf-reference`, `verify.yml`, HIL/performance workflows, `tools/benchmarks/python_impl.toml`, HIL evidence metadata, active README/docs, CLI tests, SDK fixtures, and generated parity metadata; historical v0.9.x evidence and unrelated dependency `1.4.2` strings are allowlisted. Prepare ignored disposable checkouts with `git clone --filter=blob:none https://github.com/markqvist/Reticulum.git .tmp/python-refs/Reticulum`, `git -C .tmp/python-refs/Reticulum checkout --detach e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`, the equivalent LXMF clone/checkout at `727830cefda83d9c6e3982b48675425f3f988f9c`, and never reuse an existing checkout until its exact head is asserted.
- Verification: `git check-ignore .tmp/python-refs/Reticulum`; `test "$(git -C .tmp/python-refs/Reticulum rev-parse HEAD)" = e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`; the equivalent exact LXMF assertion; `python3 --version` captured with evidence; `python3 tools/scripts/check_python_reference_pins.py`; its unit test; inventory self-test; regeneration against exact RNS/LXMF checkouts; scoped `rg` proving only allowlisted historical/unrelated old pins remain. Disposable references remain ignored and are excluded from commits; cleanup uses the repository's normal ignored-temp handling after evidence capture.
- Evidence: zero wildcard-only new callable classifications and zero unclassified release-note rows.
- Parallel safe: no; it defines the contract for every following task.

### 2. Implement the standalone bounded priority queue

- Files allowed: `crates/libs/rns-transport/src/transport/inbound_queues.rs`, `transport/mod.rs`, and focused unit tests only.
- Files forbidden: RPC, CLI, and status documents.
- Output: four bounded traffic classes with RNS defaults (data 4096, announce 256, path request 256, ingress-limited 128), non-blocking enqueue, the exact Python RNS 1.5 total order `data > announce > path request > ingress-limited`, FIFO within each class, stable height/drop snapshots, and cancellation-safe wakeup without holding a lock across `.await`. Draining is intentionally strict rather than fair: a lower class may starve indefinitely while any higher class stays non-empty, exactly matching Python's first-non-empty queue scan.
- Verification: `cargo test -p reticulum-rs-transport inbound_queues -- --nocapture` proving all six pairwise class relationships, FIFO within class, full/drop behavior, snapshot consistency, wakeup, cancellation, and sustained-higher-class starvation of every lower class.
- Evidence: deterministic queue snapshot assertions.
- Parallel safe: no.

### 3. Cut over the single ingress consumer and add early filtering

- Files allowed: `transport/jobs.rs`, `transport/core.rs`, `transport/mod.rs`, `transport/wire.rs`, `iface_parts/txmessagetype.rs`, and focused transport tests.
- Files forbidden: RPC/CLI presentation and path-request semantics.
- Output: the old `jobs.rs` packet receive/process loop is split into exactly one fast interface-RX ingestor and exactly one priority drainer; all packet processing is redirected through the new queue. Malformed/over-MTU where observable, excessive-hop, invalid destination-type, and duplicate packets are rejected before expensive routing/crypto where Rust's parsed-packet boundary permits it; interface violation/filter counters are updated.
- Verification: `cargo test -p reticulum-rs-transport rns_1_5_ingress -- --nocapture`; a structural test/search asserts only the ingestor consumes `InterfaceRxReceiver` and only the drainer invokes the packet-processing function; existing worker-supervision and cancellation tests pass.
- Evidence: ordered transport trace proving queued data overtakes announce/path-request/ingress-limited traffic, each remaining pair follows the declared total order, and the displaced loop no longer exists.
- Parallel safe: no, depends on task 2 snapshots.

### 4. Implement same-destination in-flight path-request batching

- Files allowed: `transport/path.rs`, `transport/path_requests*`, `transport/inbound_queues.rs`, `transport/mod.rs`, and focused path-request tests.
- Files forbidden: daemon/RPC/CLI files.
- Output: requests for the same destination are coalesced while a search is queued/in flight, distinct requesting interfaces are preserved, one recursive search is emitted, and the result/timeout releases all retained requesters without suppressing later requests.
- Verification: `cargo test -p reticulum-rs-transport rns_1_5_path_request_batch -- --nocapture` plus existing path-request/transport-policy tests.
- Evidence: deterministic trace with two tags/two ingress interfaces, one outbound request, and both requesters retained.
- Parallel safe: no.

### 5. Wire queue configuration and transport accessors

- Files allowed: `transport/config.rs`, `transport/core.rs`, focused `reticulumd/src/config*` files and config tests.
- Files forbidden: RPC/CLI rendering.
- Output: validated `qlen_in_data`, `qlen_in_announce`, `qlen_in_pr`, and `qlen_in_il` configuration with RNS defaults; `default_*_queue_length`, total/active link count, lowest online interface bitrate, and medium path timeout accessors wired to runtime state.
- Verification: `cargo test -p reticulumd --test config`; `cargo test -p reticulum-rs-transport rns_1_5_runtime_accessors -- --nocapture`.
- Evidence: parsed config and live transport accessor assertions matching Python formulas/defaults.
- Parallel safe: no.

### 6. Expose typed telemetry through RPC and `rnstatus`

- Files allowed: focused interface counter/state files, `rns-rpc` daemon status files/types, `reticulumd` status bridge files, `crates/apps/rns-tools/src/bin/shared/rnstatus.rs`, and tests.
- Files forbidden: unrelated SDK operations.
- Output: queue pressure/heights/drops; protocol/IFAC/filter violations; announce/path-request counts, bytes, rates/frequencies; data-flow composition/speed; active/total links; and blocked-IP listings in typed/JSON status and human `rnstatus` output.
- Verification: focused transport counter tests; `cargo test -p reticulum-rs-rpc status_snapshot`; `cargo test -p rns-tools rnstatus -- --nocapture`.
- Evidence: start `reticulumd` with an isolated temp config/RPC address, generate deterministic local traffic/queue pressure, run `cargo run -p rns-tools --bin rnstatus-rs -- --rpc <addr> --json` and human output, and retain the important response/output in `target/rns-1.5-evidence/`.
- Parallel safe: no; depends on tasks 3 and 5.

### 7. Align adaptive utility timeout behavior

- Files allowed: focused `rncp`, `rnpath`, `rnprobe`, `rnx`, and `rngit` implementation/tests plus the existing daemon RPC method table.
- Files forbidden: unrelated CLI argument changes.
- Output: medium-bitrate timeout is available through the runtime/RPC boundary and utilities use `max(user/default timeout, medium path timeout)` with correct stdout/stderr and exit behavior.
- Verification: focused RPC operation test and `cargo test -p rns-tools adaptive_timeout -- --nocapture`.
- Evidence: deterministic low-bitrate status fixture produces the larger expected timeout in every affected utility.
- Parallel safe: no.

### 8. Align discovery and Channel/Buffer contracts

- Files allowed: focused discovery codec/lifecycle/config files; `channel.rs`, `channel_buffer*`, link outlet sizing, and focused tests.
- Files forbidden: resource/ratchet/Backbone code.
- Output: optional operator LXMF address round-trips in discovery information; Buffer uses negotiated Channel MDU minus only the two-byte stream header and retains compression limits.
- Verification: focused discovery tests; `cargo test -p reticulum-rs-transport channel_buffer -- --nocapture`; `RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop -- --ignored --nocapture`.
- Evidence: discovery fixture and cross-language Buffer transfer using a negotiated MDU larger than the old fixed packet MDU.
- Parallel safe: no.

### 9. Audit and prove every remaining 1.5 bugfix row

- Files allowed: one ledger-row-sized focused module/test at a time in Resource, Link, ratchets, RNode BLE, Backbone/TCP, rngit Windows path handling, rnodeconf, and speedtest-equivalent surfaces.
- Files forbidden: bundled refactors and unrelated interfaces.
- Output: each remaining release-note row is either fixed with a focused regression, proven already equivalent with a focused regression, or marked mechanism-specific not-applicable with equivalent behavioral proof. No row is promoted from code inspection alone.
- Verification: per-row commands recorded in `docs/status/rns-1.5-delta.md`, including `cargo test -p reticulum-rs-transport resource`, link watchdog/ratchet/RNode/TCP tests, `cargo test -p rns-tools rngit`, and the existing pinned-Python Backbone probes from `tests/hil/cases/interop.toml`.
- Evidence: one named artifact/test per ledger row.
- Parallel safe: no.

### 10. Regenerate parity/status contracts and run pinned-Python evidence

- Files allowed: generated inventory/constants, active roadmap/matrices/ledger, interop docs and fixtures.
- Files forbidden: historical release ledgers.
- Output: zero partial/unmapped applicable entries at the 1.5 boundary; current docs consistently state the exact tag/counts/evidence and remaining hardware-only limits.
- Verification: exact-source inventory regeneration and `--check --require-complete`; `RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PY_REPO=.tmp/python-refs/LXMF LXMF_PYTHON_BIN=python3 LXMF_PY_COMPAT_HARNESS=$PWD/tools/scripts/python_compat_harness.py cargo test -p reticulumd --test python_compat_matrix -- --ignored --nocapture --test-threads=1`; `cargo xtask hil run --level pr --profile python-reference --output target/hil/rns-1.5-pr` with `PYTHON_RNS_PATH`, `PYTHON_LXMF_PATH`, and reference repo variables pointed at the exact local checkouts.
- Evidence: committed generated artifacts match independent regeneration from exact source refs.
- Parallel safe: no, depends on tasks 2-9.

### 11. Release gates, review, PR, and hosted stabilization

- Files allowed: branch-caused fixes only.
- Files forbidden: unrelated cleanup, weakened checks, or skipped tests.
- Output: clean branch, focused and full local gates green, POST plan/correctness/maintainability reviews addressed, one or more necessary PRs published, hosted checks stabilized, and approval limitations explicitly recorded.
- Verification: `cargo fmt --all -- --check`; strict workspace Clippy; `cargo test --workspace --tests`; `tools/scripts/check-boundaries.sh`; `cargo run -p xtask -- architecture-checks`; `cargo test -p reticulumd --test code_quality_issue_369`; `cargo xtask release-check`; exact-head GitHub checks named `CI`, `Full CI`, `Verify`, `Independent interoperability`, `Release Performance`, `Leader Readiness`, and any RNS/HIL-required checks triggered by the PR. Each check's `headSha` must equal the pushed PR head.
- Evidence: pushed commit SHA, PR URL, review findings/fixes, hosted check rollup, and final release-readiness table.
- Parallel safe: no.

## Non-goals and Forbidden Moves

- Do not publish a tag, GitHub release, crates.io packages, images, or downstream consumer upgrades; this goal ends at proven release readiness.
- Do not merge or force-push. The PR sweeper skill forbids approval/merge; attempt only platform- and policy-permitted review actions and report author self-approval as blocked.
- Do not claim physical radio, public-network, or external-client evidence without actually running it.
- Do not preserve an active 1.4.2 compatibility fallback after cutover.
- Do not classify a new callable complete solely because a broad wildcard matched it.

## Risk if Wrong

Wrong queue ordering or lock ownership can deadlock/drop control traffic under load; wrong path batching can suppress legitimate requesters; wrong MDU use can break cross-version Channel streams; stale pins can make green CI prove 1.4.2 instead of 1.5.0; overclaiming wildcard or hardware evidence can ship a release that is structurally complete but not interoperable.
