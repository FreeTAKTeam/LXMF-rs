# Current Roadmap Status

Last updated: 2026-06-02

This document is the current source of truth for repository-wide delivery
status. Update this file first when parity status, release confidence, or the
active execution order changes.

Related documents:

- Execution board: `docs/plans/2026-03-19-python-compatibility-execution-board.md`
- Numbered compatibility backlog: `docs/plans/2026-03-18-rust-python-compat-issue-list.md`
- LXMF parity snapshot: `docs/status/lxmf-parity-matrix.md`
- Reticulum parity snapshot: `docs/status/reticulum-parity-matrix.md`

## Current Summary

- PR #215's `master` -> `main` integration branch is mergeable and clean, with
  the GitHub CI rollup green at head `0c4588c`.
- The `CI / unused-deps (pull_request)` failure was resolved by removing stale
  `reticulum-rs-rpc` dependency declarations; the matching local check
  `cargo +nightly udeps --workspace --all-targets` reports all dependencies in
  use.
- The repository is not blocked by broken builds. The main blockers are now
  remaining Python parity gaps, not CI or merge conflicts.

## What Is True Now

### Landed Baseline

The following compatibility foundation work is on `main` and should be treated
as the current baseline:

- buffer writer parity (`#110`)
- buffer callback parity (`#111`)
- resource lifecycle truth and generic-resource handling (`#112`)
- daemon receipt semantics for resource-backed sends (`#113`)
- honor LXMF delivery modes in the `reticulumd` bridge (`#114`)
- path tag lifetime parity (`#115`)

This means older planning notes that say `reticulumd` ignores requested LXMF
delivery modes are stale. Delivery-mode handling is no longer an open baseline
gap, even though deeper propagation-router parity remains open.

### Still Open

- Rust/Python live interop is now represented in `.github/workflows/python-interop.yml`
  for pinned Reticulum/LXMF references. The high-signal local gates are
  `python_channel_interop`, `python_paper_interop`, and `python_compat_matrix`.
  `crates/apps/lxmf-cli/tests/python_lxmd_remote_relay.rs` is also included for
  cross-implementation LXMD relay paths.
- `reticulumd` now defaults to local Unix RPC, treats TCP as opt-in, rejects
  unauthenticated remote TCP binds, handles graceful listener shutdown, and
  documents service-manager deployment in
  `docs/runbooks/reticulumd-operational-deployment.md`.
- Propagation-router behavior is still partial relative to Python LXMF.
- Daemon receipt status and peer-activity bookkeeping now preserve Python's
  distinction between transport `sent:*` states and final `delivered` receipts:
  send/resource completion records sent-only peer tx bookkeeping, while actual
  delivery receipts mark the outbound peer heard/delivered.
- RPC message listing now accepts bounded `limit`/`cursor` parameters and uses
  a stable `timestamp:id` cursor so same-second messages paginate without
  skipping or repeating records. Message and announce list handlers now report
  `next_cursor` only when a following page exists. The legacy message timestamp
  remains integer seconds; Python payload precision is still exposed through
  `_lxmf.timestamp_f64`.
- Outbound resource retry exhaustion and advertisement dispatch failure now emit
  explicit transport failure events, and tracked daemon LXMF resource sends are
  marked failed instead of leaving stale resource tracking after timeout.
- Explicit outbound resource cancellation is available through the transport
  API: it removes sender state, emits an `OutboundCancelled` event, and sends a
  `ResourceInitiatorCancel` packet over the link's bound interface.
- `reticulumd` now uses that cancellation path for tracked direct and
  propagated resource-backed sends when a message is cancelled after the
  resource transfer has started.
- Stamp, ticket, and propagation-stamp semantics are still partial, but inbound
  delivery-stamp validation now honors Python's stamp-cost flexibility window
  by accepting proof-of-work at `target_cost - flexibility`, and local
  propagated-message stamp metadata now uses the propagation
  `target_cost - stamp_cost_flexibility` acceptance floor.
  Outbound progress queries now treat terminal normal or propagation stamp work
  states as authoritative over stale `_lxmf.progress` metadata, and outbound
  stamp-cost queries now suppress stale target-cost metadata after terminal
  normal or propagation stamp work states.
