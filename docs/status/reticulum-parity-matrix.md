# Reticulum Parity Matrix

Last reassessed: 2026-08-06

This is the maintained row-level status for Python Reticulum compatibility.
Repository-level posture and execution order live in
`docs/status/current-roadmap.md`.

Parity is recorded on two independent axes:

- implementation: `complete`, `partial`, or `not-applicable`;
- evidence: one or more of `unit`, `simulated`, `pinned-python`,
  `cross-implementation`, `prepared-host`, `third-party-client`, `hardware`,
  `public-network`, or `hardware-unverified`.

`hardware-unverified` is an evidence boundary, not an implementation failure.
Evidence labels describe validation scope independently of implementation status;
they do not downgrade a complete software surface.

Workspace paths are used for navigation. Published package names are
`reticulum-rs-core`, `reticulum-rs-transport`, and `reticulum-rs-rpc`.

v0.9.5 exposes software-controlled daemon operations through capability-gated
`RnsSdkRuntime`, `RnsSdkTransport`, `RnsSdkInterfaces`, and
`RnsSdkDataPlane` traits over both HTTP/Unix and canonical ZeroMQ. Pure
cryptography/wire behavior remains local-library access. Physical equipment and
human-operated validation remain the explicit v1.0 boundary.

## RNS 1.4.2 baseline update

The pinned Python baseline is RNS `1.4.2` at
`b48b96e61676504e0a4e527b33b9a0b4495c6872`. Regenerating the strict public
surface inventory produces **1,810 complete, 0 partial, and 1
not-applicable entry across 1,811 entries**.

The focused work has closed the demonstrated 1.4.2 routing, request-limit,
resource-serving-window, blocked-IP, `rnstatus`, and typed runtime/lifecycle
slices. The generated inventory has no remaining partial or unmapped entries;
the one not-applicable entry is the provenance-backed absent `CRNS` package.

The historical v0.9.8 release record retains its own release-boundary inventory.
Current `main` and `v0.9.9-rc.6` supersede it with this 1,810/0/1 software
inventory. The Rust resource sender enforces the Python
`receiver_min_consecutive_height` serving window; collision-list regeneration
and cross-implementation transfer evidence remain narrower follow-up concerns.
No hardware, public-network, or third-party-client claim is inferred from the
software inventory.

SDK negotiation exposes an optional typed `software_parity` orientation, and
daemon `status`, `daemon_status_ex`, and `rns.runtime.status` expose the same
structure under `reticulum.parity`. It separates overall, Reticulum, and LXMF
checkpoints, includes pinned reference versions and revisions, and reports
exact complete/applicable ratios alongside the inventory counts. It is marked
`advisory: true` for consumer orientation and does not replace capability
negotiation, runtime feature checks, or the separate hardware-evidence axis.

## Surface Matrix

