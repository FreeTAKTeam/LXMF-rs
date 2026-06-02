# Reticulum Parity Matrix

Status: historical parity snapshot; check `docs/status/current-roadmap.md` for
current repo-wide status before relying on this file for active execution order.

Last reassessed: 2026-06-02 (PR #215 GitHub CI rollup green at `0c4588c`; local `cargo +nightly udeps --workspace --all-targets`)

Status legend: `not-started` | `partial` | `done`

`done` means behavior is present in the active workspace and backed by active code/tests, not only by planning docs or migration-only crates under `crates/internal/`.

Naming note: this matrix keeps workspace paths in the table for code-navigation
clarity. The corresponding published crate names are `reticulum-rs-core`,
`reticulum-rs-transport`, and `reticulum-rs-rpc`.

As of 2026-06-02, KISS and LoRa/RNode are implemented interface areas in the
active branch rather than absent placeholders. The remaining `partial` status
for `RNS/Interfaces/*` is about reference breadth and operational completeness:
Python still ships additional interface families and wider management/tooling
behavior than this repository currently starts, validates, and documents.

| Python Surface | Rust Surface | Status | Notes |
| --- | --- | --- | --- |
| `RNS/Reticulum.py` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Usable daemon/runtime exists, but overall runtime/config/interface breadth is narrower than the Python reference. |
| `RNS/Identity.py` | `crates/libs/rns-core` | done | Identity material, hashing, signing, and key conversion are implemented in the active workspace. |
| `RNS/Destination.py` | `crates/libs/rns-core` + `crates/libs/rns-transport` | done | Destination hashing, announces, and destination descriptors are implemented and tested. |
| `RNS/Packet.py` | `crates/libs/rns-core` + `crates/libs/rns-transport` | done | Packet framing, serialization, and header semantics are present in active crates. |
| `RNS/Transport.py` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Core path/announce/resource/link transport exists, but full reference behavior and runtime policy parity are incomplete. |
| `RNS/Link.py` | `crates/libs/rns-transport` | done | Link establishment, adaptive keepalive/stale timing, Python-style activity timers, protocol `LinkClose`, and link-scoped cleanup are implemented and covered by active tests plus the live Rust/Python compatibility matrix. |
| `RNS/Interfaces/*` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Active support includes `tcp_client`, `tcp_server`, `udp`, `serial`, `ble_gatt`, serial `kiss`, TCP/Wi-Fi `kiss_tcp_client`, serial and TCP/Wi-Fi LoRa/RNode-style radio startup when a `lora` device is configured, Python-compatible `TCPClientInterface`, `TCPServerInterface`, `UDPInterface`, `SerialInterface`, `KISSInterface`, and `RNodeInterface` config aliases, Python `AutoInterface` config parsing/status defaults plus discovery multicast address, peering-token, outbound multicast/reverse peering packet planning, multicast peer-announce scheduling, discovery listener binding targets, per-adopted-interface UDP listener binding targets, daemon-side link-local OS interface enumeration/adoption and startup-plan reporting with structured host/port/scope initial peer-announce send metadata, `_runtime.auto` unicast/multicast discovery socket bind-target reporting, staged unicast and multicast discovery socket bind helpers with injected interface-index scope resolution, typed discovery datagram receive bridging from bound discovery sockets, a supplied-UDP-socket peer-announce send bridge with injected interface-index scope resolution, Python `final_init` startup-plan aggregation and runtime gating, spawned peer data-target and inbound-delivery planning on `data_port`, authenticated discovery packet handling with first-hash-bytes payload comparison, peer-lifecycle and peer-job execution helpers, Python-compatible timing profile with state constructors, platform interface-filter, link-local address selection and replacement planning, local multicast echo classification, multicast echo carrier-transition helpers, runtime `carrier_changed` flag aggregation, multi-interface inbound duplicate suppression helpers, live AutoInterface multicast socket/per-peer runtime with transport ingress injection and direct/broadcast peer UDP routing, configured UDP multicast startup with transport peer-routing, common Reticulum `outgoing` transmit suppression, common per-interface `bitrate`/`announce_cap` announce pacing, `TCPClientInterface` `fixed_mtu` carry-through into the plain TCP runtime, `TCPClientInterface` `kiss_framing = true` routing onto Rust `kiss_tcp_client`, Python-compatible single-port `KISSInterface` command-byte port-nibble stripping while RNode command decoding preserves full command bytes, Python-compatible RNode startup probe frames, KISS/RNode `id_callsign` and `id_interval` station-ID beacons including Python `KISSInterface` 15-byte beacon padding, VR-N76 KISS-over-BLE station-ID config/emission, outbound Benshi TNC fragmentation by configured BLE write length, own-beacon suppression, and stale partial KISS frame timeout handling, RNode `command_timeout_ms` startup-response deadline tuning, RNode radio-state query frame and management command constants, RNode radio-off plus leave-host teardown commands, TCP/Wi-Fi RNode idle activity detect probes, generic RNode BLE Nordic UART profile constants/defaults and raw-KISS runtime/session state in `rns-transport` including outbound station-ID beacon writes, oversized-outbound rejection, max-write-length BLE chunking before backend writes, command-response preservation plus exposed RNode monitor state for startup/radio validation, stale partial notification timeout, own-beacon suppression, feature-gated native RNode BLE scan/connect/subscribe/write/notification plumbing, and feature-gated daemon `RNodeInterface` `ble://` startup that spawns the native BLE KISS interface, emits RNode detect/radio-configuration startup frames, validates startup and fatal command responses, and sends radio-off plus leave-host shutdown frames, RNode `flow_control` default-off behavior with explicit READY flow-control enablement, status recording and validation helpers for RNode detect/firmware/platform/MCU probe responses, reported radio configuration responses, and radio-lock responses, connection-scoped RNode response-state reset, combined RNode startup-response validation with active-stream deadline enforcement, Python-compatible RNode reported-bitrate calculation, runtime radio stats including channel, PHY, CSMA, battery state names, initial telemetry defaults, inbound packet RSSI/SNR clearing, temperature, random-byte, framebuffer/display-read command frames and payloads, framebuffer-write 8-byte line frames, hard-reset command frames, and display-platform telemetry, reported radio online-state tracking, online ESP32 reset handling, fatal RNode command-error retention, fatal command-response stream teardown, hardware error classification, short-term/long-term airtime-limit commands, and feature-gated `vrn76_kiss_ble` startup for VR-N76 KISS-over-BLE devices. VR-N76 and RNode BLE hardware lifecycle evidence must be captured on a prepared host, but OS Bluetooth adapter setup, permissions, pairing, and bonding are outside this repository's responsibility. Still-missing or partial Python interface families include AX.25, Backbone, I2P, Local, Pipe, full RNode management, and Weave. |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | done | Active workspace implements the required crypto primitives used by Reticulum packets and identities. |
| `RNS/Resource.py` | `crates/libs/rns-transport` | done | Resource sender/receiver/manager flow is implemented and covered by active tests. |
| `RNS/Channel.py` | `crates/libs/rns-transport` | partial | Channel primitives exist, but this repo does not yet demonstrate full behavioral parity with the Python sequential delivery surface. |
| `RNS/Buffer.py` | `crates/libs/rns-core` + `crates/libs/rns-transport` | done | Buffer and packet buffer handling are implemented in the active crates. |
| `RNS/Discovery.py` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Announce/path discovery exists, but full bootstrap and public-interface discovery parity is not complete. |
| `RNS/Resolver.py` | `crates/libs/rns-transport` | partial | Resolver utilities exist, but parity with the Python resolver/discovery surface is incomplete. |
| `RNS/Utilities/*` | `crates/apps/rns-tools` | partial | `rnx` is substantial and `rnsd` delegates to `reticulumd`; parser-only utility placeholders such as `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`, `rnpkg`, `rnprobe`, and `rnstatus` have been retired until real equivalents exist. |
| `CRNS/*` | `crates/apps/rns-tools` | partial | The command surface is not yet equivalent to the Python utility/tooling ecosystem. |

## Confirmed Gaps

- Interface parity is incomplete, but KISS and LoRa/RNode should no longer be
  described as unimplemented. Active support now covers serial KISS,
  TCP/Wi-Fi KISS client startup, serial and TCP/Wi-Fi LoRa/RNode startup,
  feature-gated RNode BLE startup, and feature-gated VR-N76 KISS-over-BLE
  startup. Remaining gaps are the Python interface families not yet covered
  here, host/hardware lifecycle evidence for BLE/RNode devices, and broader
  operational management behavior.
- AutoInterface live daemon runtime is implemented. The daemon binds discovery
  and peer-data sockets with native scoped IPv6 resolution from `if-addrs`,
  receives and authenticates discovery datagrams, classifies peer-data packets,
  starts discovery/data receive loops plus multicast peer-announce and peer-job
  schedulers, injects accepted peer-data packets into transport through
  per-peer virtual interfaces, and routes direct/broadcast transport sends over
  peer UDP data sockets. Hardware, OS interface availability, and platform
  Bluetooth or network setup remain host responsibilities.
- Utility parity is incomplete. Unsupported Python-style utility binaries are no
  longer shipped as no-op placeholders; real Rust equivalents still need to be
  implemented before claiming Python utility parity.
- Stamp/ticket parity is incomplete, but inbound delivery-stamp validation now
  applies the Python-compatible stamp-cost flexibility floor when enforcement is
  active.
- Runtime/config parity is incomplete. The Rust daemon has a narrower set of supported live mutations and startup semantics than the Python reference.
- `lxmd` TCP server parity is partial. Python-style interface-driven `tcp_server` startup now works from config without Rust-only transport overrides, but broader launcher/runtime parity is still incomplete.
- Discovery/bootstrap parity is incomplete. Core announce/path logic exists, but the higher-level interface/discovery story is still narrower than the Python implementation.

## Reassessment Summary

- Core protocol primitives are in substantially better shape than the surrounding runtime/tooling surface.
- The active workspace is closest to parity in `reticulum-rs-core` and the lower-level parts of `reticulum-rs-transport`.
- The largest Reticulum gaps are the remaining interface-family breadth,
  operational daemon behavior, and utility/CLI parity.
