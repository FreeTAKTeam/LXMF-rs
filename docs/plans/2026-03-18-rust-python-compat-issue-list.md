# Rust/Python Compatibility Issue List

Date: 2026-03-18

This document consolidates the current Rust-vs-Python incompatibilities found by
direct code inspection and parallel agent review against the reference Python
implementations in `Reticulum` and
`LXMF`.

Historical naming note: this issue list keeps workspace paths like
`crates/libs/rns-transport`, `crates/libs/rns-rpc`, and `crates/libs/lxmf-core`
for code-navigation clarity. The corresponding published crate names are
`reticulum-rs-transport`, `reticulum-rs-rpc`, and `lxmf-wire`.

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
- remaining numbered issues not yet under active PR: see
  `docs/status/current-roadmap.md`; issue `25` is implemented in active
  workspace and its body below is newer than this historical snapshot.

Reassessment note as of 2026-05-07:

- Issue `3` is closed by active delivery-mode handling in `reticulumd`; deeper
  propagation-router behavior remains issue `4`.
- Issue `34` is closed by active announce stamp-cost parsing, persistence, and
  outbound send lookup.
- Treat `docs/status/current-roadmap.md` as the current execution order when
  this historical issue list disagrees with active evidence.
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

Status: merged baseline in [#114](https://github.com/FreeTAKTeam/LXMF-rs/pull/114);
deeper propagation-router parity remains tracked under issue `4`.

Area: daemon send path, LXMF compatibility

Rust behavior:

- Active `reticulumd` bridge tests now cover requested delivery-mode handling.
- Historical code pointers below are stale for the active workspace and are
  retained only to explain the original finding.

Python reference:

- [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:29) defines `OPPORTUNISTIC`, `DIRECT`, `PROPAGATED`, and `PAPER`
- [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2564), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2594), and [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2675) route methods through distinct logic

Impact:

- The baseline "delivery method ignored" gap is closed.
- Propagated delivery is still not full Python propagation-node behavior; keep
  parity claims constrained by issue `4`.

### 4. Propagated delivery is not implemented as Python propagation-node delivery

Status: partial. Active propagated sends require a selected outbound
propagation node, use that selected node to resolve/open a propagation link,
and now expose the selected node through `propagation_status` and
`daemon_status_ex`. Remote propagation sync calls also update started,
completed, and failed sync lifecycle fields in propagation status. This is still
not full Python propagation-node parity: peer sync/fetch/offer lifecycle and
router side effects remain narrower than `LXMRouter`.

Area: daemon routing, propagation

Rust behavior:

- propagation node selection exists in [`crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_propagation.rs`](crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_propagation.rs) and updates public propagation status snapshots
- remote propagation sync calls update `sync_state`, `state_name`,
  `sync_progress`, `last_sync_started`, `last_sync_completed`, and
  `last_sync_error` in the same public propagation status model
- outbound propagated sends in [`crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs) require the selected node and hand it to delivery tasks
- propagation delivery tasks in [`crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task.rs) resolve/open the selected propagation link and send the packed payload

Python reference:

- [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2678) requires an outbound propagation node
- [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2718) sends via the outbound propagation link

Impact:

- Rust can no longer be described as ignoring the selected propagation node, but
  it still cannot truthfully claim full Python propagation-node behavior until
  peer sync/fetch/offer lifecycle and router side effects are equivalent.

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

Status: implemented in active workspace for transport resource send entry
points.

Area: resources, daemon send path

Active Rust behavior:

- transport resource send methods confirm the advertisement dispatch outcome
  before returning success
- failed advertisement dispatch removes pending sender state, emits
  `OutboundFailed`, publishes that event through `Transport::resource_events`,
  and returns `ConnectionError`
- tests cover manager-level dispatch failure and transport-level dropped
  advertisement dispatch

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:523) only registers after advertisement send succeeds
- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:536) cancels on send failure

Impact:

- the original false-success startup path is closed for active transport
  resource sends; broader resource lifecycle parity remains tracked by the
  retry, timeout, cancellation, and segmentation items below

### 7. Outbound resources lack Python-style retry, timeout, and cleanup

Status: in progress in [#112](https://github.com/FreeTAKTeam/LXMF-rs/pull/112)

Area: resources

Rust behavior:

- [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:49) only retries inbound receivers
- outgoing senders are removed only on proof or cancel in [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:241) and [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:257)
- active outbound resources can now be explicitly cancelled through
  `ResourceManager::cancel_outgoing` or `Transport::cancel_resource`, which
  removes sender state, emits `OutboundCancelled`, and dispatches a
  `ResourceInitiatorCancel` packet over the link's bound interface

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:561) through [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:666) implement advertisement retry, part-request timeout, proof timeout, and cancellation

Impact:

