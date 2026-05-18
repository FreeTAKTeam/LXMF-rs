# Reticulum Parity Matrix

Status: historical parity snapshot; check `docs/status/current-roadmap.md` for
current repo-wide status before relying on this file for active execution order.

Last reassessed: 2026-03-10 (`cargo test -p reticulum-rs-rpc --lib`, `cargo test -p reticulum-rs-transport --lib`, `cargo test -p lxmf-wire --lib`)

Status legend: `not-started` | `partial` | `done`

`done` means behavior is present in the active workspace and backed by active code/tests, not only by planning docs or migration-only crates under `crates/internal/`.

Naming note: this matrix keeps workspace paths in the table for code-navigation
clarity. The corresponding published crate names are `reticulum-rs-core`,
`reticulum-rs-transport`, and `reticulum-rs-rpc`.

| Python Surface | Rust Surface | Status | Notes |
| --- | --- | --- | --- |
| `RNS/Reticulum.py` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Usable daemon/runtime exists, but overall runtime/config/interface breadth is narrower than the Python reference. |
| `RNS/Identity.py` | `crates/libs/rns-core` | done | Identity material, hashing, signing, and key conversion are implemented in the active workspace. |
| `RNS/Destination.py` | `crates/libs/rns-core` + `crates/libs/rns-transport` | done | Destination hashing, announces, and destination descriptors are implemented and tested. |
| `RNS/Packet.py` | `crates/libs/rns-core` + `crates/libs/rns-transport` | done | Packet framing, serialization, and header semantics are present in active crates. |
| `RNS/Transport.py` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Core path/announce/resource/link transport exists, but full reference behavior and runtime policy parity are incomplete. |
| `RNS/Link.py` | `crates/libs/rns-transport` | done | Link establishment, adaptive keepalive/stale timing, Python-style activity timers, protocol `LinkClose`, and link-scoped cleanup are implemented and covered by active tests plus the live Rust/Python compatibility matrix. |
| `RNS/Interfaces/*` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Active support is limited to `tcp_client`, `tcp_server`, `udp`, `serial`, `ble_gatt`, and startup-only `lora`; Python interface families such as Auto, AX.25, Backbone, I2P, KISS, Local, Pipe, RNode, and Weave are absent. |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | done | Active workspace implements the required crypto primitives used by Reticulum packets and identities. |
| `RNS/Resource.py` | `crates/libs/rns-transport` | done | Resource sender/receiver/manager flow is implemented and covered by active tests. |
| `RNS/Channel.py` | `crates/libs/rns-transport` | partial | Channel primitives exist, but this repo does not yet demonstrate full behavioral parity with the Python sequential delivery surface. |
| `RNS/Buffer.py` | `crates/libs/rns-core` + `crates/libs/rns-transport` | done | Buffer and packet buffer handling are implemented in the active crates. |
| `RNS/Discovery.py` | `crates/libs/rns-transport` + `crates/apps/reticulumd` | partial | Announce/path discovery exists, but full bootstrap and public-interface discovery parity is not complete. |
| `RNS/Resolver.py` | `crates/libs/rns-transport` | partial | Resolver utilities exist, but parity with the Python resolver/discovery surface is incomplete. |
| `RNS/Utilities/*` | `crates/apps/rns-tools` | partial | `rnx` is substantial and `rnsd` delegates to `reticulumd`; parser-only utility placeholders such as `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`, `rnpkg`, `rnprobe`, and `rnstatus` have been retired until real equivalents exist. |
| `CRNS/*` | `crates/apps/rns-tools` | partial | The command surface is not yet equivalent to the Python utility/tooling ecosystem. |

## Confirmed Gaps

- Interface parity is incomplete. The Python reference includes more built-in interface types than the active Rust daemon starts or validates.
- Utility parity is incomplete. Unsupported Python-style utility binaries are no
  longer shipped as no-op placeholders; real Rust equivalents still need to be
  implemented before claiming Python utility parity.
- Runtime/config parity is incomplete. The Rust daemon has a narrower set of supported live mutations and startup semantics than the Python reference.
- `lxmd` TCP server parity is partial. Python-style interface-driven `tcp_server` startup now works from config without Rust-only transport overrides, but broader launcher/runtime parity is still incomplete.
- Discovery/bootstrap parity is incomplete. Core announce/path logic exists, but the higher-level interface/discovery story is still narrower than the Python implementation.

## Reassessment Summary

- Core protocol primitives are in substantially better shape than the surrounding runtime/tooling surface.
- The active workspace is closest to parity in `reticulum-rs-core` and the lower-level parts of `reticulum-rs-transport`.
- The largest Reticulum gaps are interface breadth, operational daemon behavior, and utility/CLI parity.
