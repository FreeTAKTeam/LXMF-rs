# Open issue implementation plan: items 1–6

**Intent:** Execute the six approved work items below, each in a separate pull request, with review, CI repair and merge before the next item.
**Current Behavior:** Useful production implementations and bounded reference evidence coexist with missing behavior, unverified acceptance cells and outdated issue descriptions.
**Expected Outcome:** Reconciled acceptance, enforced automatic CI, LXMF 1.2.0 compatibility, completed scoped transport and utility software behavior, and the three maintenance fixes.
**Target-Perspective Output:** Maintainers receive six reviewed and passing merged PRs; SDK/ZeroMQ callers and reference CLI peers observe the specified behavior through production paths.
**Truth Owner:** `docs/status/current-roadmap.md` owns posture and execution order; the two parity matrices own row status. Immutable Python references define compatibility. This package records execution scope and evidence, not a competing parity inventory.
**Contract Boundary:** Preserve core/transport/application ownership, existing SDK v2 and typed ZeroMQ paths, wire semantics, authorization, public API compatibility and the repository's Rust policies.
**Cutover:** Extend existing owners. Introduce no second daemon, SDK, protocol, CI orchestrator or independent source of policy.
**Displaced Path:** Automatic CI replaces label-dependent required validation; correctly spelled buffer APIs retain forwarding aliases; any allocation API change retains existing callers. Other compatibility paths remain unless their replacement is proven.
**Value Density:** Reuse implemented behavior and existing tests; spend effort on confirmed differences and missing acceptance.
**Acceptance Evidence:** Requirement-level production tests, exact-reference traces where applicable, review findings and fixes, exact-head terminal hosted checks, and verified GitHub merge commits for each item.
**Evidence Lane:** Software and hosted CI. Existing physical evidence is historical context only; no new hardware, public-network, client-UI or release-publication claim is part of this goal.
**Kill Criteria:** Do not merge with actionable review findings, failing/missing applicable CI, unimplemented criteria disguised as documentation, or tests that bypass the production boundary. Do not weaken authentication, performance budgets or protocol timing to obtain green checks.
**Architecture Slice:** Existing workflow/xtask policy; LXMF wire/delivery plus daemon scheduling; Reticulum transport/discovery/lifecycle; rns-tools services and CLIs; status documents.
**Plan Review Gate:** PRE review before behavior changes; correctness, maintainability and POST alignment review per PR. Hardware and deferred capabilities cannot be used to shrink an in-scope software requirement.

## Authority and baseline

The user approved items 1–6 of the assessment on 2026-10-17, explicitly including
creation, review, CI fixes and merge of one PR per item. Items 7–9 (new BLE/interface
work, HIL commissioning, mutation/no_std/optical work) are not implementation tasks
in this goal. Item 1 still reconciles all 22 open issues to preserve those boundaries.

The starting source is `a8e500e88610210582e8dd8869df8b510e1e75fc`. Re-read live
main and issue state at each item; earlier evidence is not a fresh test result.
The canonical reference manifest remains
[`tools/interop/independent-implementations.toml`](../../../tools/interop/independent-implementations.toml).

| Reference | Immutable revision | Role |
| --- | --- | --- |
| RNS 1.5.2 | `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6` | Tested release baseline; do not silently promote it. |
| RNS 1.5.4-dev | `99de23c040d507e3fefca19e87b182302902725d` | Frozen #608–#614 behavior contract. |
| RNS 1.5.5 | `7f2b3b9b524c9386316379af1313b43a5e4f7a5d` | #649 feature-update acceptance. |
| Existing LXMF reference | `727830cefda83d9c6e3982b48675425f3f988f9c` | Existing Verify baseline. |
| LXMF 1.2.0 | `c3ff2d6dc2f256daab896dadc044dd5a913ecbb7` | #648 acceptance, in addition to baseline regression. |

## Ordered task board

The main agent implements sequentially. Explorer/reviewer/maintainer/verifier
roles may inspect read-only. Do not begin the next item's implementation until
the preceding PR is merged. Multiple commits within one item are encouraged for
test-first and structural/behavioral separation; the user requested six PRs,
not a separate PR per issue or subtask.

### 1. Reconcile acceptance