- explicit caller-driven cancellation no longer leaves active outbound sender
  state behind, but full Python-style segmented-transfer and watchdog lifecycle
  parity remains broader work

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

Status: implemented for active `reticulumd` receipt status and peer-activity
bookkeeping; broader resource retry/lifecycle parity remains tracked by the
resource issues.

Area: daemon status model

Active Rust behavior:

- `reticulumd` records transport send/resource completion as `sent:*` status,
  not `delivered`
- peer activity now separates sent-only transport bookkeeping from actual
  delivery receipts, so send completion updates tx bytes without marking a peer
  heard/alive or improving acceptance rate
- delivery receipts still mark the outbound peer delivered through the stored
  message destination

Python reference:

- [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:16) keeps `SENDING`, `SENT`, and `DELIVERED` distinct

Impact:

- the original conflation of resource proof/send completion with final LXMF
  delivery is closed for active daemon status and peer scoring paths
- lower-level resource retry and segmented transfer parity remains outside this
  issue

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

### 17. Link watchdog timing follow-through lagged behind the RTT-driven baseline

Status: baseline merged in [#107](https://github.com/FreeTAKTeam/LXMF-rs/pull/107); follow-through parity is now implemented in the active workspace

Area: liveness

Rust behavior:

- [`crates/libs/rns-transport/src/destination/link.rs`](crates/libs/rns-transport/src/destination/link.rs:1016) derives direct-link keepalive and stale deadlines from per-link RTT, exposes Python-style activity timers, and emits protocol `LinkClose` packets on manual or watchdog teardown
- [`crates/libs/rns-transport/src/transport/jobs.rs`](crates/libs/rns-transport/src/transport/jobs.rs:24) now schedules maintenance from the earliest watchdog or channel-retry deadline instead of a fixed-interval sweep alone
- [`crates/libs/rns-transport/src/resource/manager.rs`](crates/libs/rns-transport/src/resource/manager.rs:158) purges link-scoped resource retries when links close so teardown matches Python lifecycle expectations more closely

Python reference:

- [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:780) and [`Reticulum/RNS/Link.py`](Reticulum/RNS/Link.py:848) derive watchdog timing from RTT and per-link state

Impact:

- the RTT-driven watchdog gap is closed; remaining risk is limited to continued live interop verification rather than a known timing-model mismatch

### 18. Inbound resource allocation is unbounded by advertised parts

Status: implemented for active non-split Rust resource receiving. The receiver
now rejects zero-sized and oversized transfer advertisements, bounds advertised
part count by the Reticulum packet MDU rather than trusting `adv.parts`, and
caps compressed payload expansion by the advertised uncompressed size and
Python's 64 MiB auto-compress ceiling. Outbound resource retry exhaustion and
advertisement dispatch failure now emit failure events so daemon-level LXMF
sends can fail instead of leaving stale resource tracking behind.
Split/segmented resource support remains unsupported and is still rejected.

Area: resources, daemon resilience

Active Rust behavior:

- [`crates/libs/rns-transport/src/resource/receiver.rs`](crates/libs/rns-transport/src/resource/receiver.rs) validates transfer size, derives the maximum accepted part count from `transfer_size.div_ceil(PACKET_MDU)`, and rejects excessive `adv.parts` before allocating receive vectors
- compressed payload assembly uses bounded decompression instead of unbounded `read_to_end`
- tests cover unreasonable advertised parts, MDU-derived part-count bounds, retry cleanup, and bounded decompression
- outbound retry exhaustion and failed advertisement dispatch emit
  `OutboundFailed`; `reticulumd` maps a tracked LXMF resource timeout to a
  failed receipt plus failed peer activity

Python reference:

- Python also allocates from advertisement-derived counts, but its transfer model includes stronger watchdog and cancellation behavior in [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:608)

Remaining caveat:

- Python supports segmented resources; Rust still rejects split advertisements
  rather than implementing the full segmented transfer model
- Python has a richer resource cancellation API; Rust now exposes timeout
  failure to the daemon and explicit outbound resource cancellation through
  the transport API, but broader cancellation semantics remain partial

### 19. Inbound resource worker assumes every completed resource is LXMF

Status: implemented for active `reticulumd` resource ingestion. The inbound
worker resolves the resource's link destination first and only decodes completed
resources for `lxmf.delivery` or `lxmf.propagation`; resources on non-LXMF
destinations are ignored by the LXMF daemon worker instead of being forced
through LXMF full-wire decoding.

Area: daemon inbound pipeline

Active Rust behavior:

- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_worker.rs) calls `resolve_resource_destination()` before decoding resource payloads
- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_routing.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_routing.rs) accepts `lxmf.delivery`, treats `lxmf.propagation` separately, and rejects non-LXMF link destinations
- tests cover LXMF delivery acceptance, LXMF propagation detection, and non-delivery rejection

