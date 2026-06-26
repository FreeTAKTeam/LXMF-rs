# Reticulum Parity Matrix

Last reassessed: 2026-06-26

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
| `RNS/Transport.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Path and announce handling, link routing, resources, receipts, interface-aware sending, pacing, and duplicate suppression. | Remaining announce/path edge policy and full runtime behavior require live parity evidence. |
| `RNS/Link.py` | `crates/libs/rns-transport` | done | Establishment, proof validation, bound-interface enforcement, RTT-derived liveness, protocol close, and cleanup. | Continue live regression coverage; no confirmed blocker. |
| `RNS/Resource.py` | `crates/libs/rns-transport` | done | Bounded receive allocation, advertisement validation, retries, adaptive fragment scheduling, timeout/failure events, cancellation, and cleanup. | Split/segmented resources remain intentionally unsupported and rejected. |
| `RNS/Channel.py` | `crates/libs/rns-transport` | done | Channel packet handling, retry scheduling, buffering, ordered receive delivery, callback ordering/short-circuit/panic containment, delivery-on-proof, timeout retry, exhaustion cleanup, and live Rust/Python channel sequence tests. | No confirmed channel parity blocker. |
| `RNS/Buffer.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Packet buffers, readers/writers, and callback baseline. | No confirmed parity blocker. |
| `RNS/Interfaces/*` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | TCP client/server, Backbone TCP/HDLC listener/client compatibility with Backbone MTU defaults, Reticulum-style Backbone socket tuning for Backbone client and accepted listener streams (`TCP_NODELAY`, Linux/Android `SO_KEEPALIVE`, TCP keepalive idle/interval/count, and TCP user timeout), and Backbone-only HDLC liveness keepalives/stale/read-timeout reconnects, LocalInterface TCP-loopback listener/client-attach plus Unix filesystem and Linux/Android abstract AF_UNIX shared-instance listener/client-attach compatibility, including Unix client-attach reconnect after initial connect failures or later disconnects, TCP/Unix attach reconnect signals that re-synthesize tunnel state, and shared-instance one-hop transport wrapping, Pipe subprocess HDLC, UDP unicast/multicast with Python-style UDP `device` broadcast-address defaults and IPv4 broadcast socket sends, serial, KISS, AX.25 KISS, AutoInterface, LoRa/RNode, feature-gated RNode BLE, VR-N76 KISS-over-BLE, the in-progress shared serial/TCP RNodeMulti baseline with nested vport virtual children plus startup probe validation for detect, firmware `>= 1.74`, platform, MCU, `CMD_INTERFACES`, configured hardware vports, selected-vport radio status bookkeeping, parent-level Python ID beacon fanout to outgoing subinterfaces, and live daemon/RPC `radio_status` refresh over the transport-side runtime schema with stream/probe state and last-error reporting, the in-progress shared-serial Weave WDCL/HDLC endpoint baseline with live daemon/RPC status refresh over the transport-side endpoint, display-frame, and CPU/task/memory stat schema, and the in-progress I2P SAM peer/connectable baseline with Python-compatible persisted private-destination key filenames and live daemon/RPC tunnel status refresh over the transport-side watchdog/counter schema. | I2P full production evidence, remaining Python Backbone epoll/event-loop parity, full RNodeMulti prepared-host hardware validation/evidence, broader RNodeMulti production parity, Weave UI and hardware evidence, full RNode management, and prepared-host hardware evidence remain. |
| `RNS/Discovery.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Announce/path discovery plus live AutoInterface discovery and peer runtime. | Public bootstrap/discovery breadth remains narrower than Python. |
| `RNS/Resolver.py` | `crates/libs/rns-transport` | partial | Resolver helpers and cached lookup behavior exist. | Full resolver/discovery surface parity is not established. |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | done | Required Reticulum primitives used by identities, packets, links, and receipts. | No confirmed parity blocker. |
| `RNS/Utilities/*` | `crates/apps/rns-tools` | partial | `rnx` is substantial; `rnsd` delegates to `reticulumd`; `rnstatus-rs` reports local daemon/interface and propagation peer status from RPC with JSON and human output. | Full equivalents for retired `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`, `rnpkg`, and `rnprobe` remain absent; `rnstatus-rs` is local status only. |
| `CRNS/*` | `crates/apps/rns-tools` | partial | Selected command workflows exist. | The Python command ecosystem is not reproduced. |

## Interface Detail

Implemented interface families are active runtime code, not parser-only
placeholders:

- TCP client and server, including fixed-MTU and KISS-framed client modes.
- BackboneInterface and BackboneClientInterface config compatibility over the
  existing TCP/HDLC runtime, including listener/client alias handling and
  Backbone's larger default MTU. Backbone listener child streams and outbound
  Backbone client streams apply Reticulum-style socket tuning through a
  dedicated hook: `TCP_NODELAY` on every platform and, on Linux/Android,
  `SO_KEEPALIVE`, TCP keepalive idle/interval/count, and TCP user timeout.
  Backbone streams also opt into the shared HDLC liveness watchdog, emitting
  idle keepalives, marking stale reads, and reconnecting after read timeout
  without changing ordinary TCP client/server defaults.
- LocalInterface TCP-loopback listener/client-attach plus Unix filesystem and
  Linux/Android abstract AF_UNIX shared-instance listener/client-attach
  compatibility over the existing stream/HDLC runtime, including Python's
  default `127.0.0.1:37428` endpoint, `@rns/<instance_name>` Unix naming, and
  262144-byte local MTU. Unix client-attach retries after initial connect
  failures and reconnects after stream disconnects; TCP and Unix attach
  reconnect signals re-synthesize tunnel state through `reticulumd`, and
  attached shared-instance clients wrap one-hop outbound packets in transport
  headers before handing them to the shared instance.
- PipeInterface subprocess stdin/stdout transport with Python-style command
  parsing, HDLC packet framing, respawn delay, and default MTU.
- UDP unicast and multicast with peer routing, multicast proof fallback,
  Python-style `device` broadcast-address defaults via host interface lookup,
  and IPv4 broadcast socket sends.
- Serial, serial KISS, and AX.25 KISS with Python-compatible AX.25 UI header
  wrapping over the serial KISS runtime.
- AutoInterface discovery, authenticated peering, peer lifecycle, duplicate
  suppression, multicast announcements, data sockets, transport bridging, and
  live carrier-runtime status reporting.
- Serial and TCP/Wi-Fi LoRa/RNode with startup probes, configuration
  validation, telemetry, flow control, teardown, and live daemon/RPC
  `rnode_status` refresh plus compact `rnstatus-rs` human summaries for probe
  and radio state, with an opt-in prepared-host smoke harness for serial or
  TCP/Wi-Fi RNode devices.
- Shared serial/TCP RNodeMulti baseline with nested vport subinterfaces,
  `CMD_SEL_INT` KISS vport selection, direct routing to virtual child
  interfaces, broadcast fanout only to outgoing children, and startup probe
  validation for detect, firmware `>= 1.74`, platform, MCU,
  `CMD_INTERFACES` discovery, hardware-reported configured vports, and
  selected-vport radio command/status bookkeeping. Parent-level Python
  `id_callsign`/`id_interval` settings fan out raw callsign ID beacons on
  outgoing subinterfaces after first traffic. Daemon/RPC snapshots refresh over
  the `radio_status` runtime metadata schema, including stream/probe state and
  last-error reporting, with an opt-in prepared-host smoke harness for serial
  or TCP RNodeMulti devices.
- Shared serial Weave baseline with WDCL over HDLC framing, discovery
  handshake response, endpoint event learning, virtual peer child interfaces,
  inbound endpoint packet routing, direct endpoint command writes,
  target-scoped remote-display frame capture, CPU/task/memory stat parsing, and
  transport-side status bookkeeping refreshed into daemon/RPC
  `_runtime.weave.status`, with an opt-in prepared-host smoke harness for
  connected serial Weave devices.
- I2P SAM baseline, with transient stream sessions, `.i2p` name lookup, HDLC
  framing, virtual peer child interfaces, direct peer sends, broadcast fanout
  across configured peers, `STREAM ACCEPT` connectable sessions, and private
  destination key persistence under the daemon storage root by default or under
  explicit `state_path`/`storagepath` when configured,
  using Python-compatible hashed `.i2p` filenames with old-format key reuse and
  identity-bound new-format key names for generated destinations. Startup
  metadata reports the derived `.b32.i2p` endpoint for persisted keys and keys
  generated during startup, plus transport-side tunnel state, keepalive, stale,
  read-timeout, per-peer counter bookkeeping, and bounded closed-incoming-peer
  history refreshed into daemon/RPC `tunnel_status` runtime metadata.
- Feature-gated native RNode BLE and VR-N76 KISS-over-BLE.

Python-style interface-driven `tcp_server` startup now works from config
without Rust-only transport overrides.

`RNS/Interfaces/*` remains `partial` because parity is measured against the
whole Python family, not because the implemented interfaces are stubs.

`I2PInterface` is tracked as an in-progress family: configured outbound peers
and connectable sessions can run through SAM, and transport-side tunnel
watchdog/status bookkeeping is refreshed into daemon/RPC interface status.
Private destination keys now follow Python's default daemon-storage injection
and hashed key-file naming, including old-format fallback when an existing
Python key is present. `rnstatus-rs` human output summarizes the live I2P
tunnel status for operators. Prepared-host production evidence is still
pending.
Ordinary serial/TCP `RNodeInterface` now refreshes transport-side probe/radio
state into daemon/RPC `_runtime.lora.rnode_status`, and `rnstatus-rs` renders a
compact human summary for operators. An opt-in prepared-host smoke harness now
records serial/TCP RNode lifecycle evidence under `target/rnode-hil/`; BLE
live-status refresh and BLE prepared-host evidence remain pending.
`RNodeMultiInterface` is tracked separately as an in-progress family: the
shared serial/TCP vport routing slice exists and startup validates detect,
firmware `>= 1.74`, platform, MCU, `CMD_INTERFACES`, and hardware-reported
configured vports. Selected-vport radio status bookkeeping and live daemon/RPC
`radio_status` refresh exist, including stream/probe state and last-error
reporting, and `rnstatus-rs` renders a compact human summary of that state.
An opt-in prepared-host smoke harness now records serial/TCP RNodeMulti
evidence under `target/rnode-multi-hil/`. Full prepared-host hardware
validation across devices and broader production parity are still pending.
`WeaveInterface` is also tracked as an in-progress family: WDCL/HDLC endpoint
packet routing, target-scoped display-frame capture, CPU/task/memory stat
parsing, daemon/RPC status refresh, and compact `rnstatus-rs` human summaries
exist. An opt-in prepared-host smoke harness records connected serial evidence
under `target/weave-hil/`, while full UI integration and broader prepared-host
hardware evidence remain pending.

## Highest-Priority Gaps

1. Close remaining announce/path/discovery edge-policy differences.
2. Complete resolver/bootstrap behavior.
3. Capture prepared-host BLE/RNode lifecycle evidence.
4. Capture I2P prepared-host evidence, or explicitly document its product
   boundary.
5. Complete or explicitly defer RNodeMulti prepared-host hardware validation
   and broader production parity.
6. Implement real utility equivalents only where product demand justifies them.

## Evidence

- Workspace unit and integration tests cover core, transport, daemon, serial,
  BLE, LoRa, AutoInterface, link, channel, buffer, and resource behavior.
- `.github/workflows/python-interop.yml` runs pinned live Python channel and
  LXMF compatibility scenarios.
- Nightly mesh, soak, and embedded HIL workflows provide additional operational
  evidence, but do not promote unsupported interface families to `done`.
