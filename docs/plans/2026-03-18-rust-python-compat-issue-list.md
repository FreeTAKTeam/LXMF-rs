# Rust/Python Compatibility Issue List

Date: 2026-03-18

This document consolidates the current Rust-vs-Python incompatibilities found by
direct code inspection and parallel agent review against the reference Python
implementations in `Reticulum` and
`LXMF`.

Scope:

- `crates/libs/rns-transport`
- `crates/libs/rns-rpc`
- `crates/apps/reticulumd`
- Python references in `Reticulum`, `LXMF`, `Sideband`, and `Columba`-facing
  semantics where applicable

Goal:

- identify logic and state-machine issues that make the Rust daemon and
  transport incompatible with Python Reticulum/LXMF behavior
- prioritize the issues that block a credible "Python replacement" claim

Status snapshot as of 2026-03-19:

- merged and substantially addressed: `15` issues
  - `1`, `2`, `5`, `6`, `7`, `8`, `9`, `11`, `12`, `13`, `14`, `15`, `16`, `17`, `19`
- open draft PRs in progress: `1` issue
  - [#113](https://github.com/FreeTAKTeam/LXMF-rs/pull/113): `10`
- open follow-up on merged `15`:
  - [#111](https://github.com/FreeTAKTeam/LXMF-rs/pull/111): buffer callback parity on top of the merged channel buffer baseline
- remaining numbered issues not yet under active PR: `25`
## Priority 1

### 1. Announce validation accepts destination-hash mismatch

Status: merged in [#106](https://github.com/FreeTAKTeam/LXMF-rs/pull/106)

Area: transport, routing, ratchets

Rust behavior:

- [`crates/libs/rns-transport/src/destination.rs`](crates/libs/rns-transport/src/destination.rs:129) recomputes the expected destination hash
- [`crates/libs/rns-transport/src/destination.rs`](crates/libs/rns-transport/src/destination.rs:131) only logs mismatch and continues
- [`crates/libs/rns-transport/src/transport/announce.rs`](crates/libs/rns-transport/src/transport/announce.rs:31) remembers ratchet state
- [`crates/libs/rns-transport/src/transport/announce.rs`](crates/libs/rns-transport/src/transport/announce.rs:53) stores route state

Python reference:

- [`Reticulum/RNS/Identity.py`](Reticulum/RNS/Identity.py:443) rejects the announce
- [`Reticulum/RNS/Identity.py`](Reticulum/RNS/Identity.py:482) returns failure on mismatch

Impact:

- forged announces can poison routes and ratchet state for arbitrary destinations
- higher-level behavior becomes nondeterministic because the trust root is wrong

### 2. Packet receipts can be satisfied by forged proofs

Status: merged in [#106](https://github.com/FreeTAKTeam/LXMF-rs/pull/106)

Area: packet proofs, delivery receipts

Rust behavior:

- [`crates/libs/rns-transport/src/transport/wire.rs`](crates/libs/rns-transport/src/transport/wire.rs:39) treats non-link-request proofs as receipts based on payload shape
- [`crates/libs/rns-transport/src/transport/wire.rs`](crates/libs/rns-transport/src/transport/wire.rs:55) calls the receipt handler without signature verification

Python reference:

- [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:2102) validates proof before delivery handling
- [`Reticulum/RNS/Packet.py`](Reticulum/RNS/Packet.py:442) and [`Reticulum/RNS/Packet.py`](Reticulum/RNS/Packet.py:497) validate proof signatures

Impact:

- Rust can mark packets or messages delivered when Python would reject the proof
- this breaks receipt semantics and any success/failure logic built on them

### 3. Requested LXMF delivery method is ignored

Area: daemon send path, LXMF compatibility

Rust behavior:

- [`crates/apps/reticulumd/src/bin/reticulumd/bridge.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge.rs:289) receives `OutboundDeliveryOptions`
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge.rs:292) discards them
- the bridge then always tries direct-link delivery first and opportunistic fallback later

Python reference:

- [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:29) defines `OPPORTUNISTIC`, `DIRECT`, `PROPAGATED`, and `PAPER`
- [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2564), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2594), and [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2675) route methods through distinct logic

Impact:

- Rust does not honor client intent
- propagated delivery is not a real mode
- parity claims fail at the daemon API boundary

### 4. Propagated delivery is not implemented as Python propagation-node delivery

Area: daemon routing, propagation

Rust behavior:

- propagation node selection exists in [`crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_propagation.rs`](crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_propagation.rs:191)
- outbound send path in [`crates/apps/reticulumd/src/bin/reticulumd/bridge.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge.rs:289) never consults that selected node and never opens a propagation link

Python reference:

- [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2678) requires an outbound propagation node
- [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2718) sends via the outbound propagation link

Impact:

- Rust cannot truthfully claim propagated LXMF delivery support

### 5. Link activation has a proof race

Status: merged in [#107](https://github.com/FreeTAKTeam/LXMF-rs/pull/107)

Area: link establishment

Rust behavior:

- [`crates/libs/rns-transport/src/transport/links.rs`](crates/libs/rns-transport/src/transport/links.rs:190) sends the link request
- [`crates/libs/rns-transport/src/transport/links.rs`](crates/libs/rns-transport/src/transport/links.rs:192) only then registers the pending out-link

Python reference:

- [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:317) and [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:321) register before send

Impact:

- a fast proof can arrive before Rust has state to match it
- valid links can spuriously fail

### 6. Resource startup reports success before advertisement send is proven

Status: in progress in [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: resources, daemon send path

Rust behavior:

- [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:25) and [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:41) insert sender state before dispatch outcome is known
- [`crates/libs/rns-transport/src/transport/links.rs`](crates/libs/rns-transport/src/transport/links.rs:121) and [`crates/libs/rns-transport/src/transport/links.rs`](crates/libs/rns-transport/src/transport/links.rs:144) ignore advertisement dispatch failure and still return success

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:523) only registers after advertisement send succeeds
- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:536) cancels on send failure

Impact:

- Rust can report transfer start when nothing was actually sent

### 7. Outbound resources lack Python-style retry, timeout, and cleanup

Status: in progress in [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: resources

Rust behavior:

- [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:49) only retries inbound receivers
- outgoing senders are removed only on proof or cancel in [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:241) and [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:257)

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:561) through [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:666) implement advertisement retry, part-request timeout, proof timeout, and cancellation

Impact:

- stalled outbound resources can live forever in Rust

### 8. Failed inbound resources can get stuck forever

Status: in progress in [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: resources

Rust behavior:

- [`crates/libs/rns-transport/src/resource/receiver.rs`](crates/libs/rns-transport/src/resource/receiver.rs:108) marks failures but still returns incomplete
- [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:182) keeps failed receivers
- [`crates/libs/rns-transport/src/resource/receiver.rs`](crates/libs/rns-transport/src/resource/receiver.rs:240) stops them from retrying

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:608) through [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:645) time out and cancel failed transfers

Impact:

- dead receivers leak state and block clean transfer semantics

### 9. Duplicate resource advertisements reset receive progress

Status: in progress in [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: resources

Rust behavior:

- [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:114) always replaces the receiver for the same resource hash

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:221) through [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:237) ignore duplicate advertisements while transfer is active

Impact:

- Rust can discard already-received parts and retry state

### 10. Resource proof is treated as final LXMF delivery

Status: in progress in [#113](https://github.com/FreeTAKTeam/LXMF-rs/pull/113), stacked on [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: daemon status model

Rust behavior:

- [`crates/apps/reticulumd/src/bin/reticulumd/bridge.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge.rs:198) biases peer activity as successful too early
- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs:59) upgrades `OutboundComplete` to `"delivered"`

Python reference:

- [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:16) keeps `SENDING`, `SENT`, and `DELIVERED` distinct

Impact:

- daemon observability lies about success
- retries, UX, and peer scoring are built on the wrong state

## Priority 2

### 11. Known-destination public-key stability check is missing

Status: merged in [#106](https://github.com/FreeTAKTeam/LXMF-rs/pull/106)

Area: announce trust model

Rust behavior:

- no equivalent check exists in [`crates/libs/rns-transport/src/destination.rs`](crates/libs/rns-transport/src/destination.rs:135) through [`crates/libs/rns-transport/src/destination.rs`](crates/libs/rns-transport/src/destination.rs:219)

Python reference:

- [`Reticulum/RNS/Identity.py`](Reticulum/RNS/Identity.py:449) rejects announces that change the known key for an already known destination hash

Impact:

- Rust is weaker than Python against key-substitution style announce drift

### 12. Ratchet-bearing announce parsing is more permissive than Python

Status: merged in [#106](https://github.com/FreeTAKTeam/LXMF-rs/pull/106)

Area: announce parsing, ratchets

Rust behavior:

- [`crates/libs/rns-transport/src/destination.rs`](crates/libs/rns-transport/src/destination.rs:207) falls back to ratchet-aware parsing even when the ratchet flag is unset

Python reference:

- [`Reticulum/RNS/Identity.py`](Reticulum/RNS/Identity.py:403) through [`Reticulum/RNS/Identity.py`](Reticulum/RNS/Identity.py:423) branch strictly on the announce flag

Impact:

- Rust is more tolerant than the reference parser
- this may mask malformed peers instead of surfacing protocol drift

### 13. Transported link-request proofs skip Python validation gates

Status: merged in [#106](https://github.com/FreeTAKTeam/LXMF-rs/pull/106)

Area: routed proofs

Rust behavior:

- [`crates/libs/rns-transport/src/transport/wire.rs`](crates/libs/rns-transport/src/transport/wire.rs:73) forwards matching proofs into link-table handling
- [`crates/libs/rns-transport/src/transport/link_table.rs`](crates/libs/rns-transport/src/transport/link_table.rs:97) validates and retransmits immediately

Python reference:

- [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:2013) only transports `LRPROOF` after hop, ingress, and signature checks

Impact:

- Rust can relay proofs that Python would drop

### 14. Link interface binding is recorded but not enforced

Status: merged in [#107](https://github.com/FreeTAKTeam/LXMF-rs/pull/107)

Area: link security, multi-interface behavior

Rust behavior:

- ingress interface is stored in [`crates/libs/rns-transport/src/transport/path.rs`](crates/libs/rns-transport/src/transport/path.rs:113)
- link state carries interface metadata in [`crates/libs/rns-transport/src/destination/link.rs`](crates/libs/rns-transport/src/destination/link.rs:71)
- [`crates/libs/rns-transport/src/destination/link.rs`](crates/libs/rns-transport/src/destination/link.rs:296) does not check interface on later packets

Python reference:

- [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:979) rejects link packets arriving on the wrong interface

Impact:

- link attachment semantics differ from Python on multi-interface nodes

### 15. Channel packet semantics are not implemented

Status: merged baseline in [#109](https://github.com/FreeTAKTeam/LXMF-rs/pull/109); deeper buffer-layer parity is still in progress in [#110](https://github.com/FreeTAKTeam/LXMF-rs/pull/110) and [#111](https://github.com/FreeTAKTeam/LXMF-rs/pull/111)

Area: link data plane

Rust behavior:

- `PacketContext::Channel` exists in [`crates/libs/rns-transport/src/packet.rs`](crates/libs/rns-transport/src/packet.rs:138)
- [`crates/libs/rns-transport/src/destination/link.rs`](crates/libs/rns-transport/src/destination/link.rs:243) does not handle `Channel` packets

Python reference:

- [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:1169) and [`Reticulum/RNS/Channel.py`](Reticulum/RNS/Channel.py:581) implement reliable channel traffic

Impact:

- a Python peer using channels will not get equivalent behavior from Rust

### 16. Link proof behavior for request/response/identify differs from Python

Status: merged in [#107](https://github.com/FreeTAKTeam/LXMF-rs/pull/107)

Area: link receipts

Rust behavior:

- [`crates/libs/rns-transport/src/destination/link.rs`](crates/libs/rns-transport/src/destination/link.rs:243) through [`crates/libs/rns-transport/src/destination/link.rs`](crates/libs/rns-transport/src/destination/link.rs:273) auto-prove request, response, and identify contexts

Python reference:

- [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:992), [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:1014), and [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:1034) do not mirror that behavior

Impact:

- receipt behavior diverges even when wire bytes otherwise match

### 17. Link watchdog timing is fixed-interval instead of RTT-driven

Status: merged in [#107](https://github.com/FreeTAKTeam/LXMF-rs/pull/107)

Area: liveness

Rust behavior:

- [`crates/libs/rns-transport/src/transport/jobs.rs`](crates/libs/rns-transport/src/transport/jobs.rs:13) and [`crates/libs/rns-transport/src/transport/jobs.rs`](crates/libs/rns-transport/src/transport/jobs.rs:40) use elapsed-time thresholds

Python reference:

- [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:780) and [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:848) derive watchdog timing from RTT and per-link state

Impact:

- Rust will diverge from Python on long-latency or bursty links

### 18. Inbound resource allocation is unbounded by advertised parts

Area: resources, daemon resilience

Rust behavior:

- [`crates/libs/rns-transport/src/resource/receiver.rs`](crates/libs/rns-transport/src/resource/receiver.rs:35) allocates receive state directly from `adv.parts`

Python reference:

- Python also allocates from advertisement-derived counts, but its transfer model includes stronger watchdog and cancellation behavior in [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:608)

Impact:

- Rust daemon can be forced into excessive allocation with fewer recovery paths

### 19. Inbound resource worker assumes every completed resource is LXMF

Status: in progress in [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: daemon inbound pipeline

Rust behavior:

- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs:46) always decodes completed resource payloads as LXMF full-wire messages

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:165) treats `Resource` as generic link transport

Impact:

- Rust daemon will mishandle non-LXMF resource traffic on a shared link

### 20. Path responses drop the original request tag

Area: path discovery

Rust behavior:

- [`crates/libs/rns-transport/src/transport/path.rs`](crates/libs/rns-transport/src/transport/path.rs:24) answers a path request with `path_response(OsRng, None)`
- the original request tag is not preserved into the response path

Python reference:

- Python destinations cache and reuse path-response announce payloads keyed by the request tag in [`Reticulum/RNS/Destination.py`](Reticulum/RNS/Destination.py:277) and [`Reticulum/RNS/Destination.py`](Reticulum/RNS/Destination.py:307)

Impact:

- request/response correlation can diverge from Python behavior
- pathfinding clients that rely on tag continuity can behave differently

### 21. Recursive path forwarding regenerates tags instead of preserving them

Area: path discovery

Rust behavior:

- [`crates/libs/rns-transport/src/transport/path.rs`](crates/libs/rns-transport/src/transport/path.rs:62) calls `generate_recursive(..., None)`
- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs:196) generates a fresh random tag when none is passed

Python reference:

- Python tracks discovery path-request tags and preserves them through the discovery lifecycle in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:595) and related path-request state

Impact:

- recursive discovery initiated by Rust can diverge from Python path-request identity and duplicate suppression behavior

### 22. Path-request duplicate suppression has no bounded lifetime

Area: path discovery

Rust behavior:

- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs:72) caches seen `(destination, tag)` pairs
- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs:102) checks duplicates, but the cache is never expired or bounded

Python reference:

- Python cleans up discovery-path state over time and ties it to explicit timeout paths in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:723)

Impact:

- Rust can suppress legitimate later path requests that Python would allow again

### 23. Recursive path throttling is global instead of interface-aware

Area: path discovery

Rust behavior:

- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs:149) uses one global pending-destination map plus queue caps

Python reference:

- Python keeps richer request state and interface-sensitive path discovery behavior in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:118) and [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:595)

Impact:

- Rust recursion suppression and retry behavior differ on multi-interface nodes

### 24. Interface-side announce queueing and pacing are missing

Area: announce propagation

Rust behavior:

- Rust has only transport-global caps and no per-interface announce queue or bitrate pacing loop
- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs:77) contains queue caps for recursive path requests, not interface announce shaping

Python reference:

- Python queues announces per interface and releases them according to interface bitrate and announce caps in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:1030) and [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:246)

Impact:

- Rust will rebroadcast announces in cases where Python would defer or suppress them

### 25. Ingress-limited held-announce release behavior is missing

Area: announce propagation

Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_limits.rs`](crates/libs/rns-transport/src/transport/announce_limits.rs:67) tracks simple destination-keyed limits
- there is no held-announce buffer or deferred release path

Python reference:

- Python can hold announces during ingress pressure and later release the best candidate in [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:170) and [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:176)

Impact:

- Rust drops announces that Python would often preserve and process later

### 26. Announce forwarding rules are not interface-mode aware

Area: multi-interface routing

Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_table.rs`](crates/libs/rns-transport/src/transport/announce_table.rs:62) chooses simple rebroadcast targets without Python-style interface-mode checks

Python reference:

- Python blocks announce forwarding based on interface mode, next-hop interface mode, and attached-interface rules in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:1030)

Impact:

- announce spread and route learning can diverge on real mixed-interface networks

### 27. Announce retransmit timing and completion policy do not match Python

Area: announce/pathfinder behavior

Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_table.rs`](crates/libs/rns-transport/src/transport/announce_table.rs:164) and [`crates/libs/rns-transport/src/transport/announce_table.rs`](crates/libs/rns-transport/src/transport/announce_table.rs:180) use a fixed timeout model
- [`crates/libs/rns-transport/src/transport/jobs.rs`](crates/libs/rns-transport/src/transport/jobs.rs:225) drives retransmit from a polling loop

Python reference:

- Python pathfinder announce service uses `PATHFINDER_G`, `PATHFINDER_RW`, separate retry ceilings, and held-announce reinsertion in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:518)

Impact:

- remote nodes can learn paths at different times or not at all relative to Python behavior

### 28. Announce rate limiting is destination-keyed instead of interface-centric

Area: announce control

Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_limits.rs`](crates/libs/rns-transport/src/transport/announce_limits.rs:24) tracks one `last_announce` per destination

Python reference:

- Python tracks incoming announce frequency and ingress state on interfaces in [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:202)

Impact:

- Rust will suppress or admit announce traffic under a different policy than Python

### 29. Route restoration from cached announces is weaker than Python

Area: startup recovery

Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce.rs`](crates/libs/rns-transport/src/transport/announce.rs:53) updates the path table on live announce handling
- the announce cache in [`crates/libs/rns-transport/src/transport/announce_table.rs`](crates/libs/rns-transport/src/transport/announce_table.rs:69) is used for retransmit and path-response replay only

Python reference:

- Python restores cached announce packets into the path table on load in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:276) and [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:291)

Impact:

- Rust startup/restart routing state will not converge the same way as Python

### 30. Stamp and ticket options are accepted by the API but do not drive wire behavior

Area: LXMF primitives

Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/send_request.rs`](crates/libs/rns-rpc/src/rpc/send_request.rs:85) parses `stamp_cost` and `include_ticket`
- [`crates/libs/rns-rpc/src/rpc/helpers.rs`](crates/libs/rns-rpc/src/rpc/helpers.rs:26) stores them under `_lxmf`
- [`crates/apps/reticulumd/src/lxmf_bridge.rs`](crates/apps/reticulumd/src/lxmf_bridge.rs:9) and the LXMF core wire path do not generate stamps or consume tickets

Python reference:

- Python wires these into real send behavior in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1654), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1663), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:299)

Impact:

- Python clients relying on stamp or ticket semantics will not get equivalent behavior from Rust

### 31. Inbound stamp enforcement is missing

Area: LXMF primitives

Rust behavior:

- stamp policy can be stored and reported through [`crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_misc.rs`](crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_misc.rs:49) and [`crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs:401)
- no audited inbound path validates stamps or rejects invalid stamped messages