Python reference:

- [`Reticulum/RNS/Resource.py`](Reticulum/RNS/Resource.py:165) treats `Resource` as generic link transport

Impact:

- The original generic-resource decode bug is closed for the active daemon
  worker. Broader generic Reticulum resource APIs remain outside the LXMF daemon
  ingestion path.

### 20. Path responses drop the original request tag

Status: implemented in active workspace. Local path-request handling now passes
the decoded request tag into destination path-response generation, and
destinations cache path-response announce data by tag.

Area: path discovery

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/path.rs`](crates/libs/rns-transport/src/transport/path.rs) calls `path_response_with_tag(..., Some(request.tag_bytes.as_slice()))`
- [`crates/libs/rns-transport/src/destination.rs`](crates/libs/rns-transport/src/destination.rs) caches tagged path-response announce payloads
- tests cover cached path-response reuse for the same tag

Python reference:

- Python destinations cache and reuse path-response announce payloads keyed by the request tag in [`Reticulum/RNS/Destination.py`](Reticulum/RNS/Destination.py:277) and [`Reticulum/RNS/Destination.py`](Reticulum/RNS/Destination.py:307)

Impact:

- The original tag-drop gap is closed for active local path responses.

### 21. Recursive path forwarding regenerates tags instead of preserving them

Status: implemented in active workspace. Recursive path forwarding preserves
the decoded incoming request tag when generating the forwarded request.

Area: path discovery

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/path.rs`](crates/libs/rns-transport/src/transport/path.rs) calls `generate_recursive(..., Some(request.tag_bytes.clone()))`
- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs) keeps supplied recursive tags intact
- tests cover recursive path-request tag preservation

Python reference:

- Python tracks discovery path-request tags and preserves them through the discovery lifecycle in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:595) and related path-request state

Impact:

- The original recursive tag regeneration gap is closed for active forwarding.

### 22. Path-request duplicate suppression has no bounded lifetime

Status: implemented in active workspace. Duplicate `(destination, tag)` entries
are stored with expiries and pruned before new decode decisions.

Area: path discovery

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs) stores seen `(destination, tag)` pairs with request-timeout expiry and queue-backed pruning
- tests cover duplicate suppression and later acceptance after expiry

Python reference:

- Python cleans up discovery-path state over time and ties it to explicit timeout paths in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:723)

Impact:

- The original unbounded-lifetime duplicate suppression gap is closed.

### 23. Recursive path throttling is global instead of interface-aware

Status: implemented in active workspace for recursive request caps and pending
state. Pending recursive requests are counted per ingress interface, with
expiry releasing per-interface capacity.

Area: path discovery

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/path_requests.rs`](crates/libs/rns-transport/src/transport/path_requests.rs) keys recursive pending state by `(destination, interface)` and tracks pending counts per interface
- tests cover per-interface recursive suppression, queue caps, announce caps, and expiry-based capacity release

Python reference:

- Python keeps richer request state and interface-sensitive path discovery behavior in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:118) and [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:595)

Remaining caveat:

- This closes the confirmed global-throttle bug; broader Python announce
  queueing and interface-mode forwarding behavior remain tracked by issues
  `24` through `28`.

### 24. Interface-side announce queueing and pacing are missing

Status: implemented in active workspace for outbound multi-hop announce
broadcasts. Interfaces now maintain bounded per-interface announce queues,
pace remote announce retransmission by interface bitrate and announce-cap
budget, release queued entries on the transport announce maintenance tick, and
prioritise lower-hop queued announces before farther paths.

Area: announce propagation

Active Rust behavior:

- [`crates/libs/rns-transport/src/iface.rs`](crates/libs/rns-transport/src/iface.rs) stores queued announces on each registered interface, deduplicates by destination, bounds queue length and lifetime, and releases the lowest-hop queued entry first
- [`crates/libs/rns-transport/src/iface_runtime.rs`](crates/libs/rns-transport/src/iface_runtime.rs) computes Python-style announce pacing from serialized packet size, interface bitrate, and announce-cap percentage
- [`crates/libs/rns-transport/src/transport/jobs.rs`](crates/libs/rns-transport/src/transport/jobs.rs) drains queued announces from the existing announce maintenance loop
- tests cover immediate first transmission, later queued remote announces, and lower-hop release priority

Python reference:

- Python queues announces per interface and releases them according to interface bitrate and announce caps in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:1030) and [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:246)

Remaining caveat:

- Rust currently uses a conservative default interface bitrate/cap for generic
  interfaces. Per-interface config file exposure and richer status reporting of
  queued announce depth remain follow-up polish rather than the original
  broadcast-immediately parity bug.

### 25. Ingress-limited held-announce release behavior is missing

Status: implemented in active workspace. Ingress announce limiting is
per-interface, unknown announces can be held instead of dropped, held entries
are capacity-bounded, and release chooses the lowest-hop candidate after burst
pressure clears.

Area: announce propagation

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_limits.rs`](crates/libs/rns-transport/src/transport/announce_limits.rs) maintains per-interface held-announce maps, burst state, release timers, and capacity eviction
- [`crates/libs/rns-transport/src/transport/announce.rs`](crates/libs/rns-transport/src/transport/announce.rs) revalidates and reinjects released held announces
- tests cover per-interface ingress limiting, lowest-hop release, capacity eviction, known-route bypass, and path-response bypass

