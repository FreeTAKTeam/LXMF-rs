# Software Parity Ledger

Last reassessed: 2026-07-06

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

## v0.7.0 SDK-First Ledger Boundary

v0.7.0 ledger work is SDK-first for REM/RCH-facing software behavior. Work
should improve the existing `lxmf-sdk` and `ZmqPipelineBackendClient` in place;
it must not create a compatibility layer, adapter, shim, or new parallel SDK
surface.

| Evidence slice | In this ledger | Not accepted as |
| --- | --- | --- |
| LXMF send/receive | Typed `ZmqPipelineBackendClient` send, batch-send, status, cancellation, paper, history, conversation, propagation-control, inbound event, receipt, and drop-observability tests or named pinned Python-reference scenarios, plus native `Client` app-domain helpers that route those flows through the existing SDK surface. | A broad external-client compatibility claim. |
| Carrier attach/announce software | Software-controlled attach and announce/path evidence such as LocalInterface TCP/Unix, AutoInterface carrier runtime, I2P fake-SAM or bounded real-SAM pair evidence, local loopback TCP/UDP/Backbone/Pipe/KISS smokes, and daemon/RPC runtime snapshots. | Full physical-carrier or public-network parity. |
| Optional HIL | Prepared-host RNode, RNodeMulti, Weave, VR-N76, BLE, serial-radio, and broader device/firmware matrices. | A software-ledger row-closing requirement for the v0.7.0 SDK-first release. |

## Row Mapping