Python reference:

- Python validates and can reject inbound stamps in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1749), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1761), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:278)

Impact:

- Rust advertises stamp policy without enforcing it

### 32. `ticket_generate` does not implement Python ticket semantics

Area: LXMF primitives

Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_misc.rs`](crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_misc.rs:84) hashes destination plus current time and returns hex
- the result is cached in memory and not wired into actual send/receive message logic

Python reference:

- Python tickets are binary material persisted and reused through the router lifecycle in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1023), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1052), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:282)

Impact:

- Rust exposes the name of the feature, but not the protocol meaning

### 33. Propagation stamp validation is missing

Area: propagated LXMF

Rust behavior:

- propagation status surfaces stamp-related values in [`crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs:374)
- no audited propagation ingest path validates propagation stamps before accepting traffic

Python reference:

- Python validates propagation-node stamps in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2115), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2245), and [`LXMF/LXMF/LXStamper.py`](LXMF/LXMF/LXStamper.py:87)

Impact:

- propagated anti-abuse semantics are missing in Rust

### 34. Announced inbound stamp cost is discarded

Area: peer capability learning

Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/daemon/init.rs`](crates/libs/rns-rpc/src/rpc/daemon/init.rs:371) explicitly ignores `stamp_cost`

Python reference:

- Python updates outbound stamp-cost memory from announce data in [`LXMF/LXMF/Handlers.py`](LXMF/LXMF/Handlers.py:17) and [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1648)

Impact:

- Rust does not learn recipient stamp requirements the way Python nodes do

### 35. Deferred stamp generation is not implemented

Area: LXMF sender lifecycle

Rust behavior:

- no audited daemon path includes a deferred-stamp work queue or LXMF stamper integration

Python reference:

- Python queues deferred normal and propagation stamp work in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2404), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2440), and [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2463)

Impact:

- behavior and throughput diverge once stamp costs matter

### 36. Propagation transient-id lifecycle is incomplete

Area: propagated LXMF

Rust behavior:

- [`crates/libs/lxmf-core/src/message/wire.rs`](crates/libs/lxmf-core/src/message/wire.rs:190) has helpers for propagation packing
- the daemon path does not implement the Python-style `propagation_packed` and `transient_id` lifecycle end to end

Python reference:

- Python derives `transient_id` from destination hash plus encrypted payload and optionally appends a propagation stamp in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:438) through [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:441)

Impact:

- propagated LXMF is incomplete even below the router layer

### 37. Inbound daemon decoding drops stamp validity state

Area: daemon message model

Rust behavior:

- [`crates/libs/lxmf-core/src/message/payload.rs`](crates/libs/lxmf-core/src/message/payload.rs:74) parses an optional raw stamp
- [`crates/apps/reticulumd/src/inbound_delivery.rs`](crates/apps/reticulumd/src/inbound_delivery.rs:58) stores only title, content, timestamp, and fields

Python reference:

- Python tracks `stamp_valid`, `stamp_checked`, and propagation stamp validity on the message object in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:160) and validation logic in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:275)

Impact:

- the Rust daemon message model cannot represent Python stamp-validation outcomes

### 38. Inbound timestamp precision is truncated

Area: daemon API compatibility

Rust behavior:

- [`crates/libs/lxmf-core/src/inbound_decode.rs`](crates/libs/lxmf-core/src/inbound_decode.rs:48) truncates payload timestamps from `f64` to `i64`
- [`crates/apps/reticulumd/src/inbound_delivery.rs`](crates/apps/reticulumd/src/inbound_delivery.rs:68) persists the integer form

Python reference:

- Python preserves floating timestamps in payloads and unpacked messages in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:367) and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:752)

Impact:

- ordering and client-visible metadata can drift from Python

### 39. Inbound title/content decoding loses binary fidelity

Area: daemon API compatibility

Rust behavior:

- [`crates/libs/lxmf-core/src/inbound_decode.rs`](crates/libs/lxmf-core/src/inbound_decode.rs:46) converts bytes with `String::from_utf8(...).unwrap_or_default()`

Python reference:

- Python stores raw bytes and only decodes on explicit request in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:204), [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:213), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:792)

Impact:

- non-UTF8 title/content conventions suffer data loss via the Rust daemon

### 40. Outbound field-shape handling is stricter than Python

Area: wrapper and client compatibility

Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/send_request.rs`](crates/libs/rns-rpc/src/rpc/send_request.rs:112) rejects legacy `files` and raw wire key `5`
- [`crates/libs/lxmf-core/src/wire_fields.rs`](crates/libs/lxmf-core/src/wire_fields.rs:32) normalizes to a stricter `attachments` shape