Python reference:

- Python can hold announces during ingress pressure and later release the best candidate in [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:170) and [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:176)

Remaining caveat:

- This closes the confirmed ingress held-announce gap. Outbound announce
  queueing/pacing is now covered by issue `24`; richer Python-style status and
  per-interface config exposure can still be improved.

### 26. Announce forwarding rules are not interface-mode aware

Status: implemented for active announce broadcast dispatch. Interfaces now
carry Python-style modes, announce broadcasts are filtered through
mode-aware policy, and path expiry uses receiving-interface mode.

Area: multi-interface routing

Active Rust behavior:

- [`crates/libs/rns-transport/src/iface.rs`](crates/libs/rns-transport/src/iface.rs) models full, point-to-point, access-point, roaming, boundary, and gateway modes
- [`crates/libs/rns-transport/src/iface_runtime.rs`](crates/libs/rns-transport/src/iface_runtime.rs) applies mode-aware announce broadcast policy for local and remote announces
- [`crates/libs/rns-transport/src/transport/handler.rs`](crates/libs/rns-transport/src/transport/handler.rs) supplies local-destination and next-hop interface mode context when broadcasting announces
- [`crates/libs/rns-transport/src/transport/path_table.rs`](crates/libs/rns-transport/src/transport/path_table.rs) expires paths according to receiving-interface mode
- tests cover access-point suppression, roaming/boundary forwarding gates, local announce allowance, mode parsing, virtual interface mode inheritance, and mode-specific path expiry

Python reference:

- Python blocks announce forwarding based on interface mode, next-hop interface mode, and attached-interface rules in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:1030)

Remaining caveat:

- This closes the confirmed mode-blind broadcast bug. Outbound announce
  queueing/pacing is now covered by issue `24`; richer Python-style status and
  per-interface config exposure can still be improved.

### 27. Announce retransmit timing and completion policy do not match Python

Status: implemented for active announce retransmission timing. Retransmit
entries use Python's `PATHFINDER_G` 5-second grace, `PATHFINDER_RW` 0.5-second
random window, retry limit semantics, and shorter path-response dispatch
window.

Area: announce/pathfinder behavior

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_table.rs`](crates/libs/rns-transport/src/transport/announce_table.rs) applies a 0.5-second randomized initial rebroadcast window and 5-second grace between retries
- normal announce entries keep Python-style one-extra grace retry behavior before moving to cache
- path-response entries use a shorter direct-response grace and do not consume the normal broadcast entry
- tests cover randomized grace timing, retry completion, and path-response completion behavior

Python reference:

- Python pathfinder announce service uses `PATHFINDER_G`, `PATHFINDER_RW`, separate retry ceilings, and held-announce reinsertion in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:518)

Remaining caveat:

- Rust still drives retransmit checks from the existing async maintenance tick
  instead of Python's exact thread loop shape, but the externally relevant
  timing constants and completion behavior now match the confirmed gap.

### 28. Announce rate limiting is destination-keyed instead of interface-centric

Status: implemented in active workspace. Ingress announce rate state is keyed
by receiving interface, not by destination, and held announce release remains
per-interface.

Area: announce control

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/announce_limits.rs`](crates/libs/rns-transport/src/transport/announce_limits.rs) stores `AnnounceLimitEntry` values in a map keyed by interface hash
- each interface tracks incoming announce frequency samples, burst state, held-release timing, and held announce capacity independently
- path responses and known destinations bypass ingress hold behavior
- tests cover per-interface limiting, held announce priority, and capacity eviction

Python reference:

- Python tracks incoming announce frequency and ingress state on interfaces in [`Reticulum/RNS/Interfaces/Interface.py`](Reticulum/RNS/Interfaces/Interface.py:202)

Remaining caveat:

- This closes the confirmed destination-keyed limiter gap. Config-file exposure
  for Python's full announce-rate tuning knobs can still be improved.

### 29. Route restoration from cached announces is weaker than Python

Status: implemented in active workspace. Reticulum path-table persistence saves
Python-shaped destination and tunnel tables, writes cached announce packets, and
restores route plus destination identity state from the cached announce data.

Area: startup recovery

Active Rust behavior:

- [`crates/libs/rns-transport/src/transport/reticulum_path_store.rs`](crates/libs/rns-transport/src/transport/reticulum_path_store.rs) saves path-table entries together with Python-compatible cached announce files
- restore maps persisted Python interface hashes back to active Rust interface addresses, validates cached announce identity compatibility, restores destination identity state, and reinserts path-table entries
- tunnel-path restore is staged until the tunnel reappears, matching Python's tunnel restoration shape
- tests cover destination-table msgpack shape, cached announce files, route/identity restore, and tunnel path restore after tunnel reappearance

Python reference:

- Python restores cached announce packets into the path table on load in [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:276) and [`Reticulum/RNS/Transport.py`](Reticulum/RNS/Transport.py:291)

Remaining caveat:

- This closes the confirmed route/identity restore gap for active persisted
  path-table support. Broader cache aging and operator status surfaces can still
  be expanded.

### 30. Stamp and ticket options are accepted by the API but do not drive wire behavior

Status: implemented for active daemon sends. Send-time stamp cost,
remembered outbound tickets, and included return tickets now affect LXMF
payload construction before direct or propagated delivery.

Area: LXMF primitives

Active Rust behavior:

- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs) resolves explicit or learned stamp cost, remembered outbound tickets, and generated include-ticket material for each delivery task
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_payload.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_payload.rs) passes those options into payload construction under cancellation-aware `spawn_blocking`
- [`crates/apps/reticulumd/src/lxmf_bridge.rs`](crates/apps/reticulumd/src/lxmf_bridge.rs) adds included ticket field `0x0c`, generates proof-of-work stamps from `stamp_cost`, and generates ticket stamps from outbound tickets
- tests cover include-ticket field encoding, stamp-cost generation, cancellation during stamp generation, outbound ticket stamp generation, and Rust/Python ticket-stamp interop

Python reference:

- Python wires these into real send behavior in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1654), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1663), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:299)

Remaining caveat:

- This closes the confirmed "options parsed but ignored" gap. Broader Python
  router lifecycle behavior for ticket distribution remains tracked by issue
  `32`.

### 31. Inbound stamp enforcement is missing

Status: implemented for active direct and propagation inbound paths. Stamp
policy includes an `enforce` flag, invalid payloads can be rejected, and
non-enforcing mode records invalid stamp diagnostics.

Area: LXMF primitives

Active Rust behavior:

- [`crates/apps/reticulumd/src/inbound_delivery.rs`](crates/apps/reticulumd/src/inbound_delivery.rs) evaluates inbound stamp policy, validates proof-of-work and issued-ticket stamps, rejects invalid enforced payloads, and annotates accepted records with checked/valid/value metadata
- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_delivery_events.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_delivery_events.rs) applies the policy to inbound direct resource and packet delivery
- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_propagation.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_propagation.rs) applies the same policy to propagated LXMF payloads
- tests cover missing stamp rejection, invalid-stamp observation when enforcement is disabled, valid proof-of-work stamps, issued-ticket stamps, and destination-stripped payloads

Python reference:

- Python validates and can reject inbound stamps in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1749), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1761), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:278)

Remaining caveat:

- This closes the confirmed missing-enforcement gap. Full Python router side
  effects around peer ticket lifecycle remain tracked by issue `32`.

### 32. `ticket_generate` does not implement full Python ticket semantics

Status: partial. Active `reticulumd` persists generated inbound tickets,
reuses them until the Python renewal window, suppresses repeated ticket
delivery for the Python interval, remembers signed inbound tickets for outbound
ticket-stamped replies, and prunes expired outbound tickets plus generated
inbound tickets after the Python grace window. Broader Python router lifecycle
semantics are still incomplete.

Area: LXMF primitives

Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/daemon/init.rs`](crates/libs/rns-rpc/src/rpc/daemon/init.rs) stores and reuses generated tickets, tracks recent ticket deliveries, remembers signed inbound tickets, and exposes outbound tickets for send-time ticket stamps
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs) wires `include_ticket` and remembered outbound tickets into active delivery payload construction
- [`crates/libs/rns-rpc/src/storage/messages.rs`](crates/libs/rns-rpc/src/storage/messages.rs) prunes expired outbound tickets immediately and generated inbound tickets after the Python grace window
- tests cover reuse across daemon restarts, regeneration inside the Python
  renewal window, recent-delivery interval suppression across daemon restarts,
  signed inbound ticket remembering, unsigned inbound ticket rejection, and
  delivered include-ticket messages starting the suppression interval

Python reference:

- Python tickets are binary material persisted and reused through the router lifecycle in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1023), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1052), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:282)

Impact:

- The original placeholder behavior is gone for active daemon sends, but the
  full Python router lifecycle around ticket distribution, cleanup cadence, and
  peer side effects still needs deeper parity review.

### 33. Propagation stamp validation is missing

Status: implemented in active workspace for RPC/resource propagation ingest;
test-only seeding helpers remain intentionally raw.

Area: propagated LXMF

Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_propagation.rs`](crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_propagation.rs) canonicalizes propagation payloads and validates trailing propagation stamps when `target_cost` is nonzero.
- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_propagation.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_propagation.rs) calls the canonical propagation payload path before storing remote propagation payloads.
- Tests cover missing, short, mismatched, valid propagation-stamp ingest cases,
  remote ingest through the configured flexibility window, and local
  propagated-message metadata through the same acceptance floor.

