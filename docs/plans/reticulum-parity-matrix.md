# Reticulum Reference API Parity Matrix

Last reviewed: 2026-03-12

Reference baseline:

- Official Reticulum API Reference: <https://markqvist.github.io/Reticulum/manual/reference.html>
- Accessed: 2026-03-12
- Compatibility decision rule: `docs/contracts/compatibility-matrix.md`
- Live interop acceptance gate: `docs/contracts/external-client-interop-acceptance-v1.md`

## Purpose

This file is the execution tracker for Reticulum-facing compatibility work.
It is intentionally keyed to the published reference API surface instead of to
internal crate names alone.

A row is only `verified` when the repository contains both:

1. An implementation that covers the required behavior.
2. A repeatable proof artifact, test, or interoperability run that demonstrates
   that behavior.

Status legend:

- `verified`
- `partial`
- `missing`

## Current Matrix

| Reference slice | Published API members | Primary repo mapping | Status | Proof currently in repo | Next required work |
| --- | --- | --- | --- | --- | --- |
| Runtime bootstrap and policy | `RNS.Reticulum`, `get_instance()`, `transport_enabled()`, `link_mtu_discovery()`, `remote_management_enabled()`, `required_discovery_value()`, `publish_blackhole_enabled()`, `blackhole_sources()`, `discovered_interfaces()`, `interface_discovery_sources()` | `crates/apps/reticulumd`, `crates/libs/rns-transport/src/transport`, `crates/libs/rns-rpc` | partial | Daemon startup, config loading, and transport operation exist. | Define and prove which `RNS.Reticulum` runtime/policy features are equivalent, then implement or explicitly defer the missing shared-instance, discovery, and management surface. |
| Identity and hashing | `RNS.Identity`, `recall()`, `recall_app_data()`, `full_hash()`, `truncated_hash()`, `get_random_hash()`, `current_ratchet_id()`, `from_bytes()`, `from_file()`, `to_file()`, key load/export, `encrypt()`, `decrypt()`, `sign()`, `validate()` | `crates/libs/rns-core/src/identity.rs`, `crates/libs/rns-core/src/hash.rs`, `crates/libs/rns-core/src/ratchets.rs` | partial | Core crypto primitives and hashing helpers are implemented. | Add public compatibility helpers for file/bytes I/O, identity recall/app-data recall, ratchet id lookup, and verify them against Python fixtures. |
| Destination behavior | `RNS.Destination`, naming/hash helpers, `announce()`, link acceptance, callbacks, proof strategy, request handlers, ratchet controls, app-data defaults | `crates/libs/rns-core/src/destination.rs`, `crates/libs/rns-transport/src/destination.rs` | partial | Announce encoding/validation and ratchet-aware destination behavior exist. | Add or map the callback, request-handler, and proof-strategy surface and prove behavior against the reference runtime. |
| Packet behavior | `RNS.Packet`, `send()`, `resend()`, `get_rssi()`, `get_snr()`, `get_q()` | `crates/libs/rns-core/src/packet.rs`, `crates/libs/rns-transport/src/packet.rs`, `crates/libs/rns-transport/src/transport` | partial | Packet framing and transport dispatch exist. | Expose a public compatibility facade for packet send/resend/stat semantics and add interoperability tests that exercise real packet lifecycles. |
| Packet receipt lifecycle | `RNS.PacketReceipt`, `get_status()`, `get_rtt()`, `set_timeout()`, delivery/timeout callbacks | `crates/libs/rns-transport/src/receipt.rs`, `crates/libs/rns-transport/src/transport/mod.rs` | missing | Only internal delivery receipt mapping helpers exist. | Introduce a public packet receipt object and prove timeout, RTT, delivery, and callback semantics. |
| Link lifecycle and stats | `RNS.Link`, `identify()`, `request()`, `track_phy_stats()`, signal stats, age/inactivity helpers, `get_remote_identity()`, `teardown()`, `get_channel()`, link/resource callbacks, resource strategy | `crates/libs/rns-transport/src/destination/link.rs`, `crates/libs/rns-transport/src/transport/links.rs` | partial | Link handshake, proof, keepalive, encrypted payloads, and event emission exist. | Promote the internal link model into a reference-shaped public facade, add missing telemetry and callback semantics, and prove them with live link tests. |
| Request receipt lifecycle | `RNS.RequestReceipt`, request id, status, progress, response, response time, completion | `crates/libs/rns-transport`, `crates/libs/lxmf-sdk` | missing | No reference-shaped request receipt surface is documented or tested. | Design a public request-receipt compatibility object and validate request/response progression against the reference behavior. |
| Resource transfer behavior | `RNS.Resource`, `advertise()`, `cancel()`, progress, transfer size, data size, parts, segments, hash, compression state | `crates/libs/rns-transport/src/resource.rs`, `crates/libs/rns-transport/src/resource` | partial | Advertisement packing, request decoding, proof handling, and event types exist. | Add a public resource lifecycle facade and end-to-end resource transfer parity tests on real links. |
| Channel messaging | `RNS.Channel.Channel`, registration/handler API, `send()`, `is_ready_to_send()`, `mdu`; `RNS.MessageBase.pack()/unpack()` | `crates/libs/rns-transport/src/channel.rs` | partial | Channel envelopes and handler registration exist internally. | Align public API names and message-pack/unpack expectations with the reference `Channel` and `MessageBase` surface. |
| Buffer and raw channel I/O | `RNS.Buffer`, `create_reader()`, `create_writer()`, `create_bidirectional_buffer()`, `RNS.RawChannelReader`, `RNS.RawChannelWriter` | `crates/libs/rns-core/src/buffer.rs`, `crates/libs/rns-transport/src/buffer.rs` | partial | Low-level input/output/static buffer primitives exist. | Add compatibility wrappers for reader/writer/raw-channel use cases or explicitly mark them unsupported for a non-Reticulum claim. |
| Transport path discovery | `RNS.Transport`, announce handler registration, `has_path()`, `hops_to()`, `next_hop()`, `next_hop_interface()`, `await_path()`, `request_path()` | `crates/libs/rns-transport/src/transport`, `crates/libs/rns-transport/src/transport/path_table.rs`, `crates/apps/reticulumd` | partial | Path requests, path table updates, and next-hop bookkeeping exist internally. | Expose the public query/callback API and verify the path lifecycle against the Python implementation. |
| Real interface behavior | required runtime behavior for external interoperability | `crates/libs/rns-transport/src/iface.rs`, `crates/apps/reticulumd/src/bin/reticulumd/interfaces` | partial | TCP, UDP, and serial core interfaces exist; daemon-specific BLE, LoRa, and serial integrations also exist. | Prove reference-compatible behavior on live interfaces and document which interface modes are part of the Reticulum claim. |
| User-facing Reticulum tools | `rnsd`, `rnid`, `rnpath`, `rnprobe`, related CRNS utilities | `crates/apps/rns-tools/src/bin`, `crates/apps/reticulumd` | partial | `rnsd` launches `reticulumd`; `rnx` provides harness utilities. | Replace current stubs with real compatibility tooling or clearly scope them out of the Reticulum claim surface. |
| Live external interoperability | bidirectional exchange with reference/external Reticulum clients | `docs/runbooks/*reticulumd-interop.md`, `docs/contracts/external-client-interop-acceptance-v1.md` | partial | Acceptance criteria and runbooks are documented. | Produce repeatable passing reports and turn the interop run into a release-gated proof artifact. |

## Priority Order

1. Public receipt and request lifecycle parity
   - `PacketReceipt` and `RequestReceipt` are fully missing today and are part of
     the published API surface.
2. Transport/discovery public API parity
   - Internal path functionality exists, but the reference-facing query and
     handler surface is still missing.
3. Link/resource public facade parity
   - The internal engine is ahead of the compatibility-facing API and proof.
4. Runtime policy/shared-instance/discovery parity
   - Required before the repo can credibly claim equivalence to
     `RNS.Reticulum`.
5. Live interoperability proof
   - Required for the final compatibility claim even if the code surface looks
     complete.
6. CLI/tool parity cleanup
   - Necessary to stop overstating parity in user-facing tooling.

## Exit Rule

Do not describe this repository as `Reticulum` until every required row above is
`verified` and the live interoperability gate passes reproducibly.