Python reference:

- Python accepts arbitrary field dicts in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:220)

Impact:

- wrappers or tools that already speak Python-style raw field maps can break against the Rust daemon API

### 41. Custom storage encoding can break Python `.lxm` interchange

Area: storage interoperability

Rust behavior:

- [`crates/libs/lxmf-core/src/message/container.rs`](crates/libs/lxmf-core/src/message/container.rs:20) can emit a Python-like msgpack map container
- [`crates/libs/lxmf-core/src/message/wire.rs`](crates/libs/lxmf-core/src/message/wire.rs:78) also defines a custom `LXMFSTR0` binary storage format

Python reference:

- Python persists a msgpack map with LXMF metadata in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:655)

Impact:

- if Rust ever uses the custom storage encoding for external `.lxm` interchange, it will not be Python-compatible

## Surface Summary

Recently addressed in merged PRs:

- announce trust baseline: issues `1`, `11`, `12`
- proof and routed-proof validation baseline: issues `2`, `13`
- link establishment, interface enforcement, proof-policy, and watchdog baseline: issues `5`, `14`, `16`, `17`
- live channel and buffer-writer baseline: issue `15`
- resource lifecycle and generic-resource handling baseline: issues `6`, `7`, `8`, `9`, `19`

Still actively being refined on open stacked PRs:

