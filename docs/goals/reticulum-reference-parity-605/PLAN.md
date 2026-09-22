# Full Reticulum Reference Parity Software Acceptance Plan

**Intent:** Make LXMF-rs a software-level operationally substitutable Rust implementation of the frozen Reticulum reference behavior described by issue #605, with implementation, runtime integration, differential evidence, and exact-candidate release gates. Physical carriers, platform certification, external-client validation, and public-network soak are explicitly excluded from this goal.
**Current Behavior:** `main` has an advisory callable inventory generated from public Python functions/classes/methods. The inventory accepts wildcard mappings, uses a fixed RNS 1.5.2 release count (`1,858` entries, `1,857` complete, one not-applicable), and does not require a traceable behavioral requirement, production-path evidence, or an exact evidence artifact for each row. The maintained roadmap therefore reports complete software-surface parity while issue #605 records known gaps in IFAC wiring, transport/runtime behavior, Resource semantics, utilities, rngit, native interfaces, and physical/client evidence. PR #604 is a bounded initial increment, not completion of this epic.
**Expected Outcome:** The exact Python Reticulum 1.5.4 development revision named by #605 (`99de23c040d507e3fefca19e87b182302902725d`) or an explicitly reviewed replacement is frozen as the acceptance target. Every applicable software reference behavior has a traceable requirement, Rust owner, executable test, and evidence record; production paths and utilities are exercised; and the final release gate evaluates the exact candidate commit rather than historical or callable-only evidence. Physical/platform evidence remains a separate non-gating scope.
**Target-Perspective Output:** A maintainer can inspect the frozen-reference manifest, behavioral inventory, delta ledger, linked implementation PRs, raw evidence, generated status, and exact-head CI/release reports and determine which software rows are complete, partial, not-applicable, or blocked. Hardware-unverified and external-client rows remain visible as separate evidence, but are not silently promoted by this software goal.
**Truth Owner:** The frozen Python Reticulum revision owns reference behavior; `tools/interop/independent-implementations.toml` owns repository-local reference pins; the behavioral inventory and delta ledger own requirement/evidence classification; Rust library and application crates own implementation; `docs/status/current-roadmap.md` and the maintained parity matrices own current posture; `xtask`, CI, and evidence scripts own executable gates. Historical release records remain historical and cannot be rewritten into the current candidate truth.
**Contract Boundary:** Exact Python source and release notes flow into a behavior/evidence inventory and delta ledger, then into Rust protocol/runtime/utility implementations and focused regressions, then through daemon/RPC/CLI production paths, then into machine-readable evidence and release/support reports. Credentials, device identity, private traffic, and external-client secrets remain outside committed evidence.
**Cutover:** Freeze the reference and behavioral contract first (#606). Demote the callable-only completion claim and fixed-count gate to historical context, while preserving old release artifacts. Promote current software parity/status/release claims only after #607–#614 implementation evidence and #615 software acceptance pass. #616 is explicitly outside this goal and remains a separate operational gate. No child PR uses `Closes #605`.
**Displaced Path:** The current public-callable scan, broad wildcard mapping, fixed expected count, and advisory “complete” status are replaced as current acceptance authorities by explicit behavior rows and evidence-backed statuses. Existing generators and status files are extended in place; no competing inventory or second daemon/network protocol is introduced.
**Value Density:** The work is split into dependency-aware child PRs because the issue separates reference governance, software implementation, differential release acceptance, and physical/platform validation. Each child must have a disjoint owner boundary and evidence contract; this goal ends at software acceptance, while #616 remains outside its completion criterion.
**Acceptance Evidence:** Exact-source checkout and pin verification; behavioral inventory generation and deliberate-failure tests; focused Rust regressions; Python↔Rust mixed-peer and multi-process evidence; production daemon/RPC/CLI traces; and exact-candidate CI/release gates. Passing local unit tests or finding a matching symbol is insufficient. Platform/device/client evidence is outside this goal.
**Evidence Lane:** Map and freeze the reference first; add failing focused tests; implement the smallest owner-local behavior; validate deterministic/unit and simulated paths; run pinned-Python and multi-process interop; and run exact-candidate software release gates. Physical/platform/client evidence is not executed by this goal; if mentioned, unavailable hardware or runners remain blocked/unverified, never pass.
**Kill Criteria:** Stop and re-plan if a change introduces a second source of truth, a parallel daemon/protocol, wildcard-only promotion, silent fallback to unauthenticated/plaintext behavior, skipped required tests, fabricated evidence, an unbounded queue/resource path, a lock held across `.await`, or a current parity/release claim that is not tied to the exact candidate.
**Architecture Slice:** `reticulum-rs-core` owns protocol/identity/crypto primitives; `reticulum-rs-transport` owns routing, links, Resources, queues, IFAC carrier boundaries, discovery, and interface runtimes; `reticulumd` owns configuration and daemon wiring; `reticulum-rs-rpc` owns typed status/control; `rns-tools` owns utility/network workflows and operator presentation; `lxmf-reference` exposes generated reference metadata; `tools/scripts`, `xtask`, CI, and `docs/status` own inventory/evidence/release contracts.
**Plan Review Gate:** Requires PRE review before execution. The first executable gate is #606; later implementation work must not promote a status row until its owner and evidence gate exist.

## Architecture Map

Files to create or extend:

- `docs/goals/reticulum-reference-parity-605/PLAN.md` and `GOAL.md` for this execution contract.
- The #606 behavioral-reference manifest/delta/evidence schema in the existing `tools/scripts`, `docs/status`, and `tools/interop` owners; exact filenames are selected during the #606 architecture pass so no competing status system is created.
- Focused tests/evidence fixtures under the existing crate test trees, `tools/interop`, `tools/scripts`, and `target/` evidence output paths; committed raw credentials or private traffic are forbidden.

Files to modify by owner:

- Reference and inventory: `tools/interop/independent-implementations.toml`, `tools/scripts/check_python_reference_pins.py`, `tools/scripts/python_surface_inventory.py`, `docs/status/python-surface-mapping.json`, generated `docs/status/python-surface-parity.json`, generated `crates/libs/lxmf-reference/src/python_software_parity.rs`, and `crates/libs/lxmf-reference/src/lib.rs`.
- Current posture: `docs/status/current-roadmap.md`, `docs/status/reticulum-parity-matrix.md`, `docs/status/software-parity-ledger.md`, and the active interop/release documentation that mirrors the frozen reference.
- IFAC/runtime transport: existing `crates/libs/rns-transport` interface/transport modules and `crates/apps/reticulumd` configuration and carrier wiring, preserving the existing `IfacContext` and typed boundaries (#608).
- Routing/discovery/shared instances: existing `crates/libs/rns-transport/src/transport`, link/discovery modules, daemon integration, and focused tests (#609).
- Resource/link semantics: existing Resource sender/receiver, segmented transfer manager, link/request-response, Channel/Buffer, and ratchet paths (#610 and #615 evidence).
- Utilities/network workflows: existing `crates/apps/rns-tools`, `reticulumd`, SDK/RPC integration, and utility test/interop paths (#611).
- rngit: `crates/apps/rns-tools/src/bin/rngit_parts`, production service handlers, MessagePack fixtures, and network tests (#612 and #613).
- Native interfaces: `crates/libs/rns-transport/src/iface`, daemon adapters, and platform/feature tests (#614). Physical carrier/HIL execution is outside this goal.
- Release/evidence gates: existing `xtask`, `tools/scripts`, `.github/workflows`, `tools/interop`, `crates/libs/test-support`, and evidence manifests (#615).

Files to avoid:

- The dirty primary checkout `/home/pgiuseppe/Documents/LXMF-rs/artifacts/` and unrelated branch work.
- Historical `docs/status/v0.*` release ledgers and old release notes, except explicit historical links.
- PR #604's existing worktree and authored changes, except review/evidence work explicitly owned by #607.
- New network protocols, replacement daemons, broad rewrites, unreviewed reference repins, firmware flashing, destructive device management, and public-network load.

Source of truth: exact Python Reticulum revision frozen by #606, with repository pin metadata in `tools/interop/independent-implementations.toml` and all active mirrors checked by the release gate.

Read path: frozen source/release notes -> behavioral requirement/evidence rows -> explicit Rust owner and linked issue -> focused regression -> production runtime/utility path -> generated status/evidence -> exact-candidate CI/release/support report.

Write path: reference manifest and row status are written only by the #606 inventory/delta owners; implementation crates write behavior; daemon/RPC/CLI expose it; evidence scripts serialize observed results; roadmap/matrices are regenerated or updated from the same exact candidate.

Integration points: interface ingress/egress and IFAC, transport routing/discovery/links, Resource and Channel paths, daemon configuration/restart, RPC status, utility request/response and file side effects, rngit authorization/storage/wire schemas, native interface lifecycles, and Python mixed peers.

Migration/cutover: freeze the new target and row schema without rewriting old release claims; classify known gaps as partial/blocked; land implementation children with linked evidence; then atomically promote generated status and software release documentation for the exact candidate. Physical/support claims remain separate and do not gate this goal.

Acceptance evidence gate: this goal may close only after #606’s deliberate-failure gates, #607–#614’s linked implementation evidence, and #615’s exact-candidate software release gate are independently reviewed and green. #616’s physical/platform/client matrix is explicitly outside this goal and remains separately marked unverified.

## Execution Board

### 0. Plan and current-state audit

- Files: this goal package, current issue/child issue metadata, current `main` and PR #604 state.
- Output: reviewed plan, isolated branch, exact baseline SHA, and a requirement-to-evidence map.
- Verification: clean isolated worktree; live GitHub issue/PR state; no writes to the dirty primary checkout.
- Evidence: plan PRE verdict and baseline record.

### 1. #606 — Freeze reference and behavioral evidence contract (P0, first)

- Files: existing reference-pin manifest/checker, inventory generator/mapping/generated files, `lxmf-reference`, active roadmap/matrices/ledger, and focused checker tests.
- Output: exact 1.5.4 development target (or explicitly reviewed replacement); behavior rows covering protocol, state, configuration, persistence, private effects, interface families, utilities, and runtime integration; explicit statuses/evidence/owner/test/artifact fields; wildcard and fixed-count loopholes removed; unsupported and hardware-unverified states distinct; deliberate corrupt-fixture failures enforced by CI.
- Verification: exact source checkout assertions; pin-mirror checker; inventory regeneration/check; deliberate unmapped, stale-reference, malformed-summary, contradictory-status, and missing-evidence fixtures; generated Rust/status artifacts match exact inputs.
- Evidence: committed contract/delta report and raw command outputs tied to exact reference commits.
- Parallel safety: first; later implementation rows may proceed only against the frozen contract.

### 2. #607 — Review and integrate PR #604’s bounded increment (P0)

- Files: PR #604 branch/production paths only as required by review; existing BLE/HDLC/rngit regression/evidence files and child issue record.
- Output: blocking findings fixed, hosted CI/Verify/Independent interoperability pass on the actual merged head, and exact merge evidence recorded. Preserve the five BLE and six rngit reproductions, mobile idle/migration tests, eight HDLC vectors, canonical dotted-path/fail-closed tests, and node-side migration documentation.
- Verification: live review threads and exact-head check rollup; local focused tests are supplementary, not closure evidence.
- Evidence: merge SHA, review/check URLs, and child issue update.

### 3. #608 — IFAC production carrier ingress/egress (P0)

- Output: configured IFAC authentication is wired through actual carrier receive/transmit for applicable interfaces, with exact auth/framing/order/counter behavior, fail-closed errors, no plaintext downgrade, and mixed Python/Rust daemon evidence.
- Verification: failing daemon-level regressions, wrong/tampered/truncated credential cases, config/restart/reconfiguration tests, and two-process bidirectional announces/packets/proofs/links/Resources.

### 4. #609 — Transport, local-client announcements, shared-instance behavior (P0)

- Output: exact local-client timing and parent/child classification, with no accidental transit forwarding; preserve ordering, duplicate suppression, cached/scheduled announcements, policy, queues, persistence, expiry, and close semantics.
- Verification: deterministic clock/schedule tests plus mixed Python/Rust shared-instance and multi-hop production-path traces.

### 5. #610 — Resource collision/stream/mixed-peer behavior (P1)

- Output: deterministic collision regeneration, serving windows/indexes/rebinding, negotiated compression/chunking/metadata/size accounting, cancellation/loss/reorder recovery, bounded memory, and observable callback/error cleanup.
- Verification: collision/exhaustion fixtures, empty-to-50 MiB transfers, Python sender/receiver roles, and exact checksummed mixed-peer evidence.

### 6. #611 — Real network workflows for shipped utilities (P1)

- Output: each reference utility has an option/behavior matrix and production network path, including discovery/auth/link/request-response/Resource transfer, correct output/status/file side effects, and multi-process restart/cancellation/error behavior. Local-only operations remain local but are not promoted as remote parity.
- Verification: Rust↔Python utility roles, isolated roots, real Git fetch/push/bundle workflows, and negative-path transcripts.

### 7. #612 — rngit permissions, work operations, storage/wire schemas (P1)

- Output: executable resolver behavior, configured-access/permission inheritance/revocation/blocked identities, atomic updates, all work operations and authorization shapes, binary-safe MessagePack compatibility, and dotted-path migration semantics.
- Verification: production service handlers, Python-produced and Rust-produced requests/data, restart/round-trip persistence, denied/malformed/error cases, and preserved #604 regressions.

### 8. #613 — rngit NomadNet pages/media/link cleanup (P1)

- Output: reachable Reticulum page/file endpoints with auth/no-ident behavior, `/media` validation/ref/blob/filename semantics, safe optional WebP conversion, raw fallback, and link-scoped cleanup.
- Verification: pinned Python/NomadNet-compatible client, deterministic rendered/content/metadata fixtures, and bounded process/file cleanup evidence.

### 9. #614 — Native interface runtimes and Windows BLE parity (P1)

- Output: reference interface-family inventory, production config-to-carrier-to-status-to-teardown traces, Windows paired-address behavior, reconnect/cancellation/EOF/idle/timeout cleanup, AutoInterface integration, and explicit unsupported-kind errors.
- Verification: loopback/fake-SAM/PTY deterministic faults and software feature checks. Hosted/native platform combinations and #616 physical evidence are outside this goal.

### 10. #615 — Differential conformance and exact-candidate software release gate (P0)

- Output: executable Python↔Rust, all-Rust, multi-hop, shared-daemon, restart, loss/reorder/duplication, provenance, skip/missing-runner, and evidence-publication gates covering every #606 row; current parity/status/release docs promote only after the exact candidate passes.
- Verification: full workspace format/Clippy/tests, boundaries/architecture/module-size checks, issue-369 scanner, docs/dependency/license/security/release checks, exact commit provenance, and hosted exact-head workflows.

### 11. #616 — Physical carriers, platforms, clients, and network soak (explicitly out of scope)

- Output: none in this software goal. The existing #614/#616 evidence remains visible as hardware-unverified and is not promoted by the #615 software gate.
- Verification: no physical, native-platform, external-client, public-network, or soak execution is authorized or required here. Those rows remain a separate follow-up scope.

## Non-goals

- Do not publish releases, merge/approve automatically, force-push, buy hardware, flash firmware, erase ROMs, or perform unauthorized public-network load.
- Do not rewrite historical release claims or close #605 from callable counts, local-only tests, mocks, a single third-party client, or a green but skipped evidence lane.
- Do not duplicate #601 or PR #603; link and reuse them at their existing ownership boundaries.

## Risk if wrong

An unreviewed reference repin or wildcard inventory can make green CI prove the wrong behavior. Incorrect IFAC ordering can downgrade authenticated links; transport/path changes can suppress requesters or route local traffic transitively; Resource/rngit changes can lose data or broaden access; and simulated interface/client evidence can falsely certify physical interoperability.
