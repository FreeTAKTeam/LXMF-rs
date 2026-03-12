# Compatibility Matrix v2

Last updated: 2026-03-12

Reference baseline:

- Official Reticulum API Reference: <https://markqvist.github.io/Reticulum/manual/reference.html>
- Accessed: 2026-03-12
- Companion live-proof contract: `docs/contracts/external-client-interop-acceptance-v1.md`
- Detailed implementation tracker: `docs/plans/reticulum-parity-matrix.md`

## Compatibility Definition

For this repository, compatibility with the Reticulum protocol means:

1. Full interoperability with the reference implementation in real message
   exchange flows.
2. Sufficient functional parity with the published Reticulum reference API and
   behavior.

If both are true, the implementation may be described as `Reticulum`.
If either is false or unproven, it is not yet `Reticulum`; it is only a partial
or in-progress implementation.

Internal Rust SDK, RPC, or product-specific domain coverage is supporting
evidence only. Those surfaces do not define Reticulum compatibility by
themselves.

## Status Legend

- `verified`: implemented and backed by repo-local tests and/or reproducible
  interoperability evidence
- `partial`: meaningful implementation exists, but reference API coverage or
  interoperability proof is incomplete
- `missing`: no sufficient implementation or proof exists yet
- `n/a`: intentionally outside the Reticulum claim surface

## Claim Gate Matrix

| Gate | Required for a `Reticulum` claim | Current status | Notes |
| --- | --- | --- | --- |
| Live interoperability with the reference stack | yes | partial | Runbooks and acceptance criteria exist, but checked-in docs do not prove a passing release-gated Python reference/client interop run yet. |
| Functional parity with the published API reference | yes | partial | Core packet, destination, link, and transport primitives exist, but several published API sections are still missing or only internally mapped. |
| Internal Rust SDK/RPC contract coverage | no | partial | Important for this repo, but not sufficient to establish Reticulum compatibility. |

## Reticulum Reference API Coverage

| Reference slice | Representative API surface | Primary repo mapping | Status | Current gap summary |
| --- | --- | --- | --- | --- |
| Runtime bootstrap and node policy | `RNS.Reticulum` | `crates/apps/reticulumd`, `crates/libs/rns-transport`, `crates/libs/rns-rpc` | partial | Basic daemon/runtime exists, but the published shared-instance, remote-management, blackhole, and interface-discovery surface is not yet exposed or proven equivalent. |
| Identity and crypto primitives | `RNS.Identity` | `crates/libs/rns-core/src/identity.rs`, `crates/libs/rns-core/src/ratchets.rs` | partial | Core keys, signing, encryption, and hashing exist; file-based helpers, recall helpers, and exact reference-level public API coverage are incomplete. |
| Destination semantics | `RNS.Destination` | `crates/libs/rns-core/src/destination.rs`, `crates/libs/rns-transport/src/destination.rs` | partial | Addressing, announce validation, and ratchet support exist; callback registration, request-handler semantics, and proof-strategy parity are incomplete. |
| Packet encoding and send lifecycle | `RNS.Packet` | `crates/libs/rns-core/src/packet.rs`, `crates/libs/rns-transport/src/packet.rs`, `crates/libs/rns-transport/src/transport` | partial | Wire framing exists, but the reference object-level send/resend/stat access surface is not exposed as compatible public API. |
| Packet receipt lifecycle | `RNS.PacketReceipt` | `crates/libs/rns-transport/src/receipt.rs`, `crates/libs/rns-transport/src/transport/mod.rs` | missing | The repo has delivery receipt helpers, but not a public `PacketReceipt` compatibility object with timeout and callback semantics. |
| Link lifecycle and telemetry | `RNS.Link` | `crates/libs/rns-transport/src/destination/link.rs`, `crates/libs/rns-transport/src/transport/links.rs` | partial | Link establishment and encrypted link traffic exist; the published identify, telemetry, callback, channel, and lifecycle surface is only partially represented. |
| Request/reply receipt lifecycle | `RNS.RequestReceipt` | `crates/libs/rns-transport`, `crates/libs/lxmf-sdk` | missing | No public compatibility object currently mirrors request receipt status, progress, response, and completion semantics from the reference API. |
| Resource transfer lifecycle | `RNS.Resource` | `crates/libs/rns-transport/src/resource.rs`, `crates/libs/rns-transport/src/resource` | partial | Resource advertisements, requests, hashes, proofs, and transfer events exist; the public `Resource` facade and method-level parity are incomplete. |
| Channel messaging | `RNS.Channel.Channel`, `RNS.MessageBase` | `crates/libs/rns-transport/src/channel.rs` | partial | Envelope transport exists, but the reference message registration and `MessageBase` pack/unpack contract is not yet modeled as a compatibility surface. |
| Buffer and raw channel I/O | `RNS.Buffer`, `RNS.RawChannelReader`, `RNS.RawChannelWriter` | `crates/libs/rns-core/src/buffer.rs`, `crates/libs/rns-transport/src/buffer.rs` | partial | Low-level buffers exist, but the raw channel reader/writer compatibility API and bidirectional buffer helpers are not yet provided. |
| Transport path discovery and announce callbacks | `RNS.Transport` | `crates/libs/rns-transport/src/transport`, `crates/apps/reticulumd` | partial | Internal path tables and path requests exist, but public `register_announce_handler`, `has_path`, `hops_to`, `next_hop`, `next_hop_interface`, and `await_path` parity is incomplete. |
| Real transport interfaces | reference runtime behavior required for interop | `crates/libs/rns-transport/src/iface.rs`, `crates/apps/reticulumd/src/bin/reticulumd/interfaces` | partial | TCP/UDP/serial exist in core transport and BLE/LoRa/serial exist in the daemon, but interoperability proof across real reference-compatible interface behavior remains incomplete. |
| User-facing Reticulum tooling | `rnid`, `rnpath`, `rnprobe`, `rnsd`, related utilities | `crates/apps/rns-tools/src/bin`, `crates/apps/reticulumd` | partial | `rnsd` is a wrapper to `reticulumd`, `rnx` is a test harness, and several expected utility commands are currently stubs rather than reference-parity tools. |

## Required Exit Criteria

The repository may only claim to be `Reticulum` when all of the following are
true:

1. Every required row above is `verified`.
2. The live interoperability acceptance flow in
   `docs/contracts/external-client-interop-acceptance-v1.md` passes
   reproducibly.
3. `docs/plans/reticulum-parity-matrix.md` shows no unresolved required
   compatibility rows.
4. Release notes and README language avoid stronger compatibility claims than
   the verified evidence supports.

## Change Control

- Changes that add or remove Reticulum-facing behavior must update this file.
- New repo-local SDK or RPC features must not be described as Reticulum
  compatibility unless they also satisfy the reference API and interop gates.
- Downgrading any row from `verified` requires an ADR or migration note.
