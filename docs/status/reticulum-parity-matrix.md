# Reticulum Parity Matrix

Last reassessed: 2026-06-07

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
| `RNS/Reticulum.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Deployable daemon, configuration, persistence, RPC, graceful shutdown, and multiple live interfaces. | Python runtime/config mutation and interface breadth remain wider. |
| `RNS/Identity.py` | `crates/libs/rns-core` | done | Identity material, hashing, signing, encryption, recall, and key conversion. | No confirmed parity blocker. |
| `RNS/Destination.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Destination hashing, descriptors, announces, proof validation, ratchets, and known-key stability checks. | No confirmed parity blocker. |
| `RNS/Packet.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Framing, serialization, contexts, proofs, receipts, and header semantics. | No confirmed parity blocker. |
| `RNS/Transport.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Path and announce handling, link routing, resources, receipts, interface-aware sending, pacing, and duplicate suppression. | Remaining announce/path edge policy and full runtime behavior require live parity evidence. |
| `RNS/Link.py` | `crates/libs/rns-transport` | done | Establishment, proof validation, bound-interface enforcement, RTT-derived liveness, protocol close, and cleanup. | Continue live regression coverage; no confirmed blocker. |
| `RNS/Resource.py` | `crates/libs/rns-transport` | done | Bounded receive allocation, advertisement validation, retries, adaptive fragment scheduling, timeout/failure events, cancellation, and cleanup. | Split/segmented resources remain intentionally unsupported and rejected. |
| `RNS/Channel.py` | `crates/libs/rns-transport` | partial | Channel packet handling, retry scheduling, buffering, and live Rust/Python channel tests. | Full Python sequential-delivery and callback behavior is not yet demonstrated. |
| `RNS/Buffer.py` | `crates/libs/rns-core`, `crates/libs/rns-transport` | done | Packet buffers, readers/writers, and callback baseline. | No confirmed parity blocker. |
| `RNS/Interfaces/*` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | TCP client/server, UDP, serial, KISS, AutoInterface, LoRa/RNode, feature-gated RNode BLE, and VR-N76 KISS-over-BLE. | AX.25, Backbone, I2P, Local, Pipe, Weave, full RNode management, and prepared-host hardware evidence remain. |
| `RNS/Discovery.py` | `crates/libs/rns-transport`, `crates/apps/reticulumd` | partial | Announce/path discovery plus live AutoInterface discovery and peer runtime. | Public bootstrap/discovery breadth remains narrower than Python. |
| `RNS/Resolver.py` | `crates/libs/rns-transport` | partial | Resolver helpers and cached lookup behavior exist. | Full resolver/discovery surface parity is not established. |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | done | Required Reticulum primitives used by identities, packets, links, and receipts. | No confirmed parity blocker. |
| `RNS/Utilities/*` | `crates/apps/rns-tools` | partial | `rnx` is substantial; `rnsd` delegates to `reticulumd`; `rnstatus-rs` reports local daemon/interface status from RPC with JSON and human output. | Full equivalents for retired `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`, `rnpkg`, and `rnprobe` remain absent; `rnstatus-rs` is local status only. |
| `CRNS/*` | `crates/apps/rns-tools` | partial | Selected command workflows exist. | The Python command ecosystem is not reproduced. |

## Interface Detail

Implemented interface families are active runtime code, not parser-only
placeholders:

- TCP client and server, including fixed-MTU and KISS-framed client modes.
- UDP unicast and multicast with peer routing and multicast proof fallback.
- Held UDP multicast announces retain peer source metadata through delayed
  release, preserving virtual unicast route installation after announce limiting.
- Serial and serial KISS.
- AutoInterface discovery, authenticated peering, peer lifecycle, duplicate
  suppression, multicast announcements, data sockets, and transport bridging.
- Serial and TCP/Wi-Fi LoRa/RNode with startup probes, configuration
  validation, telemetry, flow control, and teardown.
- Feature-gated native RNode BLE and VR-N76 KISS-over-BLE.

Python-style interface-driven `tcp_server` startup now works from config
without Rust-only transport overrides.

`RNS/Interfaces/*` remains `partial` because parity is measured against the
whole Python family, not because the implemented interfaces are stubs.

Known but unsupported Python interface families such as `PipeInterface`,
`LocalInterface`, `I2PInterface`, `WeaveInterface`, and `BackboneInterface`
now fail config parsing with deterministic unsupported-family diagnostics
instead of silently loading as inert unknown interface entries.

## Highest-Priority Gaps

1. Prove complete channel ordering and callback behavior.
2. Close remaining announce/path/discovery edge-policy differences.
3. Complete resolver/bootstrap behavior.
4. Capture prepared-host BLE/RNode lifecycle evidence.
5. Decide and document support policy for missing interface families.
6. Implement real utility equivalents only where product demand justifies them.

## Evidence

- Workspace unit and integration tests cover core, transport, daemon, serial,
  BLE, LoRa, AutoInterface, link, and resource behavior.
- `.github/workflows/python-interop.yml` runs pinned live Python channel and
  LXMF compatibility scenarios.
- Nightly mesh, soak, and embedded HIL workflows provide additional operational
  evidence, but do not promote unsupported interface families to `done`.