- callback dispatch parity on top of the merged channel buffer baseline: [#111](https://github.com/FreeTAKTeam/LXMF-rs/pull/111)
- daemon receipt semantics for resource-backed sends: [#113](https://github.com/FreeTAKTeam/LXMF-rs/pull/113)
Confirmed relatively aligned primitives:

- identity hashing and announce random-blob layout look intentionally Python-aware
- basic LXMF wire payload layout `[timestamp, title, content, fields, optional stamp]` is aligned
- LXMF message-id derivation from destination, source, and payload-without-stamp is aligned

Confirmed major compatibility gaps:

- announce trust and announce metadata handling
- proof validation and receipt semantics
- link establishment, interface binding, and channel semantics
- resource sender/receiver lifecycle
- path discovery and interface-aware announce control
- delivery-mode semantics and propagated delivery
- propagation-node and peer-sync router behavior
- stamps, tickets, and propagation stamps
- daemon-side inbound decode and storage fidelity

Still worth auditing later, but no additional blocker was confirmed in this pass:

- deeper destination proof-strategy configuration parity beyond the specific proof-policy drifts already listed

## Additional High-Risk Surfaces Still Under Audit

The following areas look incomplete or likely divergent, but they need a final
evidence pass before being promoted to the confirmed issue list above.

### A. Destination proof-strategy semantics

- Python destinations expose explicit proof strategies in [`Reticulum/RNS/Destination.py`](Reticulum/RNS/Destination.py:160) and [`Reticulum/RNS/Destination.py`](Reticulum/RNS/Destination.py:369)
- a repo-wide Rust search did not find an equivalent proof-strategy surface

Risk:

- proof emission policy may still differ in more places than the currently confirmed receipt issues

### B. Stamp and ticket semantics

- Rust has daemon-side stamp policy and ticket generation hooks in [`crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_misc.rs`](crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_misc.rs:84)
- Python `LXMRouter` maintains richer outbound stamp costs, available tickets, renewal windows, and deferred stamp generation in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:140) through [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:283)

Risk:

- Rust may expose stamp/ticket APIs without implementing the actual Python semantics clients expect

### C. LXMF wire and persistence semantics

- Rust `lxmf-core` wire packing is close in shape, but full parity still needs confirmation for propagation, paper, stamps, and stored-message containers in [`crates/libs/lxmf-core/src/message/wire.rs`](crates/libs/lxmf-core/src/message/wire.rs:35)
- Python message semantics live in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:360)

Risk:

- client compatibility may still break at the message container or propagation payload layer even if transport primitives are fixed

## Recommended Fix Order

1. Fail closed on invalid announces and add Python-parity tests.
2. Validate packet proofs before satisfying receipts.
3. Make `OutboundDeliveryOptions` authoritative, including real propagated delivery.
4. Register out-links before sending link requests.
5. Rebuild resource sender and receiver lifecycle to match Python watchdog, retry, duplicate, and completion semantics.
6. Fix link interface enforcement, channel handling, and proof-policy parity.
7. Upgrade propagation-node and peer-sync behavior from bookkeeping to router semantics.

## Validation Notes

- One narrow transport test run passed:
  - `cargo test -p rns-transport resource --lib`
- That currently covers only a small subset of resource behavior and does not
  exercise the incompatibilities listed here.
