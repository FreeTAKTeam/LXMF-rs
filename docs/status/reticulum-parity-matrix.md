# Reticulum Parity Matrix

Last reassessed: 2026-06-27

This is the maintained row-level status for Python Reticulum compatibility.
Repository-level posture and execution order live in
`docs/status/current-roadmap.md`.

Status legend:

- `done`: implemented in the active workspace and backed by active tests.
- `partial`: useful behavior exists, but identified Python behavior or evidence
  remains missing.
- `not-started`: no meaningful active implementation.

Workspace paths are used for navigation. Published package names are
`reticulum-rs-core`, `reticulum-rs-transport`, and `reticulum-rs-rpc`.

## Surface Matrix

| Python surface | Rust surface | Status | Implemented baseline | Residual gap |
| --- | --- | --- | --- | --- |
| `RNS/Reticulum.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Deployable daemon, configuration, propagation-node activation, persistence, RPC, graceful shutdown, and multiple live interfaces. | Python runtime/config mutation and interface breadth remain wider. |
| `RNS/Identity.py` | `crates/libs/rns-core` | done | Identity material, hashing, signing, encryption, recall, and key conversion. | No confirmed parity blocker. |
| `RNS/Destination.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Destination hashing, descriptors, announces, proof validation, ratchets, and known-key stability checks. | No confirmed parity blocker. |
| `RNS/Packet.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Framing, serialization, contexts, proofs, receipts, Python-default link proof context, and header semantics. | No confirmed parity blocker. |
| `RNS/Transport.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Path and announce handling, including direct cached remote path responses stamped as `PATH_RESPONSE`, Python-style roaming same-interface known-path response suppression, path-table restore from cached announces without startup rebroadcast, restored tunnel-path announce cache lookup for later path responses, and shared-instance path-table save/restore suppression, link routing, resources, receipts, interface-aware sending, pacing, and duplicate suppression. | Remaining announce/path edge policy and full runtime behavior require live parity evidence. |
| `RNS/Link.py` | `crates/libs/rns-transport` | done | Establishment, proof validation, bound-interface enforcement, RTT-derived liveness, protocol close, and cleanup. | Continue live regression coverage; no confirmed blocker. |
| `RNS/Resource.py` | `crates/libs/rns-transport` | done | Bounded receive allocation, advertisement validation, retries, adaptive fragment scheduling, timeout/failure events, cancellation, and cleanup. | Split/segmented resources remain intentionally unsupported and rejected. |
| `RNS/Channel.py` | `crates/libs/rns-transport` | done | Channel packet handling, retry scheduling, buffering, ordered receive delivery, callback ordering/short-circuit/panic containment, delivery-on-proof, timeout retry, exhaustion cleanup, and live Rust/Python channel sequence tests. | No confirmed channel parity blocker. |
| `RNS/Buffer.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Packet buffers, readers/writers, and callback baseline. | No confirmed parity blocker. |
| `RNS/Interfaces/*` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | TCP client/server, including Python-style TCP-over-I2P `i2p_tunneled` socket tuning for outbound clients and accepted server streams, TCP/Backbone client reconnect tunnel re-synthesis, TCP/Backbone listener runtime status refresh into daemon/RPC status with accept counters and latest accepted stream snapshot, Backbone TCP/HDLC listener/client compatibility with Backbone MTU defaults, Reticulum-style Backbone socket tuning for Backbone client and accepted listener streams (`TCP_NODELAY`, Linux/Android `SO_KEEPALIVE`, TCP keepalive idle/interval/count, and TCP user timeout), Backbone-only HDLC liveness keepalives/stale/read-timeout reconnects, and local slow-reader HDLC tx backpressure evidence paired with Python selector/epoll and live Python Reticulum `BackboneClientInterface` slow-reader probes plus focused live channel/link/request/resource roundtrips, LocalInterface TCP-loopback listener/client-attach plus Unix filesystem and Linux/Android abstract AF_UNIX shared-instance listener/client-attach compatibility, including implicit shared local TCP sidecar coexistence with configured TCP/Backbone listeners, Unix client-attach reconnect after initial connect failures or later disconnects, TCP/Unix attach reconnect signals that re-synthesize tunnel state, and shared-instance one-hop transport wrapping, Pipe subprocess HDLC, UDP unicast/multicast with Python-style UDP `device` broadcast-address defaults and IPv4 broadcast socket sends, serial, KISS, AX.25 KISS, AutoInterface, LoRa/RNode with serial/TCP and feature-gated BLE radio-state query, blink, safe read/display/local-radio management through daemon RPC, guarded persistent/destructive RNode management through daemon RPC, feature-gated RNode BLE, VR-N76 KISS-over-BLE, the in-progress shared serial/TCP RNodeMulti baseline with nested vport virtual children plus startup probe validation for detect, firmware `>= 1.74`, platform, MCU, `CMD_INTERFACES`, configured hardware vports, selected-vport radio status bookkeeping, vport-aware transport and daemon/RPC management queueing through parent iface plus child `vport` selection, parent-level Python ID beacon fanout to outgoing subinterfaces, and live daemon/RPC `radio_status` refresh over the transport-side runtime schema with stream/probe state and last-error reporting, the in-progress shared-serial Weave WDCL/HDLC endpoint baseline with live daemon/RPC status refresh over the transport-side endpoint, display-frame, and CPU/task/memory stat schema, and the in-progress I2P SAM peer/connectable baseline with Python-compatible persisted private-destination key filenames and live daemon/RPC tunnel status refresh over the transport-side watchdog/counter schema. | I2P full production evidence, broader live Backbone production comparison evidence, full RNodeMulti prepared-host hardware validation/evidence, broader RNodeMulti production parity, Weave UI and hardware evidence, BLE RNode management hardware evidence, and prepared-host hardware evidence remain. |
| `RNS/Discovery.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Announce/path discovery plus live AutoInterface discovery and peer runtime. | Public bootstrap/discovery breadth remains narrower than Python. |
| `RNS/Resolver.py` | `crates/libs/rns-transport` | partial | Resolver helpers, cached lookup behavior, and restored path-table identity lookup from cached announces exist. | Full resolver/discovery surface parity is not established. |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | done | Required Reticulum primitives used by identities, packets, links, and receipts. | No confirmed parity blocker. |
| `RNS/Utilities/*` | `crates/apps/rns-tools` | partial | `rnx` is substantial; `rnsd` delegates to `reticulumd`; `rnstatus-rs` reports local daemon/interface and propagation peer status from RPC with JSON and human output, including configured endpoints for host/port, UDP target, Unix local socket, serial/KISS/RNode/Weave/VR-N76 devices, Pipe command, I2P SAM/peer count, and Auto group rows, plus Auto carrier/link-local, TCP/Backbone stream/listener, UDP, serial, KISS/AX.25 KISS, KISS TCP, BLE GATT, I2P, RNode/LoRa, RNodeMulti, Weave, and VR-N76 runtime summaries; `rnodeconf-rs` covers serial/TCP, feature-gated BLE, and RNodeMulti parent/vport RNode radio-state query, blink, safe read/display/local-radio commands, and guarded persistent/destructive management commands over daemon RPC. | Full equivalents for retired `rncp`, `rnid`, `rnir`, `rnpath`, `rnpkg`, and `rnprobe` remain absent; `rnodeconf-rs` is not a full Python `rnodeconf` equivalent; `rnstatus-rs` is local status only. |
| `CRNS/*` | `crates/apps/rns-tools` | partial | Selected command workflows exist. | The Python command ecosystem is not reproduced. |

## Interface Detail

Implemented interface families are active runtime code, not parser-only
placeholders:

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
  primary single-bind selector. A software TCP shared-instance smoke now proves
  strict daemon startup, loopback listener status, attach-client status,
  Python local MTU and bitrate alias reporting, fake shared-instance attach,
  and `rnstatus-rs` JSON/human output without another local Reticulum process.
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
  stale outbound route pruning after restart/removal, dynamic multicast/reverse
  announce source refresh after replacement, and Python-style fallback from unknown
  `multicast_address_type` values to `temporary`.
- Serial, TCP/Wi-Fi, and feature-gated BLE LoRa/RNode with startup probes,
  Python and Android-style selector aliases, configuration validation,
  telemetry, flow control, teardown, display-capable BLE external-framebuffer
  disable before shutdown, frame-level helpers for blink, Bluetooth control,
  display/NeoPixel controls, interference-avoidance control, Wi-Fi settings,
  config save/delete, firmware-update metadata, and ROM/EEPROM read/write/wipe
  requests, and live daemon/RPC `rnode_status` refresh plus compact
  `rnstatus-rs` human summaries for probe and radio state, with an opt-in
  prepared-host smoke harness for serial, TCP/Wi-Fi, or BLE RNode devices.
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
  incoming peer rows without requiring a prepared I2P router. The config parser accepts
  I2P-local IFAC aliases `ifac_netname` and `ifac_netkey`.
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
Known-path requests received on a roaming-mode interface are no longer answered
when the learned next-hop interface for that path is the same interface,
matching Python's roaming-interface loop suppression.
Restored path-table cached announces are now kept as lookup/cache material
rather than scheduled as fresh announce rebroadcasts at startup, while still
serving known-path responses. Shared-instance clients now skip local path-table
save and restore work like Python Reticulum.
Tunnel-only restored announces are also retained as cache material, so paths
restored when a tunnel reappears can answer later known-path requests with
direct `PATH_RESPONSE` packets.

Enabled unknown interface kinds still parse so operators can see them in daemon
status, but daemon startup marks them as failed with explicit
`unsupported interface kind` runtime metadata instead of silently dropping the
record.

`RNS/Interfaces/*` remains `partial` because parity is measured against the
whole Python family, not because the implemented interfaces are stubs. Backbone
now has Python selector/epoll and live Python Reticulum BackboneClientInterface
slow-reader probes for the same qualitative backpressure workload, plus focused
live Rust/Python Backbone channel, link-data, request/response, and resource
roundtrips in both directions, while broader Backbone production comparison
evidence remains pending. AutoInterface
now has daemon-side dynamic add/remove reconciliation for an active runtime
using the implemented diff plan plus discovery and data listener supervisors.
Zero-initial startup now keeps the polling reconciler and scheduler runtime
alive for later adopted devices, and the supervisors track replacement-stop
tasks so dynamically replaced listeners are drained during restart, removal, or
runtime shutdown. An opt-in Linux namespace prepared-host smoke now records
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
a real I2P router. The opt-in
prepared-host smoke can also require configured outbound peers to reach
`connected` state when `I2P_PEERS` is supplied. Its report explicitly records
whether the run proved only `sam_connectable_only` behavior or
`sam_connectable_with_outbound_peers` behavior, so no-peer runs are not
mistaken for outbound peer production parity. Prepared-host connected-peer
production evidence is still pending until the harness is run against a real
SAM router and reachable peer set.
Ordinary serial/TCP and feature-gated BLE `RNodeInterface` now refresh transport-side probe/radio
state into daemon/RPC `_runtime.lora.rnode_status`, and `rnstatus-rs` renders a
compact human summary for operators. Python `RNodeInterface` alias configs now
have daemon parse-to-bootstrap/status coverage as `lora` with
`_runtime.lora.rnode_status`. An opt-in prepared-host smoke harness now
records serial/TCP/BLE RNode lifecycle evidence under `target/rnode-hil/` with
bearer-scoped `evidence_scope` values (`prepared_host_serial_rnode`,
`prepared_host_tcp_rnode`, and `prepared_host_ble_rnode`) so one prepared
endpoint is not mistaken for broad hardware parity.
Display-capable BLE RNode shutdown now disables the external framebuffer before
radio-off/leave frames. Android configured RNode BLE reconnect now excludes
the failed configured peripheral from fallback scan matching, with shared alias
matching helpers and stable service-UUID fallback log context. Serial/TCP RNode
streams now expose a transport-local management dispatch handle that writes
pre-encoded KISS command frames through the live KISS runtime; feature-gated
BLE RNode streams expose the same management dispatch through the Nordic UART
write path with BLE chunking.
Radio-state query and blink dispatch are covered by local duplex/mock tests,
daemon `rnode_management` RPC dispatch, and `rnodeconf-rs` query/blink CLI
tests. Daemon RPC and `rnodeconf-rs` also queue safe config read, ROM read,
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
closed paths from lingering after failed dispatch. BLE management hardware
evidence and full Python `rnodeconf` parity remain pending.
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
including the accepted probe metadata. An opt-in
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
clears endpoint children. An opt-in prepared-host smoke harness records
connected serial evidence under `target/weave-hil/` and can optionally prove
the live `weaveconf-rs` remote-display enable/disable dispatch against that
connected device. Its report distinguishes `prepared_host_connected_serial`
evidence from `prepared_host_serial_discovery_only` bring-up evidence, while
broader prepared-host hardware evidence across device, firmware,
display/status payload, and operator-workflow combinations remains pending.

## Highest-Priority Gaps

1. Close remaining announce/path/discovery edge-policy differences beyond the
   cached remote path-response `PATH_RESPONSE` and roaming same-interface
   suppression slices.
2. Complete resolver/bootstrap behavior beyond cache-only restored path-table
   announce material, tunnel restored-cache lookup, and shared-instance
   path-table persistence suppression.
3. Capture broader prepared-host BLE/RNode lifecycle evidence across bearer,
   device, firmware, and radio combinations.
4. Capture I2P prepared-host connected-peer evidence when claiming outbound
   peer production parity.
5. Capture broader RNodeMulti prepared-host hardware validation across device,
   firmware, and radio combinations.
6. Implement real utility equivalents only where product demand justifies them.

## Evidence

- Workspace unit and integration tests cover core, transport, daemon, serial,
  BLE, LoRa, AutoInterface, link, channel, buffer, and resource behavior.
- `.github/workflows/python-interop.yml` runs pinned live Python channel/link/request/resource and
  LXMF compatibility scenarios plus Python selector/epoll and live Python
  Reticulum BackboneClientInterface slow-reader probes for Backbone
  backpressure evidence.
- Nightly mesh, soak, and embedded HIL workflows provide additional operational
  evidence, but do not promote unsupported interface families to `done`.