| Python surface | Rust surface | Implementation | Evidence | Implemented baseline | Residual gap |
| --- | --- | --- | --- | --- | --- |
| `RNS/Reticulum.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | complete | unit, simulated, pinned-python | Deployable daemon, configuration, propagation-node activation, persistence, RPC, graceful shutdown, unified legacy `status`/`daemon_status_ex` runtime visibility, path-table restore status, blackhole state and cached-path eviction, runtime mutation, discovered-interface state, live interfaces, hot-apply policy, typed runtime accessors, and lifecycle helpers. | No generated callable software gap remains; broader runtime/application-policy scenarios and hosted evidence remain separate follow-ups. |
| `RNS/Identity.py` | `crates/libs/rns-core` | complete | unit, pinned-python | Identity material, hashing, signing, encryption, recall, and key conversion. | No confirmed parity blocker. |
| `RNS/Destination.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | complete | unit, pinned-python | Destination hashing, descriptors, announces, proof generation and validation, ratchets, known-key stability checks, and bounded request/response enforcement. Single-destination Data delivery proofs are correlated through the packet cache before identity verification. | No generated callable software gap remains; broader scenario and external-client evidence is tracked independently. |
| `RNS/Packet.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | complete | unit, pinned-python | Framing, serialization, contexts, proofs, receipts, public post-encryption packet-hash correlation, explicit and implicit proof-destination correlation, Python-default link proof context, and header semantics. | No confirmed parity blocker. |
| `RNS/Transport.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | complete | unit, simulated, pinned-python | Path and announce handling, gravity-aware replacement, dynamic path rebalancing, boundary path requests, path replacement/state/await semantics, routed links/resources/receipts/tunnels, next-hop formulas, interface lifecycle, discovery and blackhole state, persistence, runtime jobs, graceful shutdown, and focused scoped-request, pacing, duplicate-suppression, MTU, restore/restart, and transport-disabled evidence. | No generated 1.4.2 callable software gap remains; multi-device, public-network, and broader scenario evidence remains separate. |
| `RNS/Link.py` | `crates/libs/rns-transport` | complete | unit, pinned-python | Establishment, proof validation, bounded request/response correlation, bound-interface enforcement for data/channel fan-out, RTT-derived liveness, protocol close, cleanup, and the focused dynamic path-rebalancing slice. | No generated callable software gap remains; cross-implementation and external-client evidence is separate. |
| `RNS/Resource.py` | `crates/libs/rns-transport` | complete | unit, simulated, pinned-python | Bounded receive allocation, advertisement validation, retries, receiver-minimum collision-guard serving window, window-local hashmap exhaustion gating, bz2 compression, adaptive fragment scheduling, timeout/failure events, cancellation, cleanup, split-resource sequencing, ordered reassembly, per-segment metadata, and whole-resource completion. | Serving-window software contract is complete; collision-list regeneration and cross-implementation transfer evidence are narrower follow-ups. |
| `RNS/Channel.py` | `crates/libs/rns-transport` | complete | unit, pinned-python | Channel packet handling, retry scheduling, buffering, ordered receive delivery, callback ordering/short-circuit/panic containment, delivery-on-proof, timeout retry, exhaustion cleanup, and live Rust/Python channel sequence tests. | No confirmed channel parity blocker. |
| `RNS/Buffer.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | complete | unit, pinned-python | Packet buffers, readers/writers, and callback baseline. | No confirmed parity blocker. |
| `RNS/Interfaces/*` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | complete | unit, simulated, prepared-host, pinned-python, hardware-unverified | Configuration, framing, startup, reconnect, runtime status/mutation, management, teardown, interface gravity, Backbone blocked-IP statistics, loopback carriers, fake-SAM, PTY/fake-TCP, deterministic Meshtastic faults, BLE mocks, device-management state machines, and pinned-Python interface probes. | Software interface surfaces are complete; physical devices and public networks remain `hardware-unverified`. |
| `RNS/Discovery.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | complete | unit, simulated, pinned-python | Python-shaped interface-discovery persistence, filtering, age status, expiry and ordering; announce MessagePack encoding/decoding, 20-round LXStamper workblocks with a pinned-Python vector, stamp/source/endpoint validation, optional encryption callbacks, live daemon ingestion of authorized unencrypted discovery announces, deterministic announce scheduling, autoconnect/monitor/teardown planning, blackhole-update scheduling/merge/atomic persistence, and live AutoInterface discovery and peer runtime. Rust maps Python thread-owned side effects to deterministic lifecycle plans consumed at daemon/transport boundaries. | No generated public-callable software gap remains in `Discovery.py`; physical carrier and public-network evidence stay outside this implementation axis. |
| `RNS/Resolver.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | complete | unit, pinned-python | The pinned Python surface contains only the intentionally no-op `resolve_identity`; the active Python-reference workflow probes that behavior. Rust additionally provides cache lookup, restored path-table identity lookup from cached announces, cacheless path save filtering, Python-format stale path-table row suppression, missing/malformed/mismatched cached-announce tolerance for active and tunnel restore, persisted announce-identity lookup, daemon `path_status`/already-known `request_path` visibility, and `_runtime.reticulum.path_table_restore` status. | No confirmed parity blocker. |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | complete | unit, pinned-python | Required Reticulum primitives used by identities, packets, links, and receipts. | No confirmed parity blocker. |
| `RNS/Utilities/*` | `crates/apps/rns-tools` | complete | unit, simulated, pinned-python, hardware-unverified | Canonical `rnx`, `rnsd`, `rnstatus`, `rnpath`, `rnodeconf`, `rncp`, `rnid`, `rnir`, `rnpkg`, `rnprobe`, `rnsh`, and `rngit` binaries cover daemon/status delegation, scoped path requests, gravity display, blocked-IP statistics, radio-management software, identity persistence, binary-safe copy, probe status, isolated shell execution, and transport-neutral repository, release, permission, work-item, and Git-bundle workflows. | Software utility surfaces are complete; physical radio commands and operator/public-network evidence remain `hardware-unverified` or deferred. |
| `CRNS/*` | none | not-applicable | pinned-python | No `CRNS` package exists in either pinned reference tree. | Provenance is resolved; no Rust implementation is required. |

### Runtime and daemon compatibility

- `[reticulum] enable_transport` now controls the full Reticulum transport
  contract independently from interface startup: disabled instances do not
  retransmit announces/path responses or forward remote Link Requests and
  established-link traffic, while local destinations remain reachable.
  `panic_on_interface_error` remains a separate strict-startup policy.
- Legacy daemon RPC now exposes `next_hop`, `next_hop_if_name`,
  `first_hop_timeout`, and `link_count`, and tracks blackholed identity
  list/add/remove state with Python-compatible malformed-input behavior and
  restart-safe persistence of local entries and removals. Adding a blackhole
  also evicts every cached path whose recalled announce identity matches it.
- Shared-instance server/client/disabled state, final path-table flush, and
  path/tunnel restore skip accounting are visible through the daemon status
  surface and focused Rust regressions.
- `rnx` delivery discovery now follows durable `list_announces` state instead
  of treating delivery destinations as propagation peers. Mesh probes enable
  Reticulum forwarding and require every node to observe every delivery
  destination before exercising delivery. Full-wire packets and resources
  received over outbound LXMF delivery links resolve to the local destination,
  preserving bidirectional direct-backchannel delivery in two-node and
  multi-hop mesh runs.

## Interface Detail

Implemented interface families are active runtime code, not parser-only
placeholders:

### v0.9.0 interface evidence boundary

| Evidence slice | Applies here | Boundary |
| --- | --- | --- |
| LXMF send/receive | Consumed from the LXMF matrix when a named SDK scenario sends or receives over a Reticulum carrier. | Proves that scenario only; it does not close broad Reticulum interface parity by itself. |
| Carrier attach/announce software | LocalInterface TCP/Unix attach, AutoInterface carrier runtime, fake-SAM or real-SAM I2P attach, software loopback TCP/UDP/Backbone/Pipe/KISS/RNodeMulti/Weave smokes, and announce/path fanout with daemon/RPC status evidence. | Supports the implemented software carrier claims and release readiness for those carrier paths. |
| Optional HIL | RNode, RNodeMulti, Weave, VR-N76, BLE, prepared-host radio/device matrices, and long-running physical-carrier checks. | Adds operational confidence but remains optional for the v0.9.0 software-parity release and does not change implementation status by itself. |

- TCP client and server, including Python-style `fixed_mtu` handling where
  `0` keeps the default TCP MTU and non-zero values below the Reticulum MTU of
  500 bytes are rejected, plus KISS-framed client modes. TCP server and
  Backbone listener sockets set Python-style `SO_REUSEADDR` before bind.
  Python-style `i2p_tunneled` TCP clients and TCP server accepted streams use
  the Reticulum I2P socket profile (`TCP_NODELAY`, keepalive enabled, and on
  Linux/Android 45-second user timeout, 10-second keepalive idle, 9-second
  keepalive interval, and 5 probes), and `tcp_server` status settings preserve
  the accepted config flag. TCP/Backbone listeners refresh daemon/RPC runtime
  status with bind/listener state, accept counters, client liveness defaults,
  and the latest accepted stream snapshot. Ordinary TCP clients and Backbone
  clients emit reconnect events that re-synthesize tunnel state after
  reconnect, matching Python initiator-client behavior for non-KISS stream
  interfaces.
- BackboneInterface and BackboneClientInterface config compatibility over the
  existing TCP/HDLC runtime, including listener/client alias handling and
  Backbone's larger default MTU. Backbone listener child streams and outbound
  Backbone client streams apply Reticulum-style socket tuning through a
  dedicated hook: `TCP_NODELAY` on every platform and, on Linux/Android,
  `SO_KEEPALIVE`, TCP keepalive idle/interval/count, and TCP user timeout.
  Backbone streams also opt into the shared HDLC liveness watchdog, emitting
  idle keepalives, marking stale reads, and reconnecting after read timeout
  without changing ordinary TCP client/server defaults; focused watchdog tests
  now cover keepalive, stale, active-after-read, and read-timeout event order,
  and local slow-reader evidence proves the bounded HDLC tx queue backpressures
  instead of draining unbounded work while a Backbone peer stops reading. The
  pinned Python interop workflow now also runs a Python selector/epoll slow-reader probe
  with `backbone_selector_backpressure_probe.py`, requiring
  `EpollSelector` on Linux, plus a live pinned Python Reticulum
  `BackboneClientInterface` transmit-buffer probe with
  `backbone_python_reference_backpressure_probe.py`, comparing those results
  with the Rust `backbone_hdlc_stream_backpressures_when_peer_stops_reading`
  proof. The same ignored `python_channel_interop` workflow now also includes
  focused live Backbone channel, link-data, request/response, and resource
  roundtrips in both directions between Rust's Backbone-tuned TCP/HDLC path
  and Python `BackboneInterface`/`BackboneClientInterface`. Python
  `BackboneInterface` configs using `remote` now have focused daemon
  parse-to-bootstrap/status coverage as `backbone_client`.
- LocalInterface TCP-loopback listener/client-attach plus Unix filesystem and
  Linux/Android abstract AF_UNIX shared-instance listener/client-attach
  compatibility over the existing stream/HDLC runtime, including Python's
  global `[reticulum] share_instance` synthesis when no explicit local
  shared-instance interface is configured, Python's default
  `127.0.0.1:37428` endpoint, `@rns/<instance_name>` Unix naming, and
  262144-byte local MTU. Python-style `force_shared_instance_bitrate` pacing
  delays outbound shared-instance packet writes before HDLC framing on TCP and
  Unix client streams. Unix client-attach retries after initial connect
  failures and reconnects after stream disconnects; TCP and Unix attach
  reconnect signals re-synthesize tunnel state through `reticulumd`, and
  attached shared-instance clients wrap one-hop outbound packets in transport
  headers before handing them to the shared instance. When global
  `share_instance` synthesizes an implicit TCP `LocalInterface`, that listener
  can now coexist with another configured TCP or Backbone listener by starting
  as a daemon sidecar while explicit multi-listener TCP configs still use the
  primary single-bind selector. Software TCP and Unix shared-instance smokes now
  prove strict daemon startup, loopback listener status, attach-client status,
  filesystem Unix listener startup, Linux abstract Unix listener/client attach,
  Python local MTU and bitrate alias reporting, fake shared-instance attach,
  and `rnstatus-rs` JSON/human output without another local Reticulum process.
  The Unix report records
  `evidence_scope = "software_unix_shared_instance_local"` so it is not
  mistaken for multi-process Python shared-instance interop evidence. A pinned
  Python shared-instance smoke now records
  `evidence_scope = "python_shared_instance_tcp_unix_attach_and_announce_forward"` after
  `reticulumd` attaches to real Python Reticulum shared instances over TCP and
  Linux abstract Unix sockets, then observes Python-origin announce fanout
  through those shared instances with traffic-client `announced_count` and
  shared-server `local_client_rxb_total`/`local_client_txb_total` counters; that
  report remains scoped to attach plus announce fanout and not broad
  application-level shared-instance traffic parity. Independent rns-rs evidence
  separately attaches a real local client to an LXMF-rs `reticulumd`, discovers
  an LXMF-rs endpoint across the daemon, exchanges encrypted packets and proofs
  in both directions, replaces the daemon, verifies client identity continuity
  plus interface-down/up reconnection, and repeats both traffic directions.
  That cross-implementation scenario also guards final-hop transport-header
  removal after a shared-instance hop. LocalInterface #384 evidence
  is included in the executable Reticulum interface parity audit at
  `target/reticulum-interface-parity-audit/report.json`, which records
  `evidence_scope = "reticulum_interfaces_384_385_parity_audit"`, accepts
  `RNODE_HIL_ARTIFACT_MANIFEST` for
  `schema = "reticulum_interface_hil_matrix_artifacts.v1"` SHA-256 checking,
  and lists `missing_full_parity` when paired RNode evidence is incomplete.
- PipeInterface subprocess stdin/stdout transport with Python-style command
  parsing, HDLC packet framing, respawn delay, default MTU, and live subprocess
  status reporting through daemon/RPC `_runtime.pipe.status`. A software
  fake-subprocess smoke now proves strict daemon startup and `rnstatus-rs`
  JSON/human reporting for a running `cat` subprocess without external devices.
- UDP unicast and multicast with peer routing, multicast proof fallback,
  Python-style `device` broadcast-address defaults via host interface lookup,
  IPv4 broadcast socket sends, and Python `UDPInterface` alias semantics where
  shared `port` can default both listen and forward ports but `listen_port`
  alone does not imply forwarding. Daemon/RPC status now refreshes UDP bind
  state, role, last observed peer-route count, packet, byte, drop, and error
  counters into `_runtime.udp.status`, and `rnstatus-rs` renders those rows for
  operators. A software loopback smoke now proves Python-style alias parsing,
  strict startup, bound loopback status, and malformed-datagram
  `bytes_rx`/`decode_errors` telemetry without external network services.
  Runtime interface mutation now hot-applies host-bound and device-bound TCP
  server listeners, including loopback, `localhost`, IPv4 wildcard, concrete
  local addresses, hostnames, and device-selected IPv4/IPv6 addresses, plus explicit UDP listener, peer,
  multicast-bind, and multicast-forward records through `set_interfaces` and
  `reload_config`, while `device`-bound, non-local concrete, and broader TCP server
  startup-only interface families and UDP partial-target and out-of-range-target records remain
  restart-required or invalid. Device-bound UDP records resolve Python-style
  IPv4 broadcast defaults during hot-apply. Duplicate
  TCP server and UDP binds are rejected before mutation. Hot-applied explicit
  TCP server records attach live daemon/RPC `_runtime.tcp.listener_status`
  metadata, hot-applied explicit UDP records attach the runtime iface and
  refresh live daemon/RPC `_runtime.udp.status` counters under focused
  software tests; multicast-bind and multicast-forward hot-apply use the
  transport peer-routing helper.
- Serial now refreshes live daemon/RPC status with open/reconnect, HDLC frame,
  packet, byte, EOF, queue, decode, serialize, read, and write-error counters.
  Serial KISS and AX.25 KISS retain Python-compatible AX.25 UI header wrapping
  over the serial KISS runtime. Android-style KISS beacon aliases
  `beacon_interval` and `beacon_data` feed the same ID beacon runtime as
  Python `id_interval` and `id_callsign`. KISS/AX.25 KISS and KISS TCP now
  refresh live daemon/RPC status with packet, data-frame, command-frame, byte,
  flow-control, queue, AX.25 drop, and error counters, and `rnstatus-rs`
  renders those counters alongside configured bearer metadata. A software
  fake-PTY smoke now proves Python-style serial `KISSInterface` and
  `AX25KISSInterface` configs, strict startup, KISS startup command emission,
  fake READY handling, and refreshed daemon/operator status without attached
  modem hardware. Python
  `TCPClientInterface` configs with `kiss_framing = true` now have focused
  daemon parse-to-bootstrap/status coverage as `kiss_tcp_client` with
  `_runtime.kiss_tcp.status`, plus a software fake-TCP smoke proving strict
  startup, KISS startup command emission, fake READY handling, and refreshed
  daemon/operator status without a real Wi-Fi KISS bridge or TCP modem.
  BLE GATT now
  refreshes live daemon/RPC status with connection/subscription, packet, HDLC
  frame, notification byte, payload byte, write-chunk, reconnect, startup
  phase, queue, decode, serialize, read/write, buffer-drop, cleanup, and
  last-error counters alongside configured BLE UUID and lifecycle timeout
  metadata.
- AutoInterface discovery, authenticated peering, peer lifecycle, duplicate
  suppression, multicast announcements, data sockets, transport bridging, and
  live carrier-runtime status reporting, including polling reconciliation for
  already adopted link-local address replacements, supervised per-interface
  discovery and data-listener receive loops, adopted-interface add/remove/change
  diff planning with explicit state apply semantics, daemon-side add/remove
  lifecycle application for active and zero-initial AutoInterface runtimes,
  Python-style multicast echo freshness seeding when adopted interfaces are
  added at runtime, stale outbound route pruning after restart/removal, dynamic
  multicast/reverse announce source refresh after replacement, and Python-style
  fallback from unknown `multicast_address_type` values to `temporary`.
- Serial, TCP/Wi-Fi, and feature-gated BLE LoRa/RNode with startup probes,
  Python and Android-style selector aliases, configuration validation,
  telemetry, flow control, teardown, display-capable BLE external-framebuffer
  disable before shutdown, frame-level helpers for blink, Bluetooth control,
  display/NeoPixel controls, interference-avoidance control, Wi-Fi settings,
  config save/delete, firmware-update metadata, and ROM/EEPROM read/write/wipe
  requests, and live daemon/RPC `rnode_status` refresh plus compact
  `rnstatus-rs` human summaries for probe and radio state, with an opt-in
  prepared-host smoke harness for serial, TCP/Wi-Fi, or BLE RNode devices.
  The bearer-neutral `RnodeBearerBackend` and single-attempt
  `RnodeBearerKissInterface` reuse that KISS/probe/configuration runtime for
  platform-owned BLE and Bluetooth Classic streams. Focused
  no-default-feature tests cover shared framing, notification preservation,
  empty-read backoff, cancellation-safe and idempotent close, and contextual
  close-failure status during aborted startup. Native Android callback/resource
  behavior, physical RNode BLE/SPP lifecycle cycling, and long-running hardware
  soak evidence remain `hardware-unverified` external mobile/HIL work.
- Meshtastic tunnel support includes the reference `RETICULUM_TUNNEL_APP`
  framing/reassembly layer, modem-preset pacing, missing-chunk requests,
  node/destination route learning, an injectable bearer handle, daemon config
  startup, runtime status refresh, and deterministic loopback simulation.
  Native serial/TCP/BLE device evidence remains hardware-unverified.
  The software row is implementation-complete with a committed 32-seed loss
  and reordering corpus plus malformed-command/error-state coverage.
- Shared serial/TCP RNodeMulti baseline with nested vport subinterfaces,
  `CMD_SEL_INT` KISS vport selection, direct routing to virtual child
  interfaces, Python-style child enabled/interface-enabled handling, broadcast
  fanout only to outgoing children, and startup probe validation for detect,
  firmware `>= 1.74`, platform, MCU,
  `CMD_INTERFACES` discovery, hardware-reported configured vports, and
  selected-vport radio command/status bookkeeping. Safe RNode management
  commands can be queued through daemon RPC by selecting the parent interface
  and providing a configured child `vport`; the transport writes `CMD_SEL_INT`
  before each queued management frame. Parent-level Python
  `id_callsign`/`id_interval` settings fan out raw callsign ID beacons on
  outgoing subinterfaces after first traffic. Software fake-TCP and fake-PTY
  smokes now prove Python-style TCP parent config, serial PTY parent config,
  strict startup probe/status refresh, `rnstatus-rs` JSON/human reporting, and
  `rnodeconf-rs` vport blink dispatch through the real daemon path without
  hardware. Strict startup mode preflights
  the configured serial or TCP parent endpoint and records startup failure
  instead of registering management targets when the endpoint is unavailable.
  Display-capable ESP32/NRF52 devices get Python-style external-framebuffer
  disable during teardown before per-vport radio-off and leave-host payload
  `0xff` frames. Daemon/RPC snapshots refresh over the `radio_status` runtime
  metadata schema, including stream/probe state, last-error reporting, and
  accepted or partial startup-probe firmware/platform/MCU/interface metadata
  from non-cancelled probe attempts, with an opt-in prepared-host smoke harness
  for serial or TCP RNodeMulti devices.
- Shared serial Weave baseline with WDCL over HDLC framing, discovery
  handshake response, endpoint event learning, virtual peer child interfaces,
  inbound endpoint packet routing, direct endpoint command writes,
  target-scoped remote-display frame capture with byte-coverage completion,
  CPU/task/memory stat parsing, and transport-side status bookkeeping refreshed
  into daemon/RPC `_runtime.weave.status`, with `rnstatus-rs` rendering remote
  switch ID, byte/frame counters, invalid-frame and last-log diagnostics,
  display progress/color, CPU/memory, and task-stat counts plus a
  `--weave-display` display-focused view and a Python-compatible
  `WDCL_CMD_REMOTE_DISPLAY` enable/disable frame primitive, live
  `weave_remote_display_control` RPC dispatch, and `weaveconf-rs`
  enable/disable commands, including software cancel/stop closure of link,
  WDCL-connected, and endpoint state. A software fake-PTY smoke now proves
  signed WDCL discovery, connected status refresh, endpoint/display/device-stat
  reporting, `rnstatus-rs --weave-display`, and live `weaveconf-rs`
  remote-display enable/disable dispatch through the real daemon path without
  hardware, with an opt-in prepared-host smoke harness for connected serial
  Weave devices.
- I2P SAM baseline, with transient stream sessions, `.i2p` name lookup, HDLC
  framing, virtual peer child interfaces, direct peer sends, broadcast fanout
  across configured peers, `STREAM ACCEPT` connectable sessions, and private
  destination key persistence under the daemon storage root by default or under
  explicit `state_path`/`storagepath` when configured,
  using Python-compatible hashed `.i2p` filenames with old-format key reuse and
  identity-bound new-format key names for generated destinations. Missing
  explicit SAM host/port config honors Python's `I2P_SAM_ADDRESS` `host:port`
  environment default before falling back to `127.0.0.1:7656`. Startup metadata
  reports the derived `.b32.i2p` endpoint for persisted keys and keys generated
  during startup, plus transport-side tunnel state, keepalive, stale,
  read-timeout, per-peer counter bookkeeping, and bounded closed-incoming-peer
  history refreshed into daemon/RPC `tunnel_status` runtime metadata. Local
  fake-SAM tests now cover outbound peer-loop session creation, lookup, stream
  connect, HDLC writes, connectable accept-loop incoming `STREAM ACCEPT`,
  virtual child registration, HDLC ingress, direct outbound egress over accepted
  streams, cleanup, and daemon/RPC status refresh for connected outbound and
  incoming peer rows without requiring a prepared I2P router. SAM session IDs
  now include the daemon transport identity when available to avoid
  cross-process ID collisions on a shared router, and expired accept-session IDs
  recreate the connectable session instead of retrying a dead ID indefinitely.
  The config parser accepts I2P-local IFAC aliases `ifac_netname` and
  `ifac_netkey`.
- Feature-gated native RNode BLE and VR-N76 KISS-over-BLE. The VR-N76 native
  interface now exposes live daemon/RPC `_runtime.vrn76.status` metadata with
  connection, subscription, readiness, startup-write failure, and queued packet
  counters, and `rnstatus-rs` renders a compact human summary. An opt-in
  prepared-host smoke harness records VR-N76 daemon startup,
  connected/subscribed/ready, and counter evidence under `target/vrn76-hil/`
  with `evidence_scope = "prepared_host_vrn76_ble_readiness"`; broader write,
  indication, disconnect, reconnect, adapter, firmware, and channel-ID
  hardware evidence remains pending.

Python-style interface-driven `tcp_server` startup now works from config
without Rust-only transport overrides.

Cached remote path-response announces now carry `PacketContext::PathResponse`
when scheduled from a known path, matching Python's `PATH_RESPONSE` treatment
for direct path answers and keeping ordinary announce rebroadcast policy
separate from path-response delivery.
When an ordinary announce is already queued for the same destination, a due
known-path `PATH_RESPONSE` now drains first and the ordinary announce
rebroadcasts afterward, matching Python's `held_announces` ordering.
Unknown-announce ingress limiting now has harness-dispatchable local evidence
for Python-style per-interface holding and lowest-hop release, preventing one
bursty ingress interface from masking independently releasable held announces
on another software ingress.
Unknown-path discovery requests now retain the requesting interface while
recursive discovery is forwarded, then answer that requester with an immediate
direct `PATH_RESPONSE` when a matching announce arrives.
Recursive path-request forwarding now respects Python's interface announce
pacing gates: queued announces and active announce-cap windows block recursive
requests, and admitted recursive requests advance the cap window.
Path-request duplicate/throttle scoping now has focused software coverage:
inbound duplicate request suppression is scoped by destination, requesting
transport, request tag, and ingress interface and expires after the request
timeout; local path-response suppression is scoped by destination, requesting
transport, request tag, and egress interface; recursive request caps and queue
limits are scoped per source interface; and expired recursive requests release
that interface capacity. This does not claim full transport parity; live mesh
and public-network behavior remain deferred.
Unknown recursive path discovery now also respects Python's
`DISCOVER_PATHS_FOR` interface-mode gate, forwarding only from access-point,
gateway, and roaming interfaces and suppressing waiting discovery requester
state for full, point-to-point, and boundary interfaces.
Incoming announces now carry their Python-format random blob through validation
into the path table. The table preserves bounded random-blob history for
Python-format persistence, ignores duplicate/stale blobs, refreshes known paths
from fresh same-hop or better announces, and allows expired or newer higher-hop
announces to replace the route and downstream announce side effects.
Routed link-table proof timeouts now model Python's unresponsive-path
exception for one-hop paths and link requests that previously took a one-hop
route: Rust marks the existing path unresponsive, requests rediscovery while
blocking the ingress interface, and allows a same-timebase higher-hop announce
to replace the unresponsive route.
Never-activated outbound links now expire their stale destination path and
schedule Python-style rediscovery path requests, with the 20-second
`PATH_REQUEST_MI` throttle and shared-instance client suppression.
Known-path requests received on a roaming-mode interface are no longer answered
when the learned next-hop interface for that path is the same interface,
matching Python's roaming-interface loop suppression.
Known-path requests that arrive on roaming-mode interfaces through a different
learned next-hop now apply Python's extra roaming response grace before sending
the direct path response.
Scoped daemon path requests now keep broadcast packet semantics while selecting
only the requested interface at dispatch time, and scoped/tagged refreshes still
issue when an unscoped cached path is already known. The local `rnx
rnpath-smoke` daemon mesh now exercises that path with `rnpath-rs --on-iface
--tag-hex` after the unscoped non-neighbor route is discovered.
Pinned Python path-discovery interop now covers Rust `reticulumd` requesting a
previously unknown Python delivery path over loopback TCP, observing found
route metadata through `path_status`, and confirming the route through
`rnpath-rs --json`. A sibling pinned Python case then reissues `rnpath-rs` with
`--on-iface` and `--tag-hex` over the learned interface, proving the
scoped/tagged daemon dispatch path and result metadata against a Python-learned
route instead of only local Rust daemons.
The mirror Python-origin path-request case suppresses Rust startup/periodic
announces, holds a quiet window where Python still has no Rust delivery path,
and then proves Python `RNS.Transport.request_path()` can discover the Rust
delivery destination over the same software loopback path.
Restored path-table cached announces are now kept as lookup/cache material
rather than scheduled as fresh announce rebroadcasts at startup, while still
serving known-path responses. Path-table save filters routes without cached
announce material, and restore ignores stale/expired Python-format active and
tunnel path-table rows before reintroducing resolver/cache state.
Malformed per-entry cached announce files and cached announces whose decoded
destination does not match the active/tunnel path row are skipped without
poisoning other valid restored routes, while malformed `destination_table` or
`tunnels` files still surface as daemon restore errors.
Daemon/RPC `_runtime.reticulum.path_table_restore.skipped` now reports
per-reason active/tunnel skip counters for unmapped interfaces, expired rows,
missing or invalid cached announces, mismatched cached destinations, duplicate
tunnel packet hashes, and identity conflicts.
Shared-instance clients now skip local path-table save and restore work like
Python Reticulum.
Tunnel-only restored announces are also retained as cache material, so paths
restored when a tunnel reappears can answer later known-path requests with
direct `PATH_RESPONSE` packets. Restored tunnel paths now preserve bounded
Python-format random-blob windows and compare them with any active path, so a
reappearing tunnel cannot replace a fresher active route unless the existing
path is expired or the tunnel path is at least as fresh under Python's
timebase rules, with active-preservation and fresher-tunnel replacement
evidence.

Enabled unknown interface kinds still parse so operators can see them in daemon
status, but daemon startup marks them as failed with explicit
`unsupported interface kind` runtime metadata instead of silently dropping the
record.

`RNS/Interfaces/*` is complete on the software implementation axis. The
remaining boundary is evidence scope, not a claim that implemented interfaces
are stubs. Backbone
now has Python selector/epoll and live Python Reticulum BackboneClientInterface
slow-reader probes for the same qualitative backpressure workload, plus focused
live Rust/Python Backbone channel, link-data, request/response, and resource
roundtrips in both directions against the pinned Python Reticulum reference.
AutoInterface
now has daemon-side dynamic add/remove reconciliation for an active runtime
using the implemented diff plan plus discovery and data listener supervisors.
Zero-initial startup now keeps the polling reconciler and scheduler runtime
alive for later adopted devices, and the supervisors track replacement-stop
tasks so dynamically replaced listeners are drained during restart, removal, or
runtime shutdown. `_runtime.auto.carrier_runtime` now exposes the last peer
lifecycle job's expired-peer count, reverse peer announce count, missing
initial multicast echo count, carrier event summary, post-job peer count, and
peer-data admitted/delivered/decode-failed/RX-closed outcomes in software
daemon tests, making Python-style peer expiry/reverse-announcement state and
transport handoff failures visible without claiming public or hardware
discovery parity. A software-only smoke records the existing transport and
daemon AutoInterface regressions under
`target/auto-interface-software-smoke/` with
`evidence_scope = "software_auto_interface_runtime"`, explicitly excluding
Linux namespace churn, real Wi-Fi/Ethernet churn, public-network soak, and
external-client evidence.
An opt-in Linux namespace prepared-host smoke now records
zero-initial add, link-local replacement, and removal churn evidence through
refreshed `_runtime.auto` status with `evidence_scope =
"linux_namespace_dummy_churn"`; broader prepared-host interface churn evidence
across real Wi-Fi, Ethernet, and platform combinations remains pending.

`I2PInterface` is tracked as an in-progress family: configured outbound peers
and connectable sessions can run through SAM, and transport-side tunnel
watchdog/status bookkeeping is refreshed into daemon/RPC interface status, with
fake-SAM coverage for outbound peer-loop writes, connectable accept-loop HDLC
ingress, accepted-stream direct egress, cleanup, and runtime counter/status
updates.
Private destination keys now follow Python's default daemon-storage injection
and hashed key-file naming, including old-format fallback when an existing
Python key is present. Missing explicit SAM host/port config now uses Python's
`I2P_SAM_ADDRESS` environment default when it is set to `host:port`.
`rnstatus-rs` human output summarizes the live I2P tunnel status for
operators, including outbound, incoming, closed, and aggregate byte counters.
The software fake-SAM smoke exercises strict daemon startup, destination
persistence, a transient outbound `NAMING LOOKUP` failure followed by recovered
connected peer state with cleared last error, connectable accept status,
accepted incoming peer visibility, and `rnstatus-rs` JSON/human output without
a real I2P router, with `evidence_scope = "software_fake_sam_i2p_runtime"`.
The opt-in prepared-host smoke can also require configured
outbound peers to reach `connected` state when `I2P_PEERS` is supplied. Its
report explicitly records whether the run proved only `sam_connectable_only`
behavior or `sam_connectable_with_outbound_peers` behavior, so no-peer runs are
not mistaken for outbound peer production parity. The real-SAM pair smoke now
records `evidence_scope = "sam_connectable_with_outbound_peers_real_pair"`
with connected dialer outbound and acceptor incoming peer rows for two local
daemons sharing one router, and can optionally record
`sam_connectable_with_outbound_peers_real_pair_soak` with periodic
`rnstatus-rs` samples for bounded single-router stability. The nightly HIL
matrix includes that pair path through the `i2p-pair` profile and uploads
`i2p-prepared-host-pair-artifacts`. Broader public I2P peer-set and
long-running production evidence remain pending.
Ordinary serial/TCP and feature-gated BLE `RNodeInterface` now refresh transport-side probe/radio
state into daemon/RPC `_runtime.lora.rnode_status`, and `rnstatus-rs` renders a
compact human summary for operators. Python `RNodeInterface` alias configs now
have daemon parse-to-bootstrap/status coverage as `lora` with
`_runtime.lora.rnode_status`. An opt-in prepared-host smoke harness now
records serial/TCP/BLE RNode lifecycle evidence under `target/rnode-hil/` with
bearer-scoped `evidence_scope` values (`prepared_host_serial_rnode`,
`prepared_host_tcp_rnode`, and `prepared_host_ble_rnode`) so one prepared
endpoint is not mistaken for broad hardware parity. The prepared-host gate also
queues safe `rnodeconf-rs query-radio-state` and `blink` management dispatch
through the live daemon binding, records the command JSON artifacts, and
requires a post-management status snapshot that remains online, radio-on, and
free of command or hardware errors. Transport-side serial/TCP LoRa runtime
status now also reports safe-management commands, guarded persistent/destructive
boundaries, queue depth/capacity, accepted and failed operation counters, the
last operation ID/command/state, and the last management error for SDK/daemon
status consumers.
Software-only RNode BLE fallback/management evidence now writes
`evidence_scope = "software_rnode_ble_fallback_management"` under
`target/rnode-ble-software-smoke/`, covering feature-gated identifier/alias
matching, configured Android peripheral fallback exclusion, command-monitor
status/degraded fallback, outbound RNode BLE MTU rejection and MTU-sized transmit,
management-frame chunking/queueing, `rnodeconf-rs` extended management
command-to-RPC coverage, feature-gated `reticulumd` daemon `RnodeBle`
management bridge dispatch, persistent/destructive CLI guard enforcement, and
shared closed-queue cleanup regressions. A fake TCP RNode smoke now records
`evidence_scope = "software_fake_tcp_rnode_prepared_host_management"` by
running the ordinary prepared-host path against a deterministic local KISS TCP
peer and verifying startup, radio configuration, radio-state query, and blink
management frames reached the peer. RNode BLE #385 is covered by the executable
Reticulum interface parity audit, which requires the software evidence plus
serial, TCP/Wi-Fi, and BLE prepared-host RNode hardware reports before strict
full-parity mode can pass. The hardware reports are collected by
`tools/scripts/reticulum-interface-hil-matrix.sh` into
`target/rnode-hil/matrix/{serial,tcp,ble}.report.json` and summarized in
`target/reticulum-interface-hil-matrix/report.json` with
`evidence_scope = "reticulum_interfaces_384_385_hil_matrix"` plus
`target/reticulum-interface-hil-matrix/artifact-manifest.json` with SHA-256
digests. Nightly HIL
exposes this collection path through the `reticulum-interface-matrix` profile;
strict hardware reports must include endpoint,
bearer, firmware, platform, MCU identity, and capture provenance fields.
Display-capable BLE RNode shutdown now disables the external framebuffer before
radio-off/leave frames. Android configured RNode BLE reconnect now excludes
the failed configured peripheral from fallback scan matching, with shared alias
matching helpers and stable service-UUID fallback log context. Serial/TCP RNode
streams now expose a transport-local management dispatch handle that writes
pre-encoded KISS command frames through the live KISS runtime; feature-gated
BLE RNode streams expose the same management dispatch through the Nordic UART
write path with BLE chunking.
Radio-state query and blink dispatch are covered by local duplex/mock tests,
daemon `rnode_management` RPC dispatch, `rnodeconf-rs` query/blink CLI tests,
and prepared-host safe-management artifacts when the serial/TCP/BLE HIL gate is
enabled. Daemon RPC and `rnodeconf-rs` also queue safe config read, ROM read,
display intensity/blanking/rotation/recondition/address, NeoPixel intensity,
and interference-avoidance enable/disable controls. Daemon RPC and
`rnodeconf-rs` additionally
queues guarded Bluetooth control, config save/delete, ROM write/wipe, hard
reset, firmware metadata, and Wi-Fi settings.
Frame-level helpers now cover Bluetooth disable/enable/pair control,
display/NeoPixel controls, interference-avoidance control, Wi-Fi settings,
config save/delete, firmware-update metadata, and ROM/EEPROM read/write/wipe
requests. Shared transport dispatch also removes interface records whose TX
queues have closed, including shared virtual-interface queues, preventing stale
closed paths from lingering after failed dispatch. Broad BLE management
hardware evidence across device, firmware, and operator workflows plus full
Python `rnodeconf` parity remain pending.
`RNodeMultiInterface` is tracked separately as an in-progress family: the
shared serial/TCP vport routing slice exists and startup validates detect,
firmware `>= 1.74`, platform, MCU, `CMD_INTERFACES`, and hardware-reported
configured vports. Selected-vport radio status bookkeeping and live daemon/RPC
`radio_status` refresh exist, including stream/probe state, last-error
reporting, accepted or partial startup-probe firmware/platform/MCU/interface
metadata from non-cancelled probe attempts, and the ordinary RNode radio-status
schema for each vport,
strict startup preflights the parent serial/TCP endpoint before registering
management targets, display-capable teardown disables the external framebuffer
before per-vport radio-off/leave frames, clean stream EOF/software stop reports
closed without masking read/write/probe failures, daemon RPC binds the parent
interface to the vport-aware management queue with explicit child `vport`
validation, and `rnstatus-rs` renders a compact human summary of that state
including the accepted probe metadata. Software fake TCP/PTY smokes now record
`software_fake_tcp_rnode_multi` and `software_fake_pty_rnode_multi` evidence
scopes with product-boundary notes so they are not mistaken for hardware
parity. An opt-in
prepared-host smoke harness now records serial/TCP RNodeMulti evidence under
`target/rnode-multi-hil/` with `evidence_scope =
"prepared_host_single_device_vport_probe"`, making clear that a passing run
proves one configured endpoint and vport set rather than broad production
parity across device, firmware, and radio combinations. Broader prepared-host
hardware validation and production parity are still pending.
`WeaveInterface` is also tracked as an in-progress family: WDCL/HDLC endpoint
packet routing, target-scoped display-frame capture with byte-coverage
completion, CPU/task/memory stat parsing, daemon/RPC status refresh, and compact
`rnstatus-rs` human summaries with remote switch, frame/log, display progress,
device-stat detail, and a `rnstatus-rs --weave-display` framebuffer/status view
exist. A Python-compatible WDCL remote-display enable/disable command frame
primitive is covered in transport tests, and `reticulumd` now wires live
dispatch through `weave_remote_display_control` with `weaveconf-rs`
enable/disable commands. Software cancel/stop now marks the runtime closed and
clears endpoint children. The software fake-PTY smoke now records
`software_fake_pty_weave` evidence scope with a product-boundary note so it
remains distinct from prepared-host hardware evidence. An opt-in prepared-host
smoke harness records
connected serial evidence under `target/weave-hil/` and can optionally prove
the live `weaveconf-rs` remote-display enable/disable dispatch against that
connected device. Its report distinguishes `prepared_host_connected_serial`
evidence from `prepared_host_serial_discovery_only` bring-up evidence, while
broader prepared-host hardware evidence across device, firmware,
display/status payload, and operator-workflow combinations remains pending.

## Highest-Priority Gaps

1. Close remaining announce/path/discovery edge-policy differences beyond the
   cached remote path-response `PATH_RESPONSE`, same-destination
   `PATH_RESPONSE`/ordinary-announce ordering, roaming same-interface
   suppression, and passed-on rebroadcast completion slices.
2. Capture broader prepared-host BLE/RNode lifecycle and safe-management
   evidence across bearer, device, firmware, and radio combinations.
3. Capture broader public I2P peer-set and long-running prepared-host evidence
   before claiming complete outbound peer production parity.
4. Capture broader RNodeMulti prepared-host hardware validation across device,
   firmware, and radio combinations.
5. Implement real utility equivalents only where product demand justifies them.

## Evidence

- Workspace unit and integration tests cover core, transport, daemon, serial,
  BLE, LoRa, AutoInterface, link, channel, buffer, and resource behavior.
- `.github/workflows/verify.yml` runs pinned live Python channel/link/request/resource and
  LXMF compatibility scenarios plus Python selector/epoll and live Python
  Reticulum BackboneClientInterface slow-reader probes for Backbone
  backpressure evidence.
- Nightly mesh, soak, and embedded HIL workflows provide additional operational
  evidence, but do not promote unsupported interface families to `done`.
