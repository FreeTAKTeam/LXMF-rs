# Native Embedded Node Mode Implementation Plan (ESP32)

## Goal

Ship a real embedded Reticulum/LXMF node on ESP32-class hardware that can announce, exchange LXMF payloads, and transfer attachments without requiring a host bridge for core protocol behavior.

## Non-Goals (Initial Scope)

- Full desktop feature parity (all RPC surfaces, all runtime interfaces).
- Large history sync and relay-store behavior identical to desktop daemon.
- Multi-radio optimization and dynamic routing policy tuning.

## Locked Inputs (Must Be True Before Implementation)

1. `docs/contracts/ble-camera-wire-v1.md` and `docs/contracts/ble-transport-runtime-contract.md` are either `active` or pinned by commit hash for this rollout.
2. Attachment chunk compatibility target is fixed to:
- `docs/contracts/sdk-v2-attachments.md`
- `docs/contracts/release-b-rpc-success-responses.md`
3. Firmware integration ownership is explicit for `lxmf-esp32-cam-fw`:
- owner list
- pinned ref per release
- CI artifact contract
4. Native-node HIL path is separate from existing host-RPC lifecycle smoke path.
5. Locked-input enforcement artifact is required:
- `docs/contracts/native-embedded-lockfile.toml` containing pinned contract versions and firmware ref.
- `cargo run -p xtask -- embedded-native-lock-check` must pass before implementation PRs.

## Success Criteria (Release Gate)

1. Interop reliability:
- 200/200 announce exchanges pass between ESP node and host `reticulumd`.
- 200/200 minimal LXMF message roundtrips pass.
2. Attachment reliability:
- 50/50 upload and 50/50 download passes for each size class: `64 KiB`, `1 MiB`.
- At least one forced-disconnect resume passes per size class.
3. Error determinism:
- All required scenarios map to locked machine codes in `docs/contracts/failure-injection-matrix.md`.
- Required machine codes are reconciled with `docs/contracts/sdk-v2-errors.md` with no naming conflicts.
- Zero unknown/unmapped error codes in CI.
4. Performance/resource bounds:
- p95 end-to-end small-message latency <= `2.0s` on lab profile.
- p95 `64 KiB` attachment transfer latency <= `20s` on lab profile.
- Runtime memory ceiling within defined budget (see resource budget section).
5. Rollback safety:
- Bridge fallback auto-enables under trigger thresholds defined in rollout controls.
- Fallback mode remains operational with no protocol regressions.

## Minimum Security Baseline

1. Key provisioning and lifecycle:
- Device identity key origin is defined (factory or enrollment flow).
- Rotation and revocation semantics documented.
2. Verification rules:
- Invalid signature/message authentication always results in deterministic reject code.
- Replay-window checks required on all accepted inbound payloads.
3. Persistence rules:
- Replay window and nonce/sequence state survive reboot.
- Corrupt state recovery path is defined and tested.
4. BLE trust assumptions:
- Link-level trust expectations (pairing/no pairing) explicitly documented.
- If pairing is deferred, threat and mitigation are documented as accepted risk.
5. Security verification artifacts:
- Required test vectors for signature validation, replay rejection, and invalid-auth rejection are published in `docs/fixtures`.
- Required report artifact: `target/hil/native-node-security-report.json`.

## Embedded Resource Budget (Hard Limits)

1. RAM:
- Max runtime heap usage by native-node core: `<= 256 KiB`.
- Max in-flight attachment buffer memory: `<= 32 KiB`.
2. Flash:
- Native-node persistent metadata budget: `<= 256 KiB`.
- Attachment spool budget: `<= 2 MiB` with bounded eviction policy.
3. In-flight concurrency:
- Max concurrent attachment transfers: `1`.
- Max concurrent message decode contexts: `4`.
4. Buffering rule:
- Full-payload buffering is prohibited for attachments.
- Attachment TX/RX must be O(chunk_size) RAM.
5. Measurement requirement:
- Peak heap and in-flight buffer usage must be recorded in `target/embedded/native-node-footprint-report.txt`.
- CI gate `embedded-node-build` fails if any budget threshold is exceeded.

## Architecture Direction

- Introduce a dedicated core crate: `crates/libs/rns-embedded-core`.
- Keep `rns-embedded-core` protocol-focused and runtime-agnostic.
- Define explicit transport and store traits with invariants.
- Keep persistence behind trait boundary for flash-friendly implementations.
- Maintain bridge mode (`bleak` path) as an operational fallback only, not normative architecture.

## Transport Contract Invariants

1. Ordering and delivery:
- Transport may reorder/duplicate/drop frames; core handles dedupe/replay/reassembly.
- Fragmentation ownership is explicit: attachment layer, not transport.
2. Reliability semantics:
- ACK/NACK sequence rules, timeout values, retry budget, and reconnect behavior are fixed.
- Backpressure surface is explicit and testable.
3. Observability:
- Transport emits link-state transitions and drop reasons with bounded-cardinality labels.

## Store Contract Invariants

1. Crash consistency:
- Atomic commit markers for replay window, identity metadata, and transfer cursors.
- Recovery process defined for torn writes.
2. Schema versioning:
- Versioned metadata schema with migration path and corruption policy.
3. Wear controls:
- Write cadence and compaction policy explicitly bounded.
- Journal protocol is required: append-only intent record, commit marker, checksum verification, replay-on-boot.
- Numeric endurance constraints:
  - replay/nonce state writes <= `1` per accepted message.
  - transfer cursor writes <= `1` per committed chunk.
  - estimated write volume remains within 100k-cycle flash endurance assumptions.