- Owner: documentation/status; input: all 22 live issue bodies/discussions and current source/evidence.
- Allowed: this package and a roadmap link; forbidden: runtime, workflow and GitHub policy mutations.
- Output: [ACCEPTANCE.md](ACCEPTANCE.md), this plan, execution handoff and progress record.
- Evidence: exhaustive issue-number reconciliation, actual source paths, explicit inherited-versus-new evidence, local link/diff checks, independent review and hosted CI.
- Depends on: none. Parallel implementation: no.
- Completion does not close any implementation issue or promote parity cells.

### 2. Make validation enforceable (#618, #617, lightweight #244)

- Owner: existing `.github/workflows/ci.yml`, `ci-full.yml`, xtask CI stages, repository-native policy tooling.
- Add automatic audit/deny on PRs and main plus weekly scanning; preserve existing exception policy. Reuse tests already running in Verify rather than duplicate them.
- Add a deterministic changed-path/Cargo-dependent selector; manifest/lock/shared fixture/workflow changes and unavailable or unclassified diffs broaden the software set. Cover renamed/deleted paths and executable contract docs.
- Add an always-reporting final check: required failed, cancelled, missing or unexpectedly skipped jobs fail. A deliberate inapplicable job is distinct.
- Add one small native branch-protection configuration with idempotent read-only check and explicit apply. Require PRs and stable applicable checks, block force-push/deletion, require no human approvals. Preserve stronger existing protection and recovery access; never broaden bypass rights silently.
- Cutover: label may widen work but is not needed for correctness. Remove/reuse duplicate required jobs. No privileged execution of fork code.
- Acceptance: selector and gate failure tests; actual fork/docs/code PR trigger evidence where feasible; observed runtime; live settings readback and failing-check protection proof. Missing external test contexts remain unproven, not simulated successes.
- Apply settings only after the check exists and reports reliably; then verify policy and merge eligibility. The user's approved implementation includes this explicit apply, not unrelated organization changes.
- #244 disposition: implement the lightweight subset and document exclusions; no mandatory reviewer tiers, size approvals, new meetings or recurring bot reports.
- Depends on: 1. Parallel implementation: no.

### 3. LXMF 1.2.0 compatibility (#648)

- Owners: `crates/libs/lxmf-core/src/{constants,announce}.rs`, `src/message/delivery.rs`, existing router/runtime and `reticulumd` delivery/announce workers. SDK v2/ZeroMQ remains the integration path.
- Four test-first slices: serialized opportunistic payload boundary; honest implementation/version metadata; slow/normal-medium scheduling and announce wakeups; serialized outbound processing and recovery after callback/send failure.
- Acceptance: exact-reference size/mode decisions below/at/above the limit including title/fields/stamp overhead; old/new/malformed metadata; absent/recovered paths, bounded request/retry counts, exhaustion and no duplicated sends/terminal outcomes; later successful delivery after a failed send/callback.
- Run Python↔Rust direct, opportunistic and propagated traces at the LXMF 1.2.0 pin and preserve baseline regressions. Record accumulated old-pin differences rather than calling every delta new in 1.2.0.
- No change to canonical RNS baseline or physical low-bandwidth claims.
- Depends on: 2. Parallel implementation: no.

### 4. Transport and residual RNS 1.5.5 (#608, #609, #649)

- Owners: `crates/libs/rns-transport/src/{iface_parts,transport,discovery}`, existing daemon bootstrap/hot-apply/configuration, RPC management and Link request paths.
- IFAC: enumerate applicable family × startup/error × reconfiguration × child inheritance × restart cells; fill uncovered production cases, keeping shared-instance exceptions and fail-closed rollback.
- Transport: complete packet/proof duplicate classes and packet-hash capacity/rotation; repeated bidirectional DIRECT/Resource delivery through deeper relay replacement; remaining routing/discovery/shared-instance cells.
- Predeclare scenario/repetition counts and protocol deadlines before runs. Use readiness/fault barriers, retain every result, and fix causes rather than accepting retries-until-green.
- RNS 1.5.5: wire discovery planning into daemon lifecycle with default qualification, collision-safe names, persisted metadata, explicit unverified override and manual-detach suppression. Cover supported platform carrier selection, cleanup and policy inheritance. Implement missing request-receipt states with one terminal outcome before/after delivery acknowledgement, and live parent-policy propagation to children.
- Reuse completed named management/discovery/IFAC work. Rendering overlaps belong to item 6; keep #649 traceability there.
- Acceptance: production daemon/SDK and exact-reference mixed-peer traces, credential-redacted status, no authentication bypass or unrelated traffic loss, correct cancellation/teardown and post-restart delivery.
- Depends on: 3. Parallel implementation: no; shared transport owners must be integrated sequentially.