Python reference:

- Python validates propagation-node stamps in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2115), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2245), and [`LXMF/LXMF/LXStamper.py`](LXMF/LXMF/LXStamper.py:87)

Impact:

- The original missing-validation gap is closed for active ingest. Full
  propagation-node lifecycle parity remains open under issue `4` and issue `36`.

### 34. Announced inbound stamp cost is discarded

Status: implemented in active workspace; verified by announce ingest/storage
tests and outbound bridge stamp-cost lookup.

Area: peer capability learning

Rust behavior:

- [`crates/apps/reticulumd/src/bin/reticulumd/announce_ingest.rs`](crates/apps/reticulumd/src/bin/reticulumd/announce_ingest.rs) parses delivery and propagation stamp-cost app-data shapes.
- [`crates/libs/rns-rpc/src/rpc/daemon/init.rs`](crates/libs/rns-rpc/src/rpc/daemon/init.rs) persists announce `stamp_cost` and exposes `outbound_stamp_cost_for`.
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_outbound.rs) uses the learned outbound stamp cost when a send request does not explicitly provide one.

Python reference:

- Python updates outbound stamp-cost memory from announce data in [`LXMF/LXMF/Handlers.py`](LXMF/LXMF/Handlers.py:17) and [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:1648)

Impact:

- The original discard bug is closed. Deferred/asynchronous stamp work remains
  separate and is tracked by issue `35`.

### 35. Deferred stamp generation is not implemented

Status: partial. Outbound `reticulumd` now schedules delivery first and builds
signed/stamped wire payloads on a blocking worker inside the delivery task.
Normal and propagation stamp generation are cancellation-aware and check
persisted `cancelled` state from the delivery task. Normal and propagation
stamp work now records `_lxmf.stamp_state` / `_lxmf.propagation_stamp_state`
lifecycle metadata (`generating`, `ready`, `failed`, or `cancelled`) plus
target-cost/error/value context. A full Python-style deferred stamp queue and
separate background stamper ownership model is still open.

Area: LXMF sender lifecycle

Rust behavior:

- active outbound delivery no longer performs stamp generation directly in the
  synchronous RPC scheduling path
- active normal and propagation stamp proof-of-work can stop when the outbound
  message becomes `cancelled`
- tracked resource-backed direct and propagated sends monitor persisted
  cancellation after the resource starts and call `Transport::cancel_resource`
  to send `ResourceInitiatorCancel` plus remove the resource tracking entry
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task_payload.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task_payload.rs) records normal stamp/ticket work state and target cost before and after payload construction
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task_propagation.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task_propagation.rs) records propagation stamp work state, target cost, generated value, and failure details
- `get_outbound_progress` treats terminal normal or propagation stamp work
  states (`failed` or `cancelled`) as authoritative over stale `_lxmf.progress`
  metadata
- `get_outbound_lxm_stamp_cost` and
  `get_outbound_lxm_propagation_stamp_cost` treat terminal normal or
  propagation stamp work states (`failed` or `cancelled`) as authoritative over
  stale target-cost metadata
- tests cover normal proof-of-work and ticket-derived stamp lifecycle metadata
  on the active delivery task, terminal stamp-state progress and cost queries,
  plus active resource cancellation after a late SDK cancel
- no audited daemon path includes a Python-style deferred-stamp work queue,
  retry lifecycle, or separate background LXMF stamper integration

Python reference:

- Python queues deferred normal and propagation stamp work in [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2404), [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2440), and [`LXMF/LXMF/LXMRouter.py`](LXMF/LXMF/LXMRouter.py:2463)

Impact:

- behavior still diverges once high stamp costs require queueing, progress,
  retry, and worker ownership semantics, but request-path blocking risk is
  reduced and cancellation plus lifecycle state are no longer purely
  status-only for active normal stamp generation, propagation stamp generation,
  or tracked resource-backed sends

### 36. Propagation transient-id lifecycle is incomplete

Status: partial. Core propagation packing derives the Python-style transient ID
from destination hash plus encrypted payload, and outbound propagated delivery
now records that ID plus `_lxmf.propagation_packed`,
`_lxmf.propagation_packed_size`, `_lxmf.propagation_packed_base64`,
`_lxmf.propagation_target_cost`,
`_lxmf.propagation_stamp_valid`, and `_lxmf.propagation_stamp_value` metadata
after payload construction. Broader Python message-object lifecycle state such
as deferred worker ownership and retry state remains narrower than the
reference.