- Peer/router/runtime parity remains partial.
- Reticulum interface breadth is still narrower than the Python reference, but
  KISS and LoRa/RNode are active implemented areas in the current branch. The
  daemon and transport crates now cover serial KISS
  framing/configuration, TCP/Wi-Fi KISS client startup, serial and TCP/Wi-Fi
  LoRa/RNode startup, feature-gated native RNode BLE startup, and feature-gated
  VR-N76 KISS-over-BLE startup. The active path includes Python-compatible KISS
  single-port command-byte decoding with port-nibble stripping, preserved full
  RNode command bytes, RNode startup probe frames, and short-term/long-term
  airtime-limit commands.
  Configured UDP multicast startup now uses the transport multicast path so
  peer-routing is registered for point-to-point replies discovered through the
  multicast interface.
  Python `AutoInterface` configuration is now parsed with Reticulum defaults,
  the Python-compatible discovery multicast address is derived in
  `rns-transport`, and reusable peering-token/IPv6-descoping, peer lifecycle,
  outbound multicast/reverse peering packet planning, authenticated discovery
  packet handling, spawned peer data-target planning on `data_port`,
  per-adopted-interface UDP listener binding targets, unicast/multicast
  discovery listener binding targets with Windows bind behavior, startup-plan
  aggregation for Python `final_init`, runtime gating for `final_init_done` and
  `online`, carrier-change runtime flag aggregation, platform
  interface-filter, link-local address selection, link-local replacement
  planning for adopted interfaces, and local multicast echo
  classification helpers match the Python discovery algorithm, including
  Python's first-hash-bytes peering packet comparison.
  Shared AutoInterface timing profiles now expose
  Python's announce, peer job, reverse-peering, initial discovery wait,
  multicast echo, duplicate suppression, and Android peering-timeout settings,
  and reusable discovery/deduplication state can be constructed from that
  profile. Pure peer-job plans now follow Python's maintenance order by
  removing timed-out peers before reverse peering, emitting reverse-peering
  packets only for still-live peers, and reporting interfaces without an
  initial multicast echo. A state-changing peer-job execution helper also
  removes stale peers, marks reverse-peering sends, and updates multicast
  carrier timeout state for live scheduler integration. Runtime
  `carrier_changed` aggregation now mirrors Python's flag for carrier
  lost/recovered events and link-local replacement. Multicast
  `peer_announce` scheduling now emits an immediate first packet per adopted
  interface and repeats on Python's announce interval. Multicast echo timeout
  and carrier-transition helpers now match Python's strict timeout boundary,
  and inbound multi-interface duplicate suppression matches Python's
  48-entry/0.75-second packet-hash window. Spawned peer inbound delivery
  decisions now reject unknown peers, suppress duplicate packets without
  refreshing peer state, and refresh known peers only on accepted packets.
  The daemon now enumerates operational OS link-local IPv6 candidates with
  `if-addrs`, applies the shared AutoInterface selector, and records the
  resulting adopted-device discovery/data listener startup plan plus initial
  multicast peer-announce send plan in `_runtime.auto`, including structured
  host/port/scope targets and planned send counts. The initial peer-announce
  bridge can resolve those targets through an injected interface-index lookup
  or the native `if-addrs` interface-index resolver and send the peering
  payloads through a supplied UDP socket. `_runtime.auto` records that native
  scope IDs come from `if-addrs` interface indexes. `_runtime.auto`
  now also reports planned unicast and multicast discovery socket bind targets,
  and the daemon has staged unicast and multicast discovery socket bind helpers.
  The multicast helper resolves link-scope group joins to interface indexes and
  binds on the unspecified address before joining the derived group. Bound
  discovery sockets now expose typed single-datagram receives with socket kind,
  interface, source, and raw payload metadata, and those datagrams can now be
  authenticated and classified into local echo, peer event, or invalid-token
  rejection outcomes through the shared AutoInterface discovery state. A
  cancellable daemon receive-loop primitive now owns bound discovery sockets,
  updates shared discovery state, and reports accepted/rejected receive events;
  `_runtime.auto` reports the planned discovery receive-loop count. The daemon
  also reports planned peer data socket binds, binds those `data_port` sockets,
  receives typed peer data datagrams, and classifies them through the shared
  known-peer and duplicate-suppression state. Enabled
  AutoInterface startup now binds native-scope discovery sockets, starts those
  receive loops, sends initial multicast peer-announce packets, starts the
  repeat multicast peer-announce scheduler, starts the peer-job scheduler,
  starts peer data receive loops, injects accepted peer-data packets into the
  normal transport ingress path through per-peer virtual interfaces, routes
  direct/broadcast transport sends back out over peer UDP data sockets, records
  `auto_discovery_runtime` counts, and reports complete AutoInterface runtime
  status when startup succeeds.
  RNode detect, firmware-version, platform, and MCU probe responses have typed
  parsers and validation helpers in `rns-transport`, and active LoRa
  streams record probe, reported radio configuration, and hardware error
  responses when present. Reported bandwidth, spreading factor, and coding rate
  now participate in startup validation and expose Python-compatible on-air
  bitrate calculation, and runtime RX/TX,
  RSSI, SNR, SNR-quality, airtime, channel-load, noise-floor, interference, PHY,
  CSMA, battery, temperature, random-byte, and display-platform stats use
  Python-compatible scaling, including Python's RNode battery state names and
  initial telemetry defaults. Inbound RNode KISS data frames clear retained
  per-packet RSSI/SNR telemetry like Python `process_incoming`. The interface
  also tracks reported radio online state, exposes display-platform
  framebuffer/display command frames including Python-compatible 8-byte
  framebuffer-write line frames, retains Python-sized
  framebuffer and display-read payloads, exposes Python's hard-reset command
  frame, treats Python's online ESP32 reset response as fatal, and retains the last fatal RNode
  command-response error for runtime visibility. A combined
  startup-response validator now folds retained fatal command errors, hardware
  identity, firmware, and reported radio-state validation into one result, and
  active streams invoke it after the RNode startup-response deadline. Startup
  validation failures and fatal command responses now tear down the active LoRa
  KISS stream for reconnect while malformed command responses remain non-fatal,
  and fresh streams clear stale RNode response state before collecting startup
  replies. `command_timeout_ms` is now accepted for RNode startup-response
  deadline tuning, and the protocol surface exposes Python's radio-state query
  frame plus the remaining RNode management command constants used by the
  reference interface. Active LoRa/RNode streams send Python-style radio-off
  plus leave-host commands on daemon or interface teardown. TCP/Wi-Fi RNode
  streams now send Python's idle activity detect probe after 3.5 seconds
  without a successful write, RNode radio-lock command responses are recorded
  with the reported radio state, and RNode config validation accepts Python's
  full `7800..=1625000` Hz LoRa bandwidth range plus Python's frequency and
  TX-power ranges at parse time. KISS startup now always emits
  Python's `CMD_READY 0x01` setup command, and KISS READY flow-control startup mirrors
  Python by allowing the first outbound packet after configuration
  before locking on later writes, and missed READY frames unlock after Python's
  five-second timeout. Inbound KISS escape decoding now mirrors Python's
  lenient handling of unknown or trailing escape bytes, and oversized inbound
  KISS payloads are capped to MTU instead of rejected. Stale partial inbound
  KISS frames are discarded after the Python read timeout before later bytes
  are decoded. KISS station-ID beacons now match Python's missing-callsign
  empty payload behavior and 15-byte minimum payload padding while RNode station-ID beacons remain unpadded. Python
  `KISSInterface` configs that omit `speed` now inherit Python's `9600` baud
  default during alias normalization, and serial Python `RNodeInterface`
  configs that omit an explicit baud rate inherit Python's `115200` baud
  default while `ble://` RNode ports are accepted as native BLE RNode ports and
  never opened as serial devices. Without the `rnode-ble` feature, daemon
  startup records an explicit failed status for those ports. The transport
  layer now exposes Python's generic
  RNode BLE Nordic UART UUID profile, write-without-response defaults, raw KISS
  notification decoding, startup writes, READY flow-control session state, stale
  partial-notification timeout handling, outbound station-ID beacon writes, and
  oversized-outbound rejection plus max-write-length chunking before backend
  writes, non-READY command-response preservation plus exposed RNode monitor
  state for startup/radio validation, own station-ID beacon suppression, and a feature-gated native
  `btleplug` RNode BLE backend for scan, connect, Nordic UART characteristic
  discovery, subscription, write, notification, cleanup, and identifier
  matching behind a backend-neutral runtime contract. Feature-gated daemon
  `RNodeInterface` `ble://` startup now builds that native backend, spawns the
  interface manager task, forwards outbound packets as raw KISS writes, polls
  BLE notifications into inbound packets, emits RNode station-ID beacons,
  appends the same RNode detect/radio-configuration command frames as
  serial/TCP startup, and writes radio-off plus leave-host frames during BLE
  cleanup. BLE startup and fatal command-response validation now feeds the same
  RNode protocol state used by serial/TCP startup, but higher-level RNode
  management CLI/configuration operations over BLE remain incomplete. BLE
  connect timeout and RNode
  command-response timeout are kept distinct: native BLE connect defaults to
  five seconds and uses `ble_connect_timeout_ms` overrides, while RNode startup
  validation defaults to Python's 1500 ms command deadline and uses
  `command_timeout_ms` overrides.
  Python `RNodeInterface` aliases now require explicit frequency,
  bandwidth, spreading-factor, and coding-rate radio parameters instead of
  silently inheriting Rust-native LoRa region defaults. Daemon
  bootstrap status still does not synchronously fail on RNode response
  validation. A feature-gated
  `vrn76_kiss_ble` daemon interface now covers the VR-N76 UUID profile,
  write-with-response KISS frames, outbound Benshi TNC fragmentation by
  configured BLE write length, indications, READY flow control, KISS
  station-ID beacon config/emission with Python empty-payload padding, own-beacon suppression,
  stale partial KISS frame timeout handling, and native BLE adapter startup.
  OS Bluetooth adapter setup, permissions, pairing, and bonding are host responsibilities outside this repository;
  hardware-backed lifecycle evidence must be captured on a prepared host.
- Parser-only `rns-tools` utility placeholders have been retired from the
  release surface. Utility parity remains incomplete until real equivalents for
  the retired Python-style commands are implemented.
- Migration-era legacy crates and router/runtime stubs have been removed from
  the repository surface; active code must stay in the workspace crates listed
  in `Cargo.toml`.
- The module-size gate is green after splitting `lxmd` launch/config helpers,
  `rnx` TCP/BLE/scenario helpers, RPC event/helper/status tests, LXMF wire tests,
  SDK backend/client/app-control/node tests, and transport resource/interface/
  path/tunnel helpers out of oversized modules.

## Active Execution Order

1. Keep architecture and boundary gates trustworthy.
2. Keep the pinned Rust/Python interop workflow green and extend it when new
   compatibility rows become supported.
3. Align `README.md`, `docs/runbooks/release-readiness.md`, and GitHub CI with
   the same definition of "green".

## Update Rules

- Update this file in the same PR that changes project-wide status claims.
- When a historical planning note disagrees with this file, treat the planning
  note as stale until it is refreshed.
- Do not mark parity items as complete here unless the behavior is implemented
  in active workspace code and backed by non-ignored evidence.
