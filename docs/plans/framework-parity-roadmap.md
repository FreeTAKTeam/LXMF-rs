# Framework Parity Roadmap

Status: historical planning note

This roadmap is retained as planning context for parity work. The release gate
and current status tracking live in:

- `docs/status/lxmf-parity-matrix.md`
- `docs/status/reticulum-parity-matrix.md`
- `docs/runbooks/release-readiness.md`

This plan groups the currently known Reticulum and LXMF framework gaps into delivery waves. The intent is to finish behavior-level parity in the active workspace, not to expand RPC surface area without matching protocol/runtime behavior.

Historical naming note: this roadmap predates the crates.io rename pass. Where
it refers to `lxmf-core` and `rns-rpc`, the current published package names are
`lxmf-wire` and `reticulum-rs-rpc`.

## Wave 1: Correctness And Core LXMF Behavior

Focus:

- Preserve live daemon outbound delivery-mode handling and keep extending the
  propagation-specific behavior behind it.
- Replace simplified paper-command behavior with real `lxmf-wire` paper encode/decode flow.
- Move stamp and ticket behavior out of legacy-style RPC placeholders into active-workspace implementations.
- Tighten the parity docs and test suite around what is actually implemented.

Deliverables:

- `reticulumd` delivery bridge respects `direct`, `opportunistic`, `propagated`, and `paper`.
- Active stamp generation/validation and ticket derivation APIs exist outside migration-only crates.
- SDK paper encode/decode uses canonical wire helpers from `lxmf-wire`.
- Tests fail if the daemon silently falls back to a different delivery mode than requested.

Exit criteria:

- Message mode selection is observable and tested end to end.
- Paper workflow output is generated from real wire helpers.
- Stamp/ticket parity items move from `partial`/`not-started` to `done` or narrow, explicit `partial`.

## Wave 2: Router, Peer, And Propagation Parity

Focus:

- Close the gap between the Python `LXMRouter` and the Rust daemon/router stack.
- Replace local placeholder propagation storage with real propagation-node behavior.
- Implement real peer semantics instead of peer-record bookkeeping only.

Deliverables:

- Real outbound propagation-node selection and fetch/ingest workflow.
- Peer sync, peer removal, and peering metadata tied to actual router behavior.
- Router state transitions and side effects aligned with Python LXMF expectations.
- Interop-focused tests that exercise Python-compatible propagation and peer scenarios.

Exit criteria:

- `LXMPeer.py` and `LXMRouter.py` can be marked `done` or have only narrow, documented residual gaps.
- Propagation RPC methods are backed by real router behavior rather than a blob store.
- Peer-related parity items stop depending on SDK event translation alone.

## Wave 3: Reticulum Runtime And Interface Breadth

Focus:

- Close the gap between Rust `reticulumd` and Python `Reticulum` daemon/runtime behavior.
- Expand supported interface families beyond the current subset.
- Improve daemon config/runtime parity and startup semantics.

Deliverables:

- A clear split between supported interfaces now and targeted interface additions.
- Implement or explicitly defer missing interface families: Auto, AX.25, Backbone, I2P, KISS, Local, Pipe, RNode, and Weave.
- Better runtime parity for discovery/bootstrap and interface lifecycle management.
- Tests and docs for daemon startup policy, interface validation, and runtime behavior.

Exit criteria:

- `RNS/Reticulum.py`, `RNS/Transport.py`, and `RNS/Interfaces/*` all move materially closer to `done`.
- The daemon supports more than the current narrow subset of interface kinds.
- Discovery/bootstrap behavior is no longer substantially narrower than the Python reference.

## Wave 4: Utility And Operator Surface Parity

Focus:

- Implement retired Python-style utility binaries in `rns-tools` as real
  utilities before adding them back to the release surface.
- Bring the operator experience closer to Python Reticulum/LXMF tooling.

Deliverables:

- Real implementations for `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`,
  `rnpkg`, `rnprobe`, and `rnstatus`; `rnsd` remains the active
  `reticulumd` delegate.
- Clear overlap boundaries between `reticulumd`, `lxmf-cli`, and `rns-tools`.
- Operator docs that map Rust commands to Python command equivalents.

Exit criteria:

- `RNS/Utilities/*` is no longer marked `partial` because of stubs.
- The utility suite is usable without falling back to Python tools.

## Wave 5: Cross-Implementation Interop And Release Gate

Focus:

- Prove parity against the Python reference behavior, not only against Rust-local contracts.
- Lock parity claims behind repeatable interop gates.

Deliverables:

- Live cross-implementation tests against local checkouts of `Reticulum` and `LXMF`.
- Fixture and behavioral gates for paper transport, propagation, peer sync, utilities, and interfaces.
- Updated parity matrices that only mark `done` when backed by active interop evidence.

Exit criteria:

- `interop.python_live_gate` stays `done` with pinned Reticulum/LXMF commits and
  live ignored tests enabled in CI.
- Parity documents are release-grade rather than aspirational.
- Future regressions are caught by active interop tests instead of manual review.

## Suggested Execution Order

1. Wave 1
2. Wave 2
3. Wave 3
4. Wave 4
5. Wave 5

Wave 1 landed its baseline delivery-mode handling, but propagation-specific
router behavior still needs to come before broader parity claims. Wave 5 should
stay last because it depends on the earlier waves being behaviorally meaningful
enough to test against the Python reference.
