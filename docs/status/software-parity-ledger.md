# Software Parity Ledger

Last reassessed: 2026-06-29

This ledger turns the current partial Reticulum and LXMF rows into
implementation-ready software work. It is scoped to software, protocol, and
runtime parity only. Hardware/HIL evidence, prepared-host device matrices, and
external-client compatibility claims are explicitly deferred and must not be
used to promote rows in this ledger.

Repository-level posture remains in `docs/status/current-roadmap.md`. Row-level
status remains in `docs/status/reticulum-parity-matrix.md` and
`docs/status/lxmf-parity-matrix.md`.

## Scope Boundary

In scope:

- Deterministic unit and integration tests in this workspace.
- Local software smokes using loopback sockets, fake serial PTYs, fake TCP
  peers, fake SAM routers, subprocesses, and daemon/RPC status snapshots.
- Pinned Python-reference protocol/runtime interop that does not depend on
  external client apps or attached hardware.
- Documentation updates that map new evidence back to the parity matrices and
  roadmap.

Out of scope for this ledger:

- RNode, RNodeMulti, Weave, VR-N76, BLE, serial-radio, or other attached-device
  HIL proof.
- Prepared-host hardware matrices and nightly HIL artifact manifests.
- Sideband, MeshChatX, Columba, or other external-client release evidence.
- Broad production soak claims for public I2P, radio deployments, or mixed
  physical networks.

Deferred evidence can be referenced only to prevent accidental scope expansion.
It must not be treated as acceptance evidence for a software ledger row.

## Row Mapping

| Ledger row | Matrix row | Owner | Software parity target | In-scope evidence | Deferred evidence |
| --- | --- | --- | --- | --- | --- |
| RNS-RUNTIME-RETICULUM | `RNS/Reticulum.py` | RNS runtime agent | Close daemon/runtime gaps that do not depend on physical interfaces: config reload semantics, runtime mutation behavior, persistence boundaries, shared-instance behavior, graceful shutdown, RPC state visibility, and failure reporting. | Focused `reticulumd` integration tests; daemon/RPC status snapshot tests; local shared-instance TCP/Unix smokes; pinned Python runtime interop when it exercises daemon behavior directly. | Device startup matrices, platform-specific prepared-host runs, external-client runtime validation. |
| RNS-TRANSPORT-POLICY | `RNS/Transport.py` | RNS transport agent | Finish announce/path/link-routing policy differences beyond the existing cached path-response, roaming same-interface suppression, restored-cache, tunnel-cache, and shared-instance persistence slices. | Transport unit tests; daemon path/announce integration tests; local loopback Reticulum traffic tests; pinned Python announce/path interop scenarios. | Multi-device mesh HIL, public network soak, client-app traffic evidence. |
| RNS-DISCOVERY | `RNS/Discovery.py` | RNS discovery agent | Expand public bootstrap, announce, AutoInterface discovery, and peer lifecycle behavior that can be proven without physical carriers. | AutoInterface state-machine tests; Linux namespace or fake-carrier smokes where available; daemon `_runtime.auto` status tests; Python-reference discovery tests without hardware dependency. | Real Wi-Fi/Ethernet churn matrices, radio discovery HIL, production network observations. |
| RNS-RESOLVER | `RNS/Resolver.py` | RNS resolver agent | Complete resolver/bootstrap semantics beyond cache lookup, restored path-table identity lookup, tunnel restored-cache lookup, and shared-instance path-table suppression. | Resolver/cache unit tests; path-table persistence tests; daemon bootstrap/status tests; Python-reference resolver edge-case fixtures. | Public bootstrap fleet evidence, long-running resolver soak, external-client resolver behavior. |
| RNS-UTILITIES | `RNS/Utilities/*` | RNS tools agent | Fill only product-needed software utility behavior for `rnx`, `rnsd`, `rnstatus-rs`, `rnodeconf-rs`, `rnpath-rs`, and any future tool surfaces, without claiming full retired-tool parity unless implemented and tested. | CLI tests; mock-RPC tests; local daemon smokes; JSON/human output golden checks; error-code and argument compatibility tests; daemon-backed `request_path` status/timeout and path metadata tests; local non-neighbor mesh `rnpath-rs` smoke through `rnx rnpath-smoke`. | Full Python `rnodeconf` hardware workflows, retired utility replacements with no product demand, and client-app operator workflows. |
| RNS-CRNS | `CRNS/*` | RNS tools agent | Decide and implement selected CRNS command workflows needed by current products; keep unsupported Python command ecosystem gaps explicit. | CLI contract tests; mock transport/RPC fixtures; local end-to-end command smokes where command behavior is implemented. | Broad Python command ecosystem reproduction, physical-network CRNS validation, external operator-client evidence. |
| LXMF-ROUTER | `LXMF/LXMRouter.py` | LXMF router agent | Finish non-propagation router convenience behavior that affects supported SDK/daemon flows while preserving the completed propagation-router lifecycle baseline. | Focused daemon/RPC tests; typed ZeroMQ SDK tests; local direct/opportunistic/paper/propagated mode tests; pinned Python LXMD remote lifecycle interop when protocol behavior is touched. | Sideband, MeshChatX, Columba compatibility claims; manual client workflows; broad live store-and-forward soak. |
| LXMF-HANDLERS | `LXMF/Handlers.py` | LXMF handlers agent | Improve delivery, announce, propagation app-data, receipt, negative/drop, and router-coupled side-effect observability. | Daemon bridge tests; handler callback tests; receipt and drop-state regressions; structured event/status assertions; pinned Python handler behavior fixtures. | External-client callback behavior, live mobile notification flows, hardware-dependent delivery paths. |