Area: propagated LXMF

Rust behavior:

- [`crates/libs/lxmf-core/src/message/wire.rs`](crates/libs/lxmf-core/src/message/wire.rs) derives the transient ID from the unstamped propagation payload bytes
- [`crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task.rs`](crates/apps/reticulumd/src/bin/reticulumd/bridge_delivery_task.rs) persists propagated send transient ID, packed state, packed bytes, packed size, target cost, and generated propagation stamp validity/value into outbound message metadata once the payload is built
- tests cover persistence of the base64-encoded propagation packed payload bytes alongside the lifecycle metadata

Python reference:

- Python derives `transient_id` from destination hash plus encrypted payload and optionally appends a propagation stamp in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:438) through [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:441)

Impact:

- propagated LXMF now exposes the outbound transient ID for observability and
  SDK state, and also exposes the generated propagated payload/stamp lifecycle
  metadata and packed bytes that Python keeps on `LXMessage`; full Python
  propagation message lifecycle parity remains incomplete.

### 37. Inbound daemon decoding drops stamp validity state

Status: partial. Active inbound delivery/resource and locally delivered
propagation paths evaluate the configured stamp policy and annotate decoded
message fields with `_lxmf.stamp_checked`, `_lxmf.stamp_valid`, and
`_lxmf.stamp_value` for accepted stamped messages. The daemon stamp policy now
also supports Python-style `enforce=false`, where invalid stamped messages are
accepted and stored with `_lxmf.stamp_checked=true` and
`_lxmf.stamp_valid=false`. Locally decrypted propagated messages also preserve
validated propagation-stamp status and value under `_lxmf.propagation_stamp_*`
metadata. Full negative-state parity remains narrower for paths where
enforcement is enabled and invalid messages are dropped.

Area: daemon message model

Rust behavior:

- [`crates/apps/reticulumd/src/inbound_delivery.rs`](crates/apps/reticulumd/src/inbound_delivery.rs) validates inbound stamps against configured policy, issued tickets, and proof-of-work stamps
- [`crates/apps/reticulumd/src/bin/reticulumd/inbound_propagation.rs`](crates/apps/reticulumd/src/bin/reticulumd/inbound_propagation.rs) annotates locally delivered propagated messages with validated propagation-stamp metadata before storage
- accepted stamped messages preserve stamp-check status and value under `_lxmf` metadata in persisted message fields
- `stamp_policy_set` accepts `enforce=false` to preserve invalid stamp status on accepted messages instead of dropping them

Python reference:

- Python tracks `stamp_valid`, `stamp_checked`, and propagation stamp validity on the message object in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:160) and validation logic in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:275)

Impact:

- accepted inbound messages can now represent Python-style positive
  stamp-validation outcomes, propagation-stamp outcomes for locally delivered
  propagated messages, and invalid-but-accepted outcomes when enforcement is
  disabled, but full negative-state storage parity remains narrower than
  Python's message object model for enforced drops.

### 38. Inbound timestamp precision is truncated

Status: implemented for client-visible metadata and stable message pagination
in active workspace. The legacy `MessageRecord.timestamp` field remains integer
seconds, but accepted inbound LXMF messages preserve Python's floating payload
timestamp in `fields._lxmf.timestamp_f64` whenever precision would otherwise be
lost, and RPC message cursors use a deterministic `(timestamp, id)` boundary.

Area: daemon API compatibility

Active Rust behavior:

- [`crates/libs/lxmf-core/src/inbound_decode.rs`](crates/libs/lxmf-core/src/inbound_decode.rs) decodes payload timestamps as `f64`
- [`crates/apps/reticulumd/src/inbound_delivery.rs`](crates/apps/reticulumd/src/inbound_delivery.rs) stores `_lxmf.timestamp_f64` metadata for fractional inbound timestamps
- [`crates/libs/rns-rpc/src/storage/messages.rs`](crates/libs/rns-rpc/src/storage/messages.rs) lists messages by `timestamp DESC, id DESC` and supports stable `timestamp:id` cursor pagination for same-second records
- legacy RPC list handlers fetch one extra record before truncating pages so
  `next_cursor` is emitted only when another page exists
- tests cover fractional inbound timestamps in stored metadata
- RPC tests cover `list_messages` cursor pagination across multiple messages
  with the same integer timestamp and exact-limit cursor exhaustion

Python reference:

- Python preserves floating timestamps in payloads and unpacked messages in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:367) and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:752)

Remaining caveat:

- legacy RPC `MessageRecord.timestamp` values still expose integer seconds;
  clients that need Python payload precision must read `_lxmf.timestamp_f64`