### 5. Bounded maintenance (#523, #521, #522)

- Owners: `rns-transport/src/packet.rs`, `iface/tcp_client.rs`, `iface_parts/ifac_wire.rs`, `rns-core/src/{ratchets.rs,crypt/fernet.rs}` and existing tests/benchmarks.
- Add correctly spelled acquisition methods; preserve old methods as forwarding aliases and document deprecation/removal conditions. Verify both APIs and callers.
- Complete RNG audit through constructor/verify/decrypt; use a fail-on-use RNG regression where appropriate, retaining authentication and encryption randomness. Do not claim a vulnerability from an unused RNG value.
- Measure the real production IFAC encode path. Existing wire-buffer reuse is not the test-only raw-buffer path; introduce narrowly scoped reusable encoding only where measurements justify it, retaining allocating public wrappers for callers.
- Acceptance: source/API compatibility, cryptographic vectors and malformed-token rejection, production allocation measurements, correct short/long/escaped frames, no stale byte exposure, unchanged IFAC/framing order.
- Depends on: 4. Parallel implementation: no (even though some files are disjoint, follow the requested numbered order).

### 6. Utilities and rngit (#611, #612, #613; #649 rendering overlap)

- Owners: `crates/apps/rns-tools/src/bin` and `rngit_parts`, existing daemon config/registration/dispatch, transport Link/Channel/Resource owners and existing interop fixtures.
- Start with `rnstatus -R`: distinct `rnstransport.remote.management` destination, enable gate, identity ACL, exact restricted `/status` response, CLI request/timeout/status behavior. Never reuse propagation control or expose broad daemon state.
- Finish every applicable software option/workflow in the existing frozen utility inventory: rncp, rnpath, rnprobe, rnstatus, rnsd, rnid, rnir, rnodeconf, rnpkg, rnsh, rnx and rngit. Verify actual reference behavior before treating a configuration-only reference as a missing network implementation. Retain existing Rust conveniences without advertising them as equivalent.
- Complete rngit operation × identity role × repository/document scope × missing/malformed/denied/success/side-effect cases, including persistence, restart, concurrency and filesystem faults. Preserve explicit safety divergences (bounded executable resolvers and no ID-zero authorization bypass).
- Complete rendering/highlighting/diffs/signed-work/template details and residual converter/cancellation/filesystem cases. Pagination already exists; it needs exact-reference evidence rather than replacement. Prove cleanup after a real silent remote-process exit, real declared converter behavior including avconv, and rendered/image-corpus conformance.
- Acceptance: separate-process Python↔Rust client/service paths in both supported directions, binary-safe files and real Git fetch/push/bundles, all configured initial-branch variants, negative auth/disabled remote management, accurate stdout/stderr/exit/side effects. Preserve documented upstream cancellation defects rather than misreporting them as successful delivery.
- Hardware, public-network and client-UI evidence remain outside this software goal; this does not exclude software simulation of slow links or platform behavior. Do not call partial utility work complete because rnstatus alone passes.
- Depends on: 5. Parallel implementation: no.

## Common validation and integration

Start with meaningful focused regressions, then affected package/integration tests
and exact-pin interop. Run formatting, dependency boundaries, module-size checks
and architecture checks as applicable. Behavior PRs require workspace Clippy/tests
and release-facing changes require `cargo xtask release-check`; diagnostics edits
also run `cargo test -p reticulumd --test code_quality_issue_369`.

Each PR must be attached to the chat, reviewed against the plan and connected
code, corrected, and observed green on its exact head. Read failed job logs before
editing; preserve infrastructure failures as failures. Do not bypass required
checks to merge. Recheck head/base, merge, then verify GitHub's merged state and
commit ancestry. Rebase the next item on that merge. No release publication is
authorized by this goal.

Record the PR URLs, heads, review outcomes, test commands/results and merge SHAs
in [PROGRESS.md](PROGRESS.md). A row is done only after all evidence and merge
requirements are satisfied. Final completion requires all six rows, not merely
six created PRs.