## Work Packet Rules

Every implementation packet derived from this ledger must:

- Name exactly one ledger row as the primary row and list any secondary rows.
- Start from an existing matrix residual gap or add a precise residual gap
  before implementation begins.
- Add or tighten a focused regression before or with the behavior change.
- Keep workspace dependency boundaries intact and run
  `tools/scripts/check-boundaries.sh` when a new local dependency edge is
  proposed.
- Update the affected parity matrix row and this ledger when the evidence or
  residual gap changes.
- Avoid hardware, HIL, external-client, or broad production-soak evidence as a
  row-closing criterion.

## Acceptance Checklist

A PR that claims progress against this ledger is acceptable only when:

- The PR description names the ledger row, affected matrix row, and exact
  software behavior changed.
- Tests or software smokes prove the new behavior without attached hardware or
  external-client apps.
- Any pinned Python-reference evidence names the specific scenario and does not
  imply unrelated row completion.
- Deferred hardware/client evidence remains documented as deferred when the
  matrix row still depends on it.
- `docs/status/reticulum-parity-matrix.md` or
  `docs/status/lxmf-parity-matrix.md` is updated when row status, baseline, or
  residual gap text changes.
- `docs/status/current-roadmap.md` is updated when the repository-level
  posture, blocker list, or execution order changes.
- Validation is focused first and broadened according to risk, with command
  output or artifact paths captured in the PR.

Suggested validation menu:

- `cargo fmt --all -- --check` for code changes.
- `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`
  for shared behavior changes.
- `cargo test --workspace --tests` for broad runtime or protocol changes.
- Package-specific tests for narrow rows, such as `cargo test -p reticulumd`,
  `cargo test -p reticulum-rs-transport`, `cargo test -p reticulum-rs-rpc`,
  `cargo test -p rns-tools`, or `cargo test -p lxmf-wire`.
- Relevant local software smoke scripts when a row touches daemon, interface,
  utility, or LXMF delivery behavior.

## Deferred Evidence Register

The following evidence categories are intentionally deferred from this software
ledger and should be tracked in roadmap or matrix residual gaps instead:

| Deferred category | Applies to | Reason |
| --- | --- | --- |
| Prepared-host RNode serial/TCP/BLE hardware matrix | `RNS/Interfaces/*`, `RNS/Utilities/*` | Required for hardware interface parity, not software protocol/runtime parity. |
| RNodeMulti single-device and broad device/firmware validation | `RNS/Interfaces/*`, `RNS/Utilities/*` | Validates attached vport hardware behavior outside this ledger. |
| Weave connected serial hardware and display/status combinations | `RNS/Interfaces/*`, `RNS/Utilities/*` | Depends on physical device and operator workflow evidence. |
| VR-N76 BLE readiness/write/reconnect evidence | `RNS/Interfaces/*`, `RNS/Utilities/*` | BLE hardware behavior is outside software-only acceptance. |
| Public I2P peer-set and long-running production soak | `RNS/Interfaces/*`, `RNS/Transport.py` | Useful operational confidence, but not required for local software parity rows. |
| Sideband, MeshChatX, Columba, and other external clients | `LXMF/LXMRouter.py`, `LXMF/Handlers.py` | Client-specific compatibility claims require separate release evidence. |

## Agent Ownership Notes

- RNS runtime agent owns daemon lifecycle, configuration, persistence, runtime
  mutation, shared-instance, and RPC state work.
- RNS transport agent owns packet routing, path/announce policy, link/resource
  routing behavior, duplicate suppression, pacing, and transport-visible
  errors.
- RNS discovery agent owns bootstrap/discovery and AutoInterface software
  discovery behavior.
- RNS resolver agent owns resolver/cache/bootstrap lookup behavior.
- RNS tools agent owns `rns-tools`, `rnstatus-rs`, `rnodeconf-rs`, `rnx`,
  `rnsd`, and selected CRNS command workflows.
- LXMF router agent owns daemon/RPC/SDK router behavior, outbound policies,
  direct/opportunistic/propagated/paper routing, selected propagation nodes,
  persistence, retries, and cancellation.
- LXMF handlers agent owns delivery, receipt, announce, propagation app-data,
  inbound bridge, negative/drop, and event observability behavior.