## Work Plan

1. Scope Contract and Embedded Interop Profile
- Create embedded interop profile doc with normative rules:
  - exact algorithms
  - canonical serialization
  - field-level compatibility
  - version reject/fallback behavior
  - fixture IDs required for byte-level conformance
- Split normative contracts by path:
  - native-node transport contract
  - camera bridge contract
- Add lock check command and artifact generation:
  - `cargo run -p xtask -- embedded-native-lock-check`
  - `docs/contracts/native-embedded-lockfile.toml`

2. Create `rns-embedded-core` Crate Skeleton
- Add modules:
  - `identity`
  - `packet`
  - `lxmf_min`
  - `attachment`
  - `transport`
  - `store`
- Add constrained build profile gates and compile checks.

3. Add Early Interop Harness and CI Gates (Before Deep Implementation)
- Add minimal harness for:
  - announce visibility
  - tiny message roundtrip
- Add required CI jobs:
  - `embedded-node-build`
  - `embedded-node-contract`
  - `embedded-node-failure-matrix`
  - `embedded-node-hil`
- Map each required job to concrete automation:
  - `xtask` entrypoint command
  - workflow file path
  - branch protection requirement
- Block later phases unless early harness remains green.

4. Port Protocol Primitives
- Implement packet framing/deframing.
- Implement signing/verification and hashing wrappers.
- Implement replay-window guard and nonce/sequence handling.
- Add byte-for-byte fixture conformance tests.

5. Implement Minimal LXMF Layer
- Implement minimal LXMF envelope encode/decode for embedded node mode.
- Support send/receive for small messages.
- Add canonical fixture conformance tests.

6. Implement Attachment Transfer Layer
- Implement chunked sender/receiver with resumable cursors.
- Define deterministic error mapping for timeout, sequence mismatch, and integrity failures.
- Enforce O(chunk_size) memory usage.
- Add retransmission and partial transfer recovery tests.

7. Implement Transport Trait + BLE Adapter
- Define `EmbeddedTransport` trait with invariant tests.
- Implement fault-injecting mock transport (drop/dup/reorder/delay) for contract tests.
- Implement ESP32 BLE adapter.

8. Implement Store Trait + ESP Storage Adapter
- Define `EmbeddedStore` trait with crash-consistency contract tests.
- Implement ESP backend (NVS/flash abstraction).
- Add migration/corruption recovery tests.
- Add reboot persistence tests for replay window and transfer cursors.

9. ESP32 Firmware Integration
- Integrate `rns-embedded-core` execution loop in firmware path.
- Keep bridge mode available behind explicit config.
- Emit required counters:
  - announce sent
  - message TX/RX
  - chunk retries
  - drop reasons
  - fallback reason code

10. Failure Injection, HIL Validation, and Artifacts
- Cover required failure matrix scenarios from `docs/contracts/failure-injection-matrix.md`.
- Reconcile all emitted machine codes with `docs/contracts/sdk-v2-errors.md` and fail CI on drift.
- Add forced-disconnect and power-cycle mid-transfer tests.
- Publish required artifacts:
  - `target/hil/native-node.log`
  - `target/hil/native-node-report.json`
  - per-test machine code assertions and fixture refs
  - `target/hil/native-node-security-report.json`

11. Rollout and Risk Controls
- Phase 1 `experimental`:
  - canary boards only
  - manual enable flag
- Phase 2 limited rollout gate:
  - attachment success rate >= `99%` over last `500` transfers
  - forced-disconnect resume success rate >= `95%`
  - no unknown machine codes in failure matrix runs
- Auto-rollback to bridge mode when:
  - `3` consecutive native-node boot failures
  - or attachment failure rate > `2%` during 24h soak
- Rollback must emit machine-readable reason code and preserve diagnostics.
- Anti-thrash rule:
  - boot-failure counter persists across reboot.
  - after auto-rollback, native mode remains disabled for a cooldown window until explicit operator re-enable.

## Hidden Assumptions (Explicit)

1. Host `reticulumd` compatibility target remains available for each HIL run.
2. ESP32 BLE throughput supports target size classes under defined budgets.
3. Replay and nonce state persistence survives power-cycle with deterministic recovery.
4. Firmware and host contract versions are pinned per release gate.

## Deliverables

- New crate: `crates/libs/rns-embedded-core`
- New embedded interop profile contract and updated transport/store contracts
- ESP firmware native-node integration path
- CI/HIL native-node checks and report artifacts
- Bridge fallback path retained and documented

## Definition of Done

1. All release-gate success criteria pass on ESP32 hardware.
2. Required CI jobs are green with failure-matrix coverage.
3. Native-node mode passes forced-disconnect and power-cycle recovery tests.
4. Bridge fallback remains functional and validated after rollback triggers.
5. Docs/contracts/runbooks are updated with pinned normative references.
6. Reviewer signoff reports no blocking architecture or testability gaps.
7. Lock-check artifact exists and `embedded-native-lock-check` passes.
8. Resource and security report artifacts are present and within thresholds:
- `target/embedded/native-node-footprint-report.txt`
- `target/hil/native-node-security-report.json`