### 39. Inbound title/content decoding loses binary fidelity

Status: implemented for accepted inbound messages in active workspace. The
public title/content fields remain UTF-8 strings, but non-UTF8 title/content
bytes are preserved as base64 metadata under `_lxmf`.

Area: daemon API compatibility

Active Rust behavior:

- [`crates/libs/lxmf-core/src/inbound_decode.rs`](crates/libs/lxmf-core/src/inbound_decode.rs) keeps decoded title/content as raw bytes
- [`crates/apps/reticulumd/src/inbound_delivery.rs`](crates/apps/reticulumd/src/inbound_delivery.rs) stores `_lxmf.title_base64` and `_lxmf.content_base64` when UTF-8 conversion would lose bytes
- tests cover non-UTF8 title/content metadata preservation

Python reference:

- Python stores raw bytes and only decodes on explicit request in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:204), [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:213), and [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:792)

Remaining caveat:

- legacy RPC string fields cannot carry arbitrary bytes directly; binary-aware
  clients must read the `_lxmf` metadata copies

### 40. Outbound field-shape handling is stricter than Python

Status: implemented for the confirmed attachment alias gap in active workspace.
Outbound send parsing accepts canonical `attachments`, legacy `files`, and raw
wire key `5`, rejects only ambiguous mixed aliases, and the wire-field encoder
normalizes accepted aliases to the Python field id.

Area: wrapper and client compatibility

Active Rust behavior:

- [`crates/libs/rns-rpc/src/rpc/send_request.rs`](crates/libs/rns-rpc/src/rpc/send_request.rs) accepts `attachments`, `files`, or raw `5` attachment fields
- [`crates/libs/lxmf-core/src/wire_fields.rs`](crates/libs/lxmf-core/src/wire_fields.rs) normalizes public aliases to raw field id `5` and preserves raw numeric field keys
- tests cover legacy `files`, raw `5`, and mixed-alias rejection

Python reference:

- Python accepts arbitrary field dicts in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:220)

Remaining caveat:

- Python accepts arbitrary field dicts beyond the confirmed attachment aliases;
  newly discovered client-specific field conventions still need case-by-case
  interop tests

### 41. Custom storage encoding can break Python `.lxm` interchange

Status: implemented in active workspace. `WireMessage::pack_storage()` now
emits the Python-style msgpack container with `state`, `lxmf_bytes`,
`transport_encrypted`, `transport_encryption`, and `method`; `unpack_storage()`
continues to read Python containers, raw wire bytes, and the older Rust-only
`LXMFSTR0` format for backward compatibility. Callers that need specific
Python container metadata can use `WireMessage::pack_storage_container()`.

Area: storage interoperability

Active Rust behavior:

- [`crates/libs/lxmf-core/src/message/container.rs`](crates/libs/lxmf-core/src/message/container.rs) models the Python storage container
- [`crates/libs/lxmf-core/src/message/wire.rs`](crates/libs/lxmf-core/src/message/wire.rs) emits the Python-compatible container from `pack_storage()`, exposes explicit container metadata via `pack_storage_container()`, and keeps legacy `LXMFSTR0` decoding only
- tests cover Python-container decode, default Python-container emission, and explicit state/method/transport metadata emission

Python reference:

- Python persists a msgpack map with LXMF metadata in [`LXMF/LXMF/LXMessage.py`](LXMF/LXMF/LXMessage.py:655)

Remaining caveat:

- storage emission now matches Python's container shape; broader `.lxm`
  interoperability still depends on the underlying packed LXMF wire semantics

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
- peer status now exposes Python-style runtime counters for
  `last_sync_attempt`, `next_sync_attempt`, `sync_backoff`, `rx_bytes`,
  `tx_bytes`, `acceptance_rate`, and per-peer message accounting
  (`offered`, `outgoing`, `incoming`, `unhandled`)
- peer status now reports Python-style `peering_key` values when the peer and
  local identity hashes are known and the remote peering cost is known, but full
  peer transfer and peering behavior is still a major gap
- Python-style propagation status now prefers per-peer `peering_timebase`,
  `propagation_transfer_limit`, `propagation_sync_limit`,
  `propagation_stamp_cost`, `propagation_stamp_cost_flexibility`, and
  `peering_cost` values when known instead of flattening every peer to the
  daemon-wide defaults
- Python-style propagation status reports daemon elapsed uptime instead of Unix
  epoch seconds
- Python-style propagation status and propagation-node announce app-data now
  use configured node transfer limits for `delivery_limit`,
  `propagation_limit`, and `sync_limit` instead of fixed defaults
- peer status now matches Python's zero-offer acceptance-rate semantics by
  reporting `acceptance_rate=0.0` until at least one outbound offer/activity is
  observed

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