| Ledger row | Matrix row | Owner | Software parity target | In-scope evidence | Deferred evidence |
| --- | --- | --- | --- | --- | --- |
| RNS-RUNTIME-RETICULUM | `RNS/Reticulum.py` | RNS runtime agent | Close daemon/runtime gaps that do not depend on physical interfaces: config reload semantics, runtime mutation behavior, persistence boundaries, shared-instance behavior, graceful shutdown, RPC state visibility, and failure reporting. | Focused `reticulumd` integration tests, including `set_interfaces`/`reload_config` hot-apply policy for TCP clients, explicit loopback TCP server listeners including `localhost`, and explicit UDP listener, peer, and multicast-bind records while treating device-bound, non-local, and broader TCP server listener shapes plus device-bound, partial-target, out-of-range-target, and multicast-forward UDP shapes as restart-required or invalid and rejecting duplicate TCP server or UDP binds before mutation; daemon/RPC status snapshot tests including unified legacy `status`/`daemon_status_ex` runtime snapshot fields, Reticulum path-table restore `ok`/`error` metadata, hot-applied explicit TCP server runtime iface plus live `_runtime.tcp.listener_status`, and hot-applied explicit UDP runtime iface plus live `_runtime.udp.status` counter refresh; local shared-instance TCP/Unix smokes; pinned Python runtime interop when it exercises daemon behavior directly. | Device startup matrices, platform-specific prepared-host runs, external-client runtime validation. |
| RNS-TRANSPORT-POLICY | `RNS/Transport.py` | RNS transport agent | Finish announce/path/link-routing policy differences beyond the existing cached path-response, held-announce path-response ordering, unknown discovery request response, source-interface recursive path-request announce pacing, bounded path-request duplicate/throttle scoping, unknown discovery interface-mode gating, random-blob announce freshness/path replacement, passed-on announce rebroadcast completion, announce rebroadcast mode policy, never-activated outbound-link rediscovery, routed link-table unresponsive-path rediscovery/replacement, intermediate-hop configured software `LINKREQUEST` MTU signalling rewrite, roaming same-interface suppression and response grace, restored-cache, tunnel-cache, shared-instance persistence, and scoped daemon path-request slices. | Transport unit tests for cached path responses, held ordinary announce release after `PATH_RESPONSE`, deterministic same-destination `PATH_RESPONSE` precedence over due ordinary announce plus later ordinary-announce release, passed-on rebroadcast completion after a local retry, handler-boundary and local transport-policy announce rebroadcast mode-policy evidence from learned next-hop interface mode, unknown-announce ingress limiting evidence for per-interface holding and lowest-hop release, matching announces answering waiting unknown-path discovery requesters and releasing requester-interface recursive discovery capacity, source-interface recursive path-request announce pacing gates, inbound duplicate path-request scoping by requester/tag/ingress interface plus expiration, local path-response duplicate/throttle scoping by requester/tag/egress interface, recursive request cap and queue scoping by source interface, Python `DISCOVER_PATHS_FOR` unknown discovery interface-mode gating, random-blob stale/fresh known-path replacement including unresponsive-path higher-hop replacement, never-activated outbound-link stale-path expiry plus throttled rediscovery, routed link-table proof-timeout rediscovery that blocks the ingress interface, and intermediate-hop configured software `LINKREQUEST` MTU signalling rewrites that preserve mode bits, clamp to software ingress/next-hop interface MTU, preserve Python-default 500-byte signalling, and leave un-signalled requests unmodified; harness-dispatchable local transport-policy evidence for scoped path-request dispatch, known-path response ordering, roaming same-interface known-path response suppression, roaming different-interface path-response grace, announce rebroadcast interface-mode policy, unknown-announce ingress policy, and intermediate-hop `LINKREQUEST` MTU signalling policy; scoped path-request RPC/bridge tests, including selected-interface dispatch, known-path scoped refresh, and no-match failure reporting; local loopback Reticulum traffic tests, including `rnx rnpath-smoke` reissuing a discovered non-neighbor path lookup as scoped/tagged `rnpath-rs --on-iface --tag-hex`; pinned Python announce/path interop scenarios, including Rust daemon `request_path` and `rnpath-rs --json` resolving a Python delivery destination over loopback TCP, a pinned Python route case that reissues scoped/tagged `rnpath-rs --on-iface --tag-hex` over the learned interface and asserts daemon result metadata, plus Python `RNS.Transport.request_path()` resolving a Rust delivery destination over loopback TCP. | Multi-device mesh HIL, public network soak, client-app traffic evidence. |
| RNS-DISCOVERY | `RNS/Discovery.py` | RNS discovery agent | Expand public bootstrap, announce, AutoInterface discovery, and peer lifecycle behavior that can be proven without physical carriers. | AutoInterface state-machine tests, including Python-style final-init gating for daemon discovery and peer-data datagrams plus Python-style multicast echo freshness seeding when adopted interfaces are added at runtime; daemon `_runtime.auto.carrier_runtime` tests for expired-peer, reverse-announce, missing-initial-echo, carrier-event, post-job peer-count, and peer-data admitted/delivered/decode-failed/RX-closed visibility; `tools/scripts/auto-interface-software-smoke.sh` report evidence with `evidence_scope = "software_auto_interface_runtime"`; Linux namespace or fake-carrier smokes where available; Python-reference discovery tests without hardware dependency. | Real Wi-Fi/Ethernet churn matrices, radio discovery HIL, production network observations. |
| RNS-RESOLVER | `RNS/Resolver.py` | RNS resolver agent | Complete resolver/bootstrap semantics beyond cache lookup, restored path-table identity lookup, cacheless path save filtering, Python-format stale path-table row suppression, active/tunnel path-table missing-cache skip behavior, per-entry active/tunnel malformed or destination-mismatched cached announce skip behavior, tunnel restored-cache lookup, shared-instance path-table suppression, tunnel restored-path random-blob freshness, and daemon RPC visibility for restored Python-format path-cache material. | Resolver/cache unit tests; path-table persistence tests including active/tunnel restore with missing per-entry cached announce material and active/tunnel restore with malformed or destination-mismatched per-entry cached announce material; tunnel restored-path random-blob window/freshness tests; daemon bootstrap/status tests for restored `destination_table`/announce-cache material through `path_status`, already-known `request_path`, persisted announce-identity lookup, missing active/tunnel cached-announce row skips without restore errors or identity resurrection, and `_runtime.reticulum.path_table_restore` success/failure status; Python-reference resolver edge-case fixtures. | Public bootstrap fleet evidence, long-running resolver soak, external-client resolver behavior. |
| RNS-UTILITIES | `RNS/Utilities/*` | RNS tools agent | Fill only product-needed software utility behavior for `rnx`, `rnsd`, `rnstatus-rs`, `rnodeconf-rs`, `rnpath-rs`, and any future tool surfaces, without claiming full retired-tool parity unless implemented and tested. | CLI tests, including `rnsd` delegation environment override, forwarded arguments/output, and delegated success/failure status; mock-RPC tests, including `rnstatus-rs` and `rnpath-rs` TCP/default and Unix-domain daemon RPC transports; local daemon smokes; JSON/human output golden checks; error-code and argument compatibility tests; daemon-backed `request_path` status/timeout/path metadata/scope tests; local non-neighbor mesh `rnpath-rs` smoke through `rnx rnpath-smoke`, including scoped/tagged refresh over the learned outgoing interface. | Full Python `rnodeconf` hardware workflows, retired utility replacements with no product demand, and client-app operator workflows. |
| RNS-CRNS | `CRNS/*` | RNS tools agent | Decide and implement selected CRNS command workflows needed by current products; keep unsupported Python command ecosystem gaps explicit. | CLI contract tests; mock transport/RPC fixtures; local end-to-end command smokes where command behavior is implemented. | Broad Python command ecosystem reproduction, physical-network CRNS validation, external operator-client evidence. |
| LXMF-ROUTER | `LXMF/LXMRouter.py` | LXMF router agent | Finish non-propagation router convenience behavior that affects supported SDK/daemon flows while preserving the completed propagation-router lifecycle baseline and improving the existing `lxmf-sdk`/`ZmqPipelineBackendClient` surface in place. | Focused daemon/RPC tests, including queued/pre-handoff `app.delivery.cancel` acceptance, persisted cancelled status, delivery trace/event/lifecycle observability, no later bridge handoff, `announce_received_wakes_pending_direct_and_opportunistic_outbound` coverage for Python-style `lxmf.delivery` announce wakeup of stored pending direct/opportunistic outbound work, and `identity_miss_status_defers_only_direct_and_opportunistic_peer_delivery` coverage for nonterminal reticulumd direct/opportunistic identity-miss deferral; typed ZeroMQ SDK tests including send/batch-send, delivery status, paper decode metadata, history, conversation summaries, direct cancel result variants, envelope cancel payload/extension preservation on `ZmqPipelineBackendClient`, and native `app.messages().cancel(...)` delegation through the existing `Client` app-domain surface; `lxmf`/`lxmf-cli` paper encode/decode CLI tests that reach the typed SDK paper surface; local direct/opportunistic/paper/propagated mode tests; pinned Python LXMD remote lifecycle interop when protocol behavior is touched. | Sideband, MeshChatX, Columba compatibility claims; manual client workflows; broad live store-and-forward soak; new parallel SDK surfaces. |
| LXMF-HANDLERS | `LXMF/Handlers.py` | LXMF handlers agent | Improve delivery, announce, propagation app-data, receipt, negative/drop, signature-status, and router-coupled side-effect observability on the existing SDK/RPC event stream. | Daemon bridge tests; handler callback tests; receipt and drop-state regressions including transport-origin delivery receipts publishing the same pollable SDK `receipt` event payload fields as RPC-origin receipts, successful direct packet/resource deliveries publishing SDK-pollable raw inbound events with LXMF bytes plus direct transport and signature metadata, direct packet/resource drops, propagated local-delivery drops, RPC-layer ignored-destination propagation rejects from `propagation_ingest`, Python-served alias ingest, and remote fetch/download/sync imports, decryptable remote fetched/downloaded propagated local-delivery decode/stamp/policy drops, duplicate remote fetch/download/sync imports that emit observer-visible `inbound_dropped` events without duplicate storage/upsert work and without unservable peer marks for processed-only payloads, pre-decode propagated local-delivery rejects for local-addressed short/undecryptable envelopes plus strict remote fetch/download local-import rejects, propagated local-delivery signature metadata on stored records plus raw inbound events for unknown-source, verified, and invalid-signature states, local propagated-delivery processed-transient markers that make later `propagation_ingest` report duplicate accounting without re-counting received messages, and replayed processed-transient or already-stored-message local propagated-delivery duplicate events that stay observer-visible without storing or recounting the duplicate; typed SDK app-event projections for inbound message, receipt, drop, and lifecycle payloads; `sdk_poll_events_v2` receive-side assertions; structured event/status assertions; pinned Python handler behavior fixtures. | External-client callback behavior, live mobile notification flows, hardware-dependent delivery paths; new parallel SDK event surfaces. |

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
  direct/opportunistic/propagated/paper routing, paper operation envelopes,
  selected propagation nodes, persistence, retries, and cancellation.
- LXMF handlers agent owns delivery, receipt, announce, propagation app-data,
  inbound bridge, negative/drop, and event observability behavior.
