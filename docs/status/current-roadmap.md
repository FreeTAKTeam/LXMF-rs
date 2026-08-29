# Current Roadmap Status

Last reassessed: 2026-08-29

This file is the repository-level source of truth for parity posture, release
confidence, and execution order. Detailed row-level status lives in:

- `docs/status/reticulum-parity-matrix.md`
- `docs/status/lxmf-parity-matrix.md`
- `docs/status/software-parity-ledger.md`

The software parity ledger maps software/protocol/runtime parity rows into
implementation-ready work packets and explicitly defers hardware/HIL and
external-client evidence.

Historical plans and issue lists explain how work was approached; they do not
override these status files.

## Current Position

LXMF-rs retains the v0.9.5 SDK-access baseline and now reaches complete
software-surface parity against Python RNS 1.5.2 at
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`. The 1.5 alignment adds bounded
priority ingress queues, early filtering and protocol accounting, in-flight
path-request batching, negotiated Channel/Buffer MDU use, discovery operator
addresses, queue/interface/link telemetry, and medium-bitrate timeout accessors.
The exact upstream-to-Rust disposition is recorded in
[`rns-1.5-delta.md`](rns-1.5-delta.md).

The v0.10.0 baseline is superseded by stable v0.10.1, merged on `main` at
`25a976945cb335dff3be692981151c8741a5fdeb` and published at
[`v0.10.1`](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.10.1). The
maintenance release carries the exact RNS 1.5.2 pin, queue/keepalive/shared-
instance fixes, IFAC/profiling parity surfaces, and owned-buffer Resource
sender equivalent. Platform artifacts, provenance, independent interop, OCI,
performance evidence, and all 17 public crates are published for that immutable
boundary. The performance gate is `pass_with_warnings` with throughput `1.013x`,
CPU `1.010x`, and peak RSS `1.084x` versus the same-runner v0.9.1 baseline; its
sole warning is the documented 13.99% Rust resource-sized message-encode
dispersion.

The project is best described by capability level:

| Capability | Status | Meaning |
| --- | --- | --- |
| Wire compatible | achieved | Core Reticulum packet/identity primitives and LXMF message encodings are implemented and tested. |
| Direct-message interoperable | achieved | Selected bidirectional Rust/Python direct, link, channel, paper, and daemon paths are exercised in CI. |
| Propagation interoperable | achieved | Propagated delivery, complete Python-only `LXMPeer.py` lifecycle coverage, and Python-reference propagation router fetch/download/sync lifecycle coverage are implemented and tested. |
| Operationally substitutable | achieved against RNS 1.5.2 | The software-controlled runtime includes the 1.5 ingress, routing, telemetry, timeout, discovery, dataplane-control, keepalive, IFAC, profiling, and `rngit` slices. |
| Full Python software surface parity | achieved | The strict inventory reports 1,857 complete, 0 partial, and 1 provenance-backed not-applicable entry. |
| ZeroMQ SDK-access parity | achieved in v0.9.5 implementation | Generated classification and daemon-operation inventory live in `sdk-zmq-parity.json`; release evidence must still pass all gates. |
| Independent implementation evidence | published for stable `v0.10.1` | Pinned rns-rs and Reticulum-Go release profiles cover two-node/multi-hop behavior; rns-rs additionally covers mixed/all-Rust five-node chains, routing policy, restart, shared daemon, exact large Resources, and deterministic chaos. Explicit peer divergences remain failures owned by the peer and are allowlisted narrowly by CI. |
| Performance evidence | published for stable `v0.10.1` | Tag workflow [`33254264175`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264175) passed with the bounded checksummed JSON, HTML, raw evidence, and regression-gate result. The gate is `pass_with_warnings` for one documented 13.99% Rust resource-sized encode dispersion; throughput/CPU/RSS ratios are `1.013x`/`1.010x`/`1.084x`. |

The independent evidence axis is documented in [`docs/interop`](../interop/README.md).
It does not promote Python parity rows, third-party clients, physical interfaces,
or public-network soak. Pull requests run the bounded rns-rs profile; nightly and
release tiers add both peers, expanded chaos, exact 50 MiB transfers, raw logs,
and standalone JSON/Markdown/HTML artifacts.

The canonical stable performance dataset is
[`docs/performance/v0.10.1.json`](../performance/v0.10.1.json), with its
standalone dashboard at [`docs/performance/v0.10.1.html`](../performance/v0.10.1.html).
The historical v0.10.0 dataset remains available at
[`docs/performance/v0.10.0.json`](../performance/v0.10.0.json).
The v0.10.1 independent interoperability workflow
[`33254264125`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264125)
passed on the immutable release commit and its public reports are attached to
the [`v0.10.1` release](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.10.1).
The v0.10.1 performance workflow is
[`33254264175`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264175)
and passed with the four performance assets attached to the public release.

## v0.9.9 Historical Stable Release

`v0.9.9` is the prior stable release. Its immutable tag resolves to
`51fd3beebdace78d6c7f38748c6bcfe452032559`, and its CI, Verify, Release,
independent-interoperability, performance, leader-readiness, and crates.io
publication workflows passed. The
[`v0.9.9` release](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.9.9)
publishes 35 assets spanning host archives, native packages, checksums, SBOMs,
provenance, and standalone interoperability/performance evidence. Stable
release notes are in
[`docs/release-notes-v0.9.9.md`](../release-notes-v0.9.9.md).

The historical v0.10.0 boundary was 1,839 generated entries. The current
RNS 1.5.2 development boundary is 1,858 entries: 1,857 applicable and complete,
zero partial, zero unmapped, and one provenance-backed
not-applicable entry. The published v0.9.9 tag retains its historical 1.4.2
inventory. Physical interfaces, public networks, and third-party clients remain
separate evidence axes described above.

## v0.10.0 Stable Release

The reviewed implementation aligns the complete software-controlled RNS
surface with Python Reticulum 1.5.0 and is merged on `main` at
`e9111b2621afc31329fa403a61696b7a3d8987f1`. The immutable `v0.10.0` tag
resolves to `5436ee715f94f81e18abb0808cfca52fcd7cc9bc`. Its implementation ledger is
[`rns-1.5-delta.md`](rns-1.5-delta.md), its stable release ledger is
[`v0.10.0-release.md`](v0.10.0-release.md), and its release notes are
[`release-notes-v0.10.0.md`](../release-notes-v0.10.0.md). The historical
candidate record remains [`v0.10.0-release-candidate.md`](v0.10.0-release-candidate.md).

The release, independent interoperability, signing, provenance, OCI, crates.io,
and performance workflows are verified for that exact tag. The performance
workflow passed on attempt 3 after two earlier attempts hit distinct transient
loopback port races; the final publication job attached the checksummed JSON,
HTML, raw bundle, and gate checksum. Homebrew is explicitly skipped because its
tap/token is not configured.

## v0.10.1 Stable Release

The RNS 1.5.2 maintenance release is merged on `main` at
`25a976945cb335dff3be692981151c8741a5fdeb`; immutable tag `v0.10.1` and the
[public release](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.10.1)
resolve to that commit. It carries the exact Python Reticulum 1.5.2 reference
(`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`), the strict 1,858-entry inventory
(1,857 applicable complete, zero partial/unmapped, one not applicable), and
the queue, shared-instance, keepalive, IFAC, profiling, and Resource sender
parity slices described in [`rns-1.5-delta.md`](rns-1.5-delta.md).

PR [#584](https://github.com/FreeTAKTeam/LXMF-rs/pull/584) passed the hosted CI,
Verify/HIL, and independent-interoperability checks before merge. The release
workflow [33254264203](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264203),
crates.io publication workflow
[33255408925](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33255408925),
and independent interoperability workflow
[33254264125](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264125)
passed; Homebrew was skipped because `HOMEBREW_TAP_TOKEN` is not configured.
The tag-triggered performance comparison [33254264175](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264175)
passed with the documented bounded warning and attached its checksummed JSON,
HTML, raw bundle, and checksum to the release; the gate result is recorded in
the stable release ledger.

## v0.9.9-rc.6 Historical Release Candidate

The final candidate was `v0.9.9-rc.6`, with workspace and publishable crate
version `0.9.9`. It was the RNS 1.4.2 software-parity prerelease: the pinned
Reticulum reference is `b48b96e61676504e0a4e527b33b9a0b4495c6872`, the pinned
LXMF reference is `727830cefda83d9c6e3982b48675425f3f988f9c`, and the generated
strict inventory target is 1,811 total, 1,810 complete, zero partial, and one
not-applicable entry.

This RC reconciles the maintained matrices and release metadata with the
implementation already present on `main`, then exercises the exact local and
tag-triggered release gates. The seven tracked LXMF rows and the generated RNS
callable inventory are complete on the software axis. Physical RNode/RNodeMulti,
Weave, VR-N76, BLE, serial-radio, public-I2P/public-Reticulum, and
third-party-client evidence remains separately tracked and does not become a
hidden implementation partial.

Historical candidate notes are in `docs/release-notes-v0.9.9-rc.6.md`; the exact gate,
artifact, signing, OCI, and performance record is in
`docs/status/v0.9.9-release-candidate.md`.

## v0.9.8 Historical Release

v0.9.8 is a historical stable release over the v0.9.7 baseline. Its release
notes and candidate ledger remain historical records of the exact v0.9.8
boundary, including its original resource-transfer, MTU, propagation, SDK
deadline, lock-scope, metadata, cancellation, and compression evidence.

Resource-layer wire fidelity now includes outbound bz2 auto-compression
matching `Resource.__init__`'s default, completing the existing inbound
decompression path. This is a row-level increment recorded in
`docs/status/reticulum-parity-matrix.md`; no capability status in the table
above changes.

Split-resource receive now strips the metadata block from the first segment
only, matching `Resource.py`'s own asymmetry between the metadata flag (set on
every segment, so the receiver can size the whole transfer) and the
length-delimited block itself (prefixed to segment 1 alone). This is a
row-level increment recorded in `docs/status/reticulum-parity-matrix.md`; no
capability status in the table above changes.

The v0.9.8 release record is not the active stable-release record. Its historical
candidate notes live in `docs/release-notes-v0.9.8.md`; its evidence ledger is
`docs/status/v0.9.8-release-candidate.md`.

## v0.9.6 Stabilization Baseline

v0.9.6 is a patch-level hardening release over the v0.9.5 software-parity
baseline. The RNS 1.4.2 re-pin reopens the software-parity release gate; its
new behavior must be implemented or explicitly deferred before a complete
parity claim can be restored.

Current candidate work includes link-context fan-out through each link's bound
interface, packet-cache-correlated single-destination delivery proofs, real-link
plain-resource routing evidence, and identified-peer propagation behavior.
Transport-disabled runtimes now enforce Python's `enable_transport = false`
contract across known-path Link Requests and established-link transit traffic
without blocking locally hosted destinations.
Fan-out now has additive reporting APIs that distinguish no matching link,
complete delivery to interface queues, and partial packet-build or dispatch
failure. Core and transport `RnsError` values implement the standard Rust error
traits. Packet-ingress workers now stop when their transport receive queue
closes, and the issue-369 scanner rejects ignored channel sends in single-line
and multiline forms as well as mutex-poison branches that discard failure.
Identity and address-hash parsing now rejects malformed or overlong key
material without panic/default substitution, LXMF message-ID encoding has a
fallible path used by protocol/runtime code, policy lookups fail closed, and
database migrations plus persisted JSON reads preserve their actual errors.

The v0.9.6 hardening gate and publication remain historical baseline evidence.
Its release notes and audit are retained for historical comparison while the
current release candidate is tracked above.

The 2026-07-10 integration pass reconciled the outstanding parity branches as
one compatible Rust surface. Reticulum configuration now keeps
`enable_transport` independent from strict `panic_on_interface_error` startup,
and daemon RPC exposes next-hop/interface/first-hop timeout metadata,
`link_count`, blackholed-identity state, and shared-instance status without
collapsing absence into malformed data. LXMF delivery keeps atomic
single-destination policy mutators alongside the broader Python convenience
surface, reuses identified direct backchannels, preserves delivery-trace and
opportunistic packet metadata, and resolves conversation display names from
durable delivery announces even when delivery announces are intentionally not
promoted to propagation peers. Standard delivery announce encoding now has one
shared core helper that preserves the Python-compatible display-name and stamp
slots while placing optional application capability metadata in an extension
slot, so applications do not need daemon-local or callsign-based encoders. The
`rnx` soak harness now discovers delivery
destinations from that durable announce surface, enables transport forwarding
for mesh nodes, and requires full-mesh destination visibility before delivery.
Inbound routing now resolves full-wire packets and resources received over an
outbound LXMF delivery link back to the local delivery destination, which keeps
direct backchannel reuse bidirectional in two-node and multi-hop mesh runs.

## v0.9.5 Complete ZeroMQ SDK and Measured Performance

v0.9.5 keeps software implementation parity separate from transport access.
The release requires the generated 1,665-row SDK-access classification, the
daemon operation inventory, canonical concurrent ROUTER/DEALER transport,
native async events, typed RNS/LXMF control traits, five-node mesh and soak
evidence, and generated performance documentation from pinned Python revisions.

The SDK contract release is additive `v2.6`; schema namespace `v2`, protocol
version `2`, and v2.5 request compatibility remain unchanged. Pure wire and
cryptography helpers remain local Rust APIs.

## v1.0 human and equipment boundary

Physical RNode/RNodeMulti, Weave, VR-N76, BLE/serial/radio validation, public
I2P/network soak, third-party clients, manual mobile/operator workflows, and
interactive signing ceremonies are explicitly deferred to v1.0. Until then
they remain hardware-unverified or human-validation targets and are not v0.9.5
release blockers.

## v0.9.0 Full Software-Parity Baseline

The v0.9.0 release criterion was zero partial or unmapped entries in the
generated pinned-Python surface manifest. The scope is the full public
Reticulum and LXMF software surface, official utilities, and idiomatic Rust
equivalents. `docs/status/python-surface-parity.json` is generated from the
pinned references and `docs/status/python-surface-mapping.json`; CI rejects
stale or unmapped entries.

The RNS 1.2.2 baseline recorded **1,664 implementation-complete, 0 partial,
and 1 provenance-backed not-applicable entry across 1,665 entries**. The
current RNS 1.5.2 inventory records **1,857 implementation-complete, 0
partial, and 1 provenance-backed not-applicable entry across 1,858 entries**.
Documentation CI checks inventory drift; the release gate continues to require
zero partial entries. SDK negotiation and daemon runtime status expose this as
an advisory consumer orientation with separate overall, Reticulum, and LXMF
checkpoints; capability negotiation and runtime feature checks remain
authoritative for behavior.

Implementation and evidence are independent. Hardware-capable interfaces may
reach `implementation=complete` with deterministic simulation while retaining
`evidence=hardware-unverified`. The release does not claim physical-device,
public-network, or third-party-client validation. Daemon consumers continue to
use `ZmqPipelineBackendClient`; embedded hosts use
`lxmf-runtime::InProcessBackend`, both through the same `SdkBackend` contract.

Scoped release evidence is split as follows:

| Evidence slice | Release use | Acceptance boundary |
| --- | --- | --- |
| LXMF send/receive | Proves typed SDK delivery, receipt/status, history, inbound event, paper, and propagation-control behavior exercised through `ZmqPipelineBackendClient`. | Software and pinned Python-reference evidence support only the named scenario and mapped manifest entries. |
| Carrier attach/announce software | Proves daemon/runtime interface attach, local shared-instance attach, AutoInterface carrier lifecycle, I2P fake-SAM or real-SAM attach, and announce/path fanout where the carrier is software-controlled. | Supports interface readiness for the implemented software carriers only; it does not claim broad physical-network parity. |
| Optional HIL | Adds confidence for RNode, RNodeMulti, Weave, VR-N76, BLE, and prepared-host carrier/device combinations. | Useful release evidence, but optional for the v0.9.0 software-parity release and tracked independently from implementation status. |

## Strong Areas

### Reticulum

- Identity, destination, packet, cryptography, link, resource, and buffer
  behavior are the strongest RNS areas.
- Link establishment, proof validation, interface binding, watchdog timing,
  teardown, receipts, and resource lifecycle have active regression coverage.
- Resource fragment requests now scope hashmap exhaustion to the current
  request window and gate on an outstanding update, matching
  `Resource.request_next`/`waiting_for_hmu`. Before this, a receiver signalled
  exhaustion on every round for any resource larger than one hashmap segment,
  which walks the Python sender's `receiver_min_consecutive_height` serving
  window past the fragments being requested; measured against a real NomadNet
  node, a 46 MB transfer stopped after 8 of 2260 fragments and timed out,
  where it now completes segment after segment. `RNS/Resource.py` is
  reclassified `partial` on the back of this.
- Resource fragment scheduling is now adaptive on Python's own ladder: the
  window grows per clean round and shrinks per failed one, with a ceiling
  that steps between the slow, very-slow and fast maxima on measured rate.
  Measured against a real NomadNet node, the same 46 MB fetch runs at 234
  fragments/s where a fixed window of 4 managed 84.
- `Link::request_packet`/`response_packet` complete the request/response
  pair: the receive half already decrypted both contexts, but nothing could
  build either, so a peer had to send every request and every reply as a
  resource transfer even when the packed form fits a single packet. Python
  chooses per message (`Link.request`/`handle_request`); the choice is the
  caller's here, and the crate now exposes both options. Note the id
  asymmetry — a packet-borne request has no id field, so the responder
  derives one from the packet hash.
- Cached remote path responses now keep the cached announce payload while
  stamping the direct response packet as `PATH_RESPONSE`, aligning another
  Python announce/path discovery edge policy.
- Known-path `PATH_RESPONSE` work now preempts any due ordinary announce for
  the same destination and then releases the ordinary announce on the next
  retransmission drain, matching Python's `held_announces` edge ordering; this
  is now covered by both a deterministic announce-table regression and a
  harness-dispatchable local transport-policy evidence case.
- Unknown path requests now retain the requesting interface while recursive
  discovery runs, then send an immediate direct `PATH_RESPONSE` when a matching
  announce arrives, matching Python's waiting discovery request behavior.
- Matching announces now also consume waiting unknown-path discovery requesters
  and release the requester interface's recursive discovery capacity for later
  unknown-path requests.
- Recursive path requests now obey Python's interface announce pacing gates:
  queued announces or an active announce cap block the request, while a
  recursive request admitted by the gate advances the next allowed
  announce/path slot.
- Path-request duplicate/throttle state now has bounded software coverage:
  inbound duplicate request suppression is scoped by destination, requesting
  transport, request tag, and ingress interface and expires after the request
  timeout; local path-response suppression is scoped by destination, requesting
  transport, request tag, and egress interface; and recursive discovery capacity
  is tracked per source interface and released after the request timeout. This
  is path-request policy evidence only, not a full transport-runtime parity
  claim.
- Unknown recursive path discovery now follows Python's `DISCOVER_PATHS_FOR`
  interface-mode gate: only access-point, gateway, and roaming interfaces
  forward unknown-path discovery, while full, point-to-point, and boundary
  interfaces do not retain waiting discovery requesters.
- Incoming announces now retain Python's random-blob emission time for path
  replacement: duplicate/stale blobs are ignored, fresh same-hop or better
  announces can refresh known routes, and expired or newer higher-hop announces
  can replace the active path in software-only transport tests.
- Never-activated outbound links now trigger Python-style path rediscovery:
  the stale path is expired, rediscovery requests are throttled by the
  `PATH_REQUEST_MI` window, and shared-instance clients leave rediscovery to
  the shared instance.
- Routed link-table proof timeouts now model Python's unresponsive-path
  exception: one-hop or topology-change routes are marked unresponsive,
  rediscovery requests avoid the ingress interface, and equal-timebase
  higher-hop announces can replace the unresponsive path.
- Intermediate-hop `LINKREQUEST` forwarding now rewrites existing configured
  software link MTU signalling by preserving mode bits and clamping to the
  software ingress/next-hop interface MTU ceiling, while Python-default
  500-byte signalling and un-signalled requests remain unmodified; this now
  has harness-dispatchable local transport-policy evidence.
- Known-path requests on roaming interfaces also suppress direct path answers
  when the learned next-hop iface is the same roaming iface, matching Python's
  loop-avoidance behavior; this now has harness-dispatchable local
  transport-policy evidence alongside the focused transport regression.
- Roaming-interface known-path responses that are not same-interface loops now
  wait Python's extra roaming grace before answering, keeping opportunistic
  path discovery from racing roaming peers too aggressively; this now has
  harness-dispatchable local transport-policy evidence at the transport
  boundary.
- Pending ordinary announce rebroadcasts now complete early when a later
  incoming transport announce proves the rebroadcast has already been passed
  onward, while retaining cached announce material for known-path responses.
- A remote announce with no next-hop interface on file is now blocked on every
  outgoing interface mode, matching the rung `Transport.py`'s per-interface
  announce ladder opens with ("Blocking announce broadcast on <iface> since next
  hop interface doesn't exist"). Only roaming and boundary rejected a missing
  next hop before, and they did it as a side effect of matching `Some(..)`, so
  the other four modes carried it. The rung is scoped to remote announces: a
  destination this node owns has no next hop by definition and stays
  announceable.
- Transport announce rebroadcasts now have deterministic handler-boundary
  and local transport-policy evidence that the learned next-hop interface mode
  drives Python-style outgoing mode policy, including access-point suppression
  and roaming/boundary loop avoidance.
- Per-interface `announces_from_internal` and `announces_to_internal` now reach
  that decision, matching `Interface.py:122-123` and the reads at
  `Transport.py:1417` and `Transport.py:1430`. An interface that opts out of
  `announces_from_internal` refuses a remote announce whose next hop is an
  internal-mode interface, and an internal-mode interface refuses one that
  reached this node over a boundary unless that boundary sets
  `announces_to_internal`. Both keys parse from an interface's own config and
  arrive in `InterfaceSharedConfig` on the startup and hot-apply routes, and
  applying shared config to a live interface reaches the virtual children
  already registered on it, so a discovered peer carries the new policy without
  being recreated.
- Announce-rate target rebroadcast suppression now has harness-dispatchable
  local transport-policy evidence that rapid repeats are allowed through the
  configured grace window, then suppressed until the target interval reopens.
- Unknown-announce ingress limiting now has harness-dispatchable local
  transport-policy evidence for Python-style per-interface holding and
  lowest-hop release, so bursty unknown announce traffic on one ingress no
  longer stands in for all interfaces in the parity matrix.
- A node that will not retransmit no longer files the announces it hears into
  the retransmission queue, matching `Transport.py:2267`'s
  `(transport_enabled() or is_from_local_client) and context != PATH_RESPONSE`
  guard on the `announce_table` insert. Those announces are cached rather than
  dropped, because this crate rebuilds a path entry's announce packet from the
  announce table when persisting where Python stores a packet hash. A cached
  announce that supersedes a queued one refreshes it in place, so persistence
  and later path responses read the announce the path table accepted, and
  refreshing a destination the cache already holds no longer evicts an
  unrelated one. Measured against a public hub over six minutes, the queue grew
  14 -> 317 and never decremented; it now stays at 0 while the bounded cache
  holds the same routes. Python's local-client announce timing
  (`retransmit_timeout = now`, `retries = PATHFINDER_R`) is not implemented, and
  the shared-instance condition reads the receiving interface rather than
  Python's parent-interface `is_local_client_interface`.
- Restored Reticulum path-table announces are now cache-only lookup material at
  startup, not fresh rebroadcast work, while still serving known-path response
  requests from the restored cache.
- `reticulumd` bootstrap now has software evidence that restored Python-format
  path-cache material is visible through daemon `path_status` and already-known
  `request_path` RPC after restart, that restored cached-announce identity keys
  are persisted for daemon announce-identity lookup, and daemon status reports
  `_runtime.reticulum.path_table_restore.status` as `ok` or `error` so corrupt
  `destination_table` state remains observable without making startup fatal.
  The same daemon status payload now exposes
  `_runtime.reticulum.path_table_restore.skipped` counters for per-reason
  active/tunnel restore skips, including unmapped interfaces, expired rows,
  missing or invalid cached announces, mismatched cached destinations, duplicate
  tunnel packet hashes, and identity conflicts.
- Reticulum path-table persistence now writes only routes with cached announce
  material and restore hardens Python-format active and tunnel path-table rows
  by ignoring stale/expired path rows, so restart bootstrap cannot revive
  resolver routes without usable identity/cache material.
- Graceful `reticulumd` shutdown now forces a final Reticulum path-table
  persistence pass, so recently learned announce/path state does not rely on
  the debounce worker firing before process exit.
- Reticulum path-table restore now treats active and tunnel path-table rows
  with missing cached announce files, active and tunnel path-table rows with
  malformed cached announce files, and active/tunnel rows whose cached announce
  belongs to a different destination as unusable rows instead of aborting the
  whole restore; `reticulumd` bootstrap/status tests now cover the missing
  active/tunnel cached-announce rows alongside the existing malformed-cache
  daemon evidence.
  Malformed `destination_table` and `tunnels` files remain observable daemon
  restore errors.
- Shared-instance clients skip local Reticulum path-table save and restore
  work, matching Python's shared-instance bootstrap/persistence boundary.
- Tunnel-only restored announces are retained as cache material so paths
  restored on tunnel reappearance can answer later known-path requests, and
  tunnel path restore now carries Python-format random-blob windows while
  respecting active-path freshness, hop count, and expiry before replacing a
  route, including explicit evidence for both preserving a fresher active route
  and replacing it with fresher restored tunnel state.
- `reticulumd` supports TCP client/server, including Python-style
  TCP-over-I2P `i2p_tunneled` socket tuning for outbound clients and accepted
  server streams and Python-style `fixed_mtu` falsey/default and Reticulum
  MTU lower-bound validation, TCP/Backbone listener `SO_REUSEADDR` parity,
  Backbone TCP/HDLC listener/client compatibility with Backbone MTU defaults
  and Reticulum-style Backbone socket tuning
  (`TCP_NODELAY`, Linux/Android keepalive probes, and TCP user timeout) plus
  Backbone-only HDLC stream liveness keepalives, stale detection, and
  read-timeout reconnects, local slow-reader HDLC tx backpressure evidence
  paired with Python selector/epoll and live Python Reticulum
  `BackboneClientInterface` slow-reader probes in the pinned Python interop
  workflow, and live Rust/Python Backbone channel, link-data,
  request/response, and resource roundtrips in both directions over Python
  `BackboneInterface`/`BackboneClientInterface` against the pinned reference,
  TCP/Backbone client reconnect tunnel re-synthesis, TCP/Backbone listener
  daemon/RPC runtime status with accept counters and latest accepted stream
  snapshots, Python `BackboneInterface` `remote` alias
  parse-to-bootstrap/status coverage as `backbone_client`, LocalInterface
  TCP-loopback plus Unix filesystem
  and Linux/Android abstract AF_UNIX shared-instance listener/client-attach
  compatibility, including Unix client-attach reconnect after startup failures
  or later disconnects and TCP/Unix attach reconnect signals that
  re-synthesize tunnel state, Python-style global `[reticulum] share_instance`
  synthesis when no explicit local shared-instance interface is configured,
  implicit shared local TCP listener coexistence with configured TCP/Backbone
  listeners through a sidecar startup path,
  Python-style `force_shared_instance_bitrate` stream pacing, plus
  shared-instance one-hop transport wrapping, and `status`/`daemon_status_ex`
  now report Reticulum shared-instance mode, flags, endpoint, and interface
  name for active server, attached client, and disabled states,
  LocalInterface TCP and Unix shared-instance software smoke coverage for
  strict startup, TCP listener/attach status, filesystem Unix listener startup,
  Linux abstract Unix listener/client attach, Python local MTU, bitrate alias
  reporting, and `rnstatus-rs` JSON/human output, plus pinned Python Reticulum
  shared-instance attach and Python-origin announce-fanout evidence over TCP
  and Linux abstract Unix sockets. The Reticulum interface parity audit records
  LocalInterface #384 evidence under
  `target/reticulum-interface-parity-audit/report.json` with
  `evidence_scope = "reticulum_interfaces_384_385_parity_audit"` and optional
  `RNODE_HIL_ARTIFACT_MANIFEST` verification for
  `schema = "reticulum_interface_hil_matrix_artifacts.v1"` matrix artifacts,
  Pipe subprocess HDLC with a software fake-subprocess smoke for strict daemon
  startup and refreshed `rnstatus-rs` JSON/human runtime status, UDP
  unicast/multicast plus
  Python-style UDP `device` broadcast-address defaults, IPv4 broadcast socket
  sends, shared-`port` forward fallback semantics, and a software loopback
  smoke for strict startup, bind status, and receive-side decode telemetry,
  serial, KISS, AX.25
  KISS with Android-style beacon alias compatibility plus a software fake-PTY
  smoke for serial KISS/AX.25 KISS startup frames, READY handling, and
  `rnstatus-rs` reporting, Python
  `TCPClientInterface` `kiss_framing = true` parse-to-bootstrap/status
  coverage as `kiss_tcp_client` plus a software fake-TCP smoke for KISS TCP
  startup frames, READY handling, and `rnstatus-rs` reporting, AutoInterface with
  Python-style multicast address type fallback, polling adopted-address
  reconciliation, adopted-interface add/remove/change diff planning,
  daemon-side add/remove lifecycle application for active AutoInterface
  runtimes, supervised discovery receive loops, and supervised link-local
  data-listener restart with tracked replacement shutdown, LoRa/RNode,
  feature-gated RNode BLE, feature-gated VR-N76 KISS-over-BLE, and the
  in-progress shared serial/TCP RNodeMulti baseline with nested vport virtual
  children, a shared-serial Weave WDCL/HDLC endpoint baseline, and an
  outbound I2P SAM peer baseline. Enabled unknown interface kinds remain
  parseable for operator visibility but are covered as explicit failed startup
  records with `unsupported interface kind` runtime metadata.
- Meshtastic tunnel support includes the reference `RETICULUM_TUNNEL_APP`
  chunk metadata, modem-preset pacing, missing-chunk requests,
  node/destination route learning, an injectable bearer handle, daemon TOML
  startup, runtime status refresh, and deterministic loopback lifecycle
  simulation. Native device evidence remains explicitly hardware-unverified.
  Configuration and integration guidance lives in
  `docs/interfaces/meshtastic.md`.
- RNodeMultiInterface has a transport-side vport slice: a single serial or TCP
  RNode endpoint can host nested subinterfaces, select virtual ports with KISS
  `CMD_SEL_INT`, route direct sends to the matching virtual child, and fan out
  broadcasts to children that remain marked outgoing. Startup probe validation
  covers detect, firmware `>= 1.74`, platform, MCU, `CMD_INTERFACES`
  discovery, and configured vports reported by the hardware. Parent-level
  Python `id_callsign`/`id_interval` beacons are carried into the transport and
  fan out as raw callsign data on outgoing subinterfaces after first traffic.
  Runtime status bookkeeping applies selected-vport radio command/status
  responses to the matching child status record, and daemon/RPC snapshots
  refresh the `_runtime.rnode_multi.radio_status` schema from the
  transport-side runtime handle, including stream/probe state, last error
  reporting for absent or failing hardware, accepted or partial startup-probe
  firmware/platform/MCU/interface metadata from non-cancelled probe attempts,
  and the ordinary RNode radio-status fields for each vport. Daemon/RPC can
  queue safe RNode management commands
  through the parent interface with explicit configured child `vport`
  validation; the transport writes `CMD_SEL_INT` before each queued management
  command frame. Software fake-TCP and fake-PTY smokes now exercise strict
  daemon startup, startup-probe status refresh, `rnstatus-rs` JSON/human output,
  and `rnodeconf-rs` vport blink dispatch through the real TCP and serial PTY
  parent paths without hardware, while their reports record
  `software_fake_tcp_rnode_multi` and `software_fake_pty_rnode_multi` evidence
  scopes with product-boundary notes. Display-capable ESP32/NRF52 devices get Python-style
  external-framebuffer disable during teardown before per-vport radio-off and
  leave-host payload `0xff` frames. Clean stream EOF and software stop now
  report `stream_state = "closed"` without masking read/write/probe failure
  states or `last_error`. In strict startup mode, the daemon
  preflights the configured serial port or TCP endpoint and fails closed before
  registering RNodeMulti management targets if the parent endpoint is
  unavailable. Prepared-host reports explicitly mark their scope as
  `prepared_host_single_device_vport_probe`, proving one configured endpoint
  and vport set without claiming broad production parity across device,
  firmware, and radio combinations.
- Ordinary serial/TCP and feature-gated BLE RNodeInterface status now refreshes
  the transport-side RNode probe/radio state into daemon/RPC
  `_runtime.lora.rnode_status`; compact `rnstatus-rs` output summarizes
  bearer, online/detected state, firmware, radio configuration, counters,
  battery, hardware errors, and last command error. Python `RNodeInterface`
  alias configs now have parse-to-bootstrap/status coverage as `lora` with
  `_runtime.lora.rnode_status`. An opt-in prepared-host
  smoke harness records serial/TCP/BLE RNode lifecycle evidence under
  `target/rnode-hil/` with bearer-scoped `evidence_scope` values for serial,
  TCP/Wi-Fi, and BLE prepared endpoints; that same prepared-host gate now
  dispatches safe `rnodeconf-rs query-radio-state` and `blink` management
  commands through the live daemon binding, records their queued JSON results,
  and captures a post-management status snapshot that must remain online,
  radio-on, and command-error free. The transport-side serial/TCP LoRa status
  now also reports native safe-management metadata for SDK/daemon consumers:
  supported safe commands, guarded persistent/destructive command boundaries,
  queue depth/capacity/closed state, accepted and failed operation counters,
  the last queued or failed operation ID/command/state, and the last
  management error. A software-only RNode BLE smoke records
  `evidence_scope = "software_rnode_ble_fallback_management"` under
  `target/rnode-ble-software-smoke/` for feature-gated fallback, command-monitor,
  management dispatch, outbound RNode BLE MTU rejection and MTU-sized transmit,
  `reticulumd` daemon `RnodeBle` management bridge dispatch, `rnodeconf-rs`
  extended management command-to-RPC coverage, persistent/destructive CLI guard
  enforcement, and shared closed-queue cleanup regressions. A fake TCP RNode smoke records
  `evidence_scope = "software_fake_tcp_rnode_prepared_host_management"` by
  running the ordinary prepared-host path against a deterministic local KISS TCP
  peer and verifying startup, radio configuration, radio-state query, and blink
  management frames reached the peer. The Reticulum interface parity audit
  combines RNode BLE #385 software evidence with serial, TCP/Wi-Fi, and BLE prepared-host RNode hardware reports before allowing a strict full-parity
  pass; `tools/scripts/reticulum-interface-hil-matrix.sh` collects those three
  bearer reports under `target/rnode-hil/matrix/` and writes
  `target/reticulum-interface-hil-matrix/report.json` with
  `evidence_scope = "reticulum_interfaces_384_385_hil_matrix"` plus
  `target/reticulum-interface-hil-matrix/artifact-manifest.json` with SHA-256
  digests. Nightly HIL
  exposes the same path through the `reticulum-interface-matrix` profile, and
  strict reports must include endpoint,
  bearer, firmware, platform, MCU identity, and capture provenance fields.
  Display-capable BLE
  RNode shutdown now disables the external framebuffer before radio-off/leave
  frames. Android configured RNode BLE reconnect now excludes the failed
  configured peripheral from the fallback scan, while still allowing alias and
  service-UUID fallback matches with stable log context. Serial/TCP RNode streams now expose a
  transport-local management dispatch handle that writes
  pre-encoded KISS command frames through the live KISS runtime; feature-gated
  BLE RNode streams expose the same management dispatch through the Nordic UART
  write path with BLE chunking. The first covered operations are radio-state
  query and blink indication, backed by duplex/mock tests, daemon
  `rnode_management` RPC dispatch, reticulumd bridge dispatch tests,
  `rnodeconf-rs` mock-RPC CLI tests, and prepared-host safe-management
  dispatch artifacts when the serial/TCP/BLE HIL gate is enabled. The
  daemon/tool path now also queues safe
  config/ROM read, display, NeoPixel, and interference-avoidance controls.
  Daemon RPC and `rnodeconf-rs` also queue guarded persistent/destructive RNode
  controls for Bluetooth, config save/delete, ROM write/wipe, hard reset,
  firmware metadata, and Wi-Fi settings, with explicit persistent/destructive
  confirmation params.
  Frame-level helpers exist for Bluetooth control,
  display/NeoPixel controls, interference-avoidance control, Wi-Fi settings,
  config save/delete, firmware-update metadata, and ROM/EEPROM read/write/wipe
  requests.
- A bearer-neutral `RnodeBearerBackend` and single-attempt
  `RnodeBearerKissInterface` now let mobile platform owners provide ordered BLE
  or Bluetooth Classic byte streams while this crate retains shared KISS
  framing, RNode probe/configuration, MTU and flow-control enforcement, runtime
  status, and teardown. Platform backends can retain a conservative write cap
  after ATT MTU negotiation, payload writes wait until radio startup is
  validated, and older firmware that omits only the radio-state echo can enter
  an explicit compatibility mode after every other probe and radio parameter
  matches. Focused no-default-feature tests cover shared BLE/SPP
  framing, notification preservation, empty-read backoff, cancellation-safe and
  idempotent close, close-failure reporting during aborted startup, the
  firmware compatibility boundary, and conservative BLE chunking. Native
  Android callback/resource lifecycle validation, physical RNode BLE/SPP
  lifecycle cycling, and long-running hardware soak evidence remain external
  mobile/HIL gaps and are not claimed by this software increment.
- WeaveInterface has a transport-side WDCL/HDLC slice: a shared serial parent
  can answer discovery, learn endpoint events, register virtual endpoint
  children, receive endpoint packets, write direct endpoint commands, and expose
  refreshed `_runtime.weave.status` metadata with switch, endpoint, log-event,
  byte/frame, target-scoped remote display-frame, and CPU/task/memory
  device-stat fields. Display-frame completion is based on received byte
  coverage rather than highest observed offset, and software cancellation/stop
  now marks the runtime link closed while clearing WDCL connection and endpoint
  state. `rnstatus-rs` renders remote switch ID, byte/frame counters,
  invalid-frame and last-log diagnostics, display dimensions, completion, byte
  progress, color format, CPU/memory, and task-stat counts for operator status
  views, and `rnstatus-rs --weave-display <interface-name>` provides a
  display-focused framebuffer/status subset for operators. The transport has a
  Python-compatible WDCL remote-display service control frame primitive
  (`WDCL_CMD_REMOTE_DISPLAY` enable/disable) covered by software tests, and
  `reticulumd` exposes live dispatch through the
  `weave_remote_display_control` RPC bridge with `weaveconf-rs`
  enable/disable commands. A software fake-PTY smoke now proves signed WDCL
  discovery, connected runtime status refresh, endpoint/display/device-stat
  reporting, `rnstatus-rs --weave-display`, and live `weaveconf-rs`
  enable/disable dispatch through the real daemon path without hardware; its
  report records `software_fake_pty_weave` evidence scope with a
  product-boundary note. An
  opt-in prepared-host smoke harness records
  connected serial evidence under `target/weave-hil/` and can optionally prove
  the live `weaveconf-rs` remote-display enable/disable dispatch against that
  connected device. Prepared-host reports explicitly distinguish
  `prepared_host_connected_serial` evidence from
  `prepared_host_serial_discovery_only` bring-up evidence while keeping broader
  device, firmware, display/status payload, and operator-workflow parity out of
  scope for a single run.
- I2PInterface has a transport-side SAM slice: configured peers get virtual
  unicast children, transient SAM stream sessions, name lookup, HDLC packet
  framing, direct peer sends, broadcast fanout, and transient connectable
  `STREAM ACCEPT` support for incoming peers with private-key persistence when
  `state_path`/`storagepath` is configured. Missing explicit SAM host/port
  config honors Python's `I2P_SAM_ADDRESS` `host:port` environment default
  before falling back to `127.0.0.1:7656`. Persisted private destination keys
  use Python-compatible hashed `.i2p` filenames, prefer existing old-format
  interface-name keys when present, and otherwise use the identity-bound
  new-format key name. Daemon runtime metadata reports the derived `.b32.i2p`
  endpoint for persisted keys and keys generated during startup, plus refreshed
  `tunnel_status` metadata for tunnel state, reconnect attempts, errors,
  counters, keepalive/stale/read-timeout bookkeeping, and bounded recent
  history for closed incoming peers. Local fake-SAM coverage now exercises the
  outbound peer loop through session creation, lookup, stream connect, HDLC
  writes, and refreshed runtime counters, plus the connectable accept loop
  through incoming `STREAM ACCEPT`, virtual child registration, HDLC ingress,
  direct outbound egress over the accepted stream, runtime counters, and
  cleanup. SAM session IDs now include the daemon transport identity when
  available to avoid cross-process collisions on a shared router, and expired
  accept-loop session IDs recreate the connectable session instead of retrying
  a dead ID indefinitely.
- AutoInterface has a live daemon runtime, including discovery, peer lifecycle,
  peer-data sockets, transport ingress, outbound routing, multicast proof
  fallback, supervised discovery/data receive loops, transport-side
  adopted-interface diff planning, daemon-side add/remove lifecycle
  application for active and zero-initial runtimes, and polling link-local
  replacement reconciliation for already adopted interfaces. Replacement-stop
  tasks for dynamically swapped discovery/data listeners are tracked and
  drained during restart, removal, or runtime shutdown. Loopback peer-data
  tests now prove direct per-peer outbound routes stop emitting after
  listener removal/restart and refresh only after a new accepted peer datagram.
  Discovery and peer-data datagrams processed before Python's final-init peering
  wait has elapsed are now ignored, so packets handled before AutoInterface
  comes online cannot create peers, peer-data routes, or rejection events.
  Daemon `_runtime.auto.carrier_runtime` status now records the last
  AutoInterface peer lifecycle job's expired-peer count, reverse peer announce
  count, missing initial multicast echo count, carrier event summary, post-job
  peer count, and peer-data admitted/delivered/decode-failed/RX-closed outcome
  counters in focused software tests. A software-only smoke now records those
  existing transport and daemon AutoInterface regressions under
  `target/auto-interface-software-smoke/` with
  `evidence_scope = "software_auto_interface_runtime"`. This is local runtime
  observability evidence, not broader Wi-Fi/Ethernet/public-network discovery
  parity.
  An opt-in Linux
  namespace prepared-host smoke now records zero-initial add, link-local
  replacement, and removal churn evidence through refreshed `_runtime.auto`
  status with `evidence_scope = "linux_namespace_dummy_churn"`; remaining
  follow-up is broader prepared-host interface churn evidence across real
  Wi-Fi, Ethernet, and platform combinations.
- I2P transport-side tunnel watchdog/status bookkeeping is refreshed into
  daemon/RPC interface status, and `rnstatus-rs` now summarizes outbound,
  incoming, closed, and aggregate byte counters for the tunnel rows. The
  software fake-SAM smoke exercises strict daemon startup, destination
  persistence, a transient outbound `NAMING LOOKUP` failure followed by
  recovered connected peer state with cleared last error, connectable accept
  status, accepted incoming peer visibility, and `rnstatus-rs` JSON/human
  output without a real I2P router, with
  `evidence_scope = "software_fake_sam_i2p_runtime"`. The
  prepared-host smoke can now optionally require configured outbound peers to
  reach `connected` state when `I2P_PEERS` is supplied; its report explicitly
  distinguishes no-peer `sam_connectable_only` evidence from
  `sam_connectable_with_outbound_peers` production evidence. The real-SAM pair
  smoke now records
  `evidence_scope = "sam_connectable_with_outbound_peers_real_pair"` with
  connected dialer outbound and acceptor incoming peer rows for two local
  daemons sharing one router, and can optionally record
  `sam_connectable_with_outbound_peers_real_pair_soak` with periodic
  `rnstatus-rs` samples for bounded single-router stability. The nightly HIL
  matrix includes that pair path through the `i2p-pair` profile and uploads
  `i2p-prepared-host-pair-artifacts`. Broader public I2P peer-set and
  long-running production evidence remain pending.
- Feature-gated VR-N76 KISS-over-BLE now refreshes transport-side runtime
  status into daemon/RPC `_runtime.vrn76.status`; `rnstatus-rs` summarizes
  connected, subscribed, ready, startup-write failure, and queue counters. An
  opt-in prepared-host smoke harness records daemon startup, connected,
  subscribed, ready, and counter evidence under `target/vrn76-hil/` with
  `evidence_scope = "prepared_host_vrn76_ble_readiness"`; broader write,
  indication, disconnect, reconnect, adapter, firmware, and channel-ID
  hardware evidence remains pending.
- UDP now refreshes live bind state, role, last observed peer-route count,
  packet, byte, drop, and error counters in daemon/RPC metadata and
  `rnstatus-rs`. A software loopback smoke now proves Python-style
  `UDPInterface` alias parsing, strict daemon startup, bound loopback status,
  and malformed-datagram `bytes_rx`/`decode_errors` telemetry without external
  network services. `set_interfaces` and `reload_config` now hot-apply
  host-bound or device-bound TCP server listeners, including loopback,
  `localhost`, IPv4 wildcard, concrete, hostname, and device-selected IPv4/IPv6
  addresses, alongside TCP clients and explicit or device-bound UDP listener,
  peer, multicast-bind, and multicast-forward records. Device-bound UDP uses
  Python-style IPv4 broadcast defaults; partial-target and out-of-range-target
  UDP shapes remain restart-required or invalid, and duplicate
  TCP server or UDP binds are rejected before mutation.
  Hot-applied explicit TCP server records attach live daemon/RPC
  `_runtime.tcp.listener_status` metadata, hot-applied explicit UDP records
  attach the runtime iface and refresh live daemon/RPC `_runtime.udp.status`
  counters under focused software tests, and multicast-bind and
  multicast-forward hot-apply go through the transport peer-routing helper
  instead of a bare UDP spawn. Serial
  now refreshes live open/reconnect, HDLC frame, packet, byte, EOF, queue,
  decode, serialize, read, and write-error counters.
  KISS/AX.25 KISS and KISS TCP now refresh live packet, data-frame,
  command-frame, byte, flow-control, queue, AX.25 drop, and error counters. A
  software fake-PTY smoke now proves Python-style `KISSInterface` and
  `AX25KISSInterface` alias parsing, strict daemon startup, KISS startup command
  emission, fake READY handling, refreshed `_runtime.kiss.status`, and
  `rnstatus-rs` JSON/human output without attached modem hardware.
  A software fake-TCP smoke now proves Python-style `TCPClientInterface`
  `kiss_framing = true` alias parsing, strict daemon startup, KISS startup
  command emission, fake READY handling, refreshed `_runtime.kiss_tcp.status`,
  and `rnstatus-rs` JSON/human output without a real Wi-Fi KISS bridge or TCP
  modem.
  BLE GATT now refreshes live connection/subscription, packet, HDLC frame,
  notification byte, payload byte, write-chunk, reconnect, startup phase,
  queue, decode, serialize, read/write, buffer-drop, cleanup, and last-error
  counters alongside configured BLE UUID and lifecycle timeout metadata.
- `rnstatus-rs` now provides a local daemon status utility over the existing
  RPC status surface through TCP or Unix-domain sockets, including JSON output
  plus human interface endpoint details across configured interface families,
  runtime startup state, Auto carrier/link-local state, TCP/Backbone listener
  state, plus UDP, serial, KISS, BLE GATT, I2P, RNodeMulti, Weave, and VR-N76
  status rows and propagation peer state. The legacy `status` RPC projection
  now exposes the same additive daemon/runtime snapshot fields as
  `daemon_status_ex`, including interface, policy, propagation, stamp, delivery
  pipeline, and capability metadata, while preserving the original identity and
  running fields.
- `rnsd` remains a compatibility shim for `reticulumd`, with CLI tests proving
  `RETICULUMD_BIN` override, forwarded arguments/output, and delegated exit
  success/failure status.
- `rnpath-rs` now exercises daemon-backed path lookup through `rnx
  rnpath-smoke`, and its CLI request-path path has mock-RPC coverage for both
  the default TCP endpoint and Unix-domain transport. The smoke starts a local
  four-node mesh and verifies a non-neighbor destination resolves with
  next-hop/interface metadata over the software RPC path, then reissues the
  lookup as a scoped/tagged path request on the learned outgoing interface and
  verifies the daemon echoes the scope fields.
- The pinned Python compatibility matrix now includes
  `rns_path_request_rust_to_python`, a loopback TCP case where Rust
  `reticulumd` starts with an unknown Python delivery path, resolves it through
  `request_path`, reports route metadata through `path_status`, and confirms
  the same path through `rnpath-rs --json`. The companion
  `rns_path_request_python_to_rust` case suppresses Rust startup/periodic
  announces, holds a quiet window where Python still has no Rust delivery path,
  and then proves Python `RNS.Transport.request_path()` can discover the Rust
  delivery destination over the same software loopback path.
- Daemon-backed path requests now preserve optional interface scope and request
  tag bytes from RPC through `reticulumd` into the transport path-request
  generator. Scoped requests dispatch as broadcast path requests on exactly the
  selected interface, and scoped/tagged refreshes still issue even when an
  unscoped cached path already exists; a syntactically valid but non-matching
  interface scope is surfaced as a request failure instead of a silent no-op.
  `rnpath-rs` exposes matching `--on-iface` and `--tag-hex` flags, and `rnx
  rnpath-smoke` now exercises them against the local daemon mesh after learning
  the non-neighbor path's outgoing interface.

### LXMF

- Message wire/storage packing, signatures, propagation packing, paper
  encoding, timestamp precision metadata, binary-field preservation, and
  Python-compatible storage containers are implemented.
- Documented basic LXMF field IDs are exported through `lxmf-wire`, and the
  typed ZeroMQ SDK send path preserves those field keys plus
  `_lxmf_fields_msgpack_b64` for REM/RCH payload compatibility.
- The pinned Python `LXMF.py` module helper surface is now exposed in
  `lxmf-wire`, including delivery app-data display-name and stamp-cost parsing,
  compression support defaults, and propagation-node announce name/cost
  validation with both Python-style boolean and typed Rust diagnostic paths.
- The typed ZeroMQ SDK send and batch-send paths now treat payload `body` as
  message content when `content` is absent, while still preserving `body` in
  fields, so direct-chat links/body text do not get JSON-stringified.
- Delivery modes are honored by the daemon; the old claim that requested modes
  are ignored is obsolete.
- RPC daemon `lxmf.delivery` announce ingestion now wakes stored pending
  direct/default-direct and opportunistic outbound messages for the announced
  destination while leaving propagated, paper, terminal, already-sending, and
  other-destination records untouched. Reticulumd direct/opportunistic peer
  identity misses after delivery path-request timeout now enter nonterminal
  `queued: waiting for announce` state, so later delivery announces can requeue
  them instead of leaving a terminal `failed:*` receipt.
- Destination-level outbound delivery stamp costs learned from Python-style
  `lxmf.delivery` announces are now queryable through `get_outbound_stamp_cost`
  and the `app.delivery.outbound_stamp_cost` SDK envelope operation.
- The in-process propagated-delivery path (`lxmf-runtime` `send_propagated`)
  now mines a real LXMF propagation stamp (ported `LXStamper` workblock HKDF
  in `lxmf-wire` `stamp.rs`) at the Python default target cost 16 instead of
  appending a fixed all-zero stamp, so default-configured relays (minimum
  accepted cost 13) accept these transfers. Remaining gap: the relay's
  announced stamp cost (`pn_stamp_cost_from_app_data`) is not yet plumbed
  into stamp generation, so relays enforcing a minimum above 16 still
  reject this path.
- Direct and propagated resource sends support receipt-state separation,
  timeout/failure propagation, and active resource cancellation.
- Link sends now register packet/resource receipt tracking before handoff and
  accept Python-style link proofs with default packet context, so Python
  delivery receipts can advance daemon-originated sends from `sent:*` to
  `delivered` while preserving resource completion status.
- Raw transport sends now expose the finalized post-encryption packet hash in
  `SendPacketTrace`, plus a pre-dispatch observer for race-free application
  mapping to the packet hash reported by `DeliveryReceipt`.
- The typed ZeroMQ SDK delivery status path now preserves daemon-reported
  retry-attempt counts and reason codes in `DeliverySnapshot`, so REM/RCH can
  inspect retry and recovery state without dropping to raw RPC status calls.
- Ticket validity, renewal, derivation, persistence, and inbound ticket reuse
  are implemented.
- Delivery ticket generation is now exposed through the registered SDK
  operation path as `app.delivery.ticket.generate` with legacy
  `ticket_generate` alias support, preserving ticket interval suppression
  metadata over typed ZeroMQ backend calls.
- Propagation peers have real queue, policy, maintenance, throttling, peering,
  offer-response, source-accounting, and acceptance-rate behavior. These are
  substantial implementations, not SDK-only placeholders.
- Python-style propagation `auth_required` configuration now reaches
  `propagation_enable` and the daemon propagation status, so node-level
  propagation auth policy is visible with the rest of the propagation peer
  policy.
- Python-style propagation control ACL entries now reach `propagation_enable`,
  `allow_control`, and `disallow_control` as normalised 16-byte identity hashes,
  and are visible through propagation status plus typed SDK recovery state.
- Local and remote peer-sync offer-response cleanup now preserves peers and
  propagation queues for retry on retryable or otherwise unexpected numeric
  responses, while still treating access denial and throttling as distinct
  Python paths.
- Retryable numeric local offer responses now mirror payload-backed live queue
  marks into the active peer record snapshot before returning, so restart/export
  state preserves the retry queue even when the serialized snapshot was empty.
- Retryable, throttled, generic failed, malformed-import, and
  bridge-unavailable remote peer-sync paths now perform the same payload-backed
  live and restored queue snapshot mirroring before reporting the failed sync,
  keeping local and remote retry/export behavior aligned.
- Remote peer-sync failure events now include a structured `failure_kind` on
  the top-level event and nested propagation payload, preserving observer-level
  distinctions for throttling, identity, key, data, stamp, not-found, timeout,
  access-denied, and generic failures without changing retry behavior.
- Payload-backed remote failure snapshots now replace stale serialized peer
  queue IDs with live payload-backed marks, so bridge failures do not preserve
  obsolete restart/export work after the underlying payload is gone.
- Zero-cost peer stamp policies now sync unstamped queued offers immediately
  without waiting for absent peering metadata, matching the Python "no stamp
  required" path and avoiding repeated peer-sync postponement.
- Python propagation announce transfer and sync limits are now converted from
  advertised integer or fractional kilobytes into the byte limits used by
  peer-sync queue selection, so valid queued payloads are not misclassified as
  transfer-limited.
- Propagation peer maintenance selection now claims the chosen peer before
  invoking sync by recording the sync attempt and next backoff window, while
  allowing the internal maintenance-triggered sync to consume that claim, so
  concurrent scheduler passes cannot double-select the same peer.
- Manual `/pn/peer/sync` control requests now force an immediate peer sync
  through ordinary backoff windows, while scheduled maintenance and remote
  syncs still respect retry postponement, matching the operator-triggered
  retry path.
- Remote fetch/download/sync imports now validate the full returned propagation
  payload batch before mutating the local store or in-memory payload cache, so a
  mixed valid/invalid remote response fails without leaving partial relay state.
- Remote fetch/download/sync imports now also reject payloads for ignored
  destinations during batch validation, so remote relay responses cannot bypass
  local replication policy or queue ignored work to peers.
- Malformed remote fetch and download imports now mirror existing
  payload-backed live queue marks into active peer record snapshots before
  failing, preserving restart/export retry state for already queued relay work.
- Malformed remote fetch and download imports from an already active source
  peer now also update that peer's failure backoff and publish the failed
  peer-sync event, so invalid post-transfer payloads share retry scheduling and
  observability with transport-level remote transfer failures.
- Remote fetch and download bridge failures now mirror existing payload-backed
  live queue marks into active peer record snapshots before returning the
  failure, preserving restart/export retry state for already queued relay work.
- Remote fetch and download bridge failures from an already active source peer
  now also update that peer's failure backoff and publish the failed peer-sync
  event, so retry scheduling and observability match the preserved queue
  snapshot.
- Remote fetch and download access-denied bridge errors now preserve the
  propagation `no_access` lifecycle state instead of collapsing the denial into
  generic failure, while retaining the bridge error text for operators.
- Access-denied remote transfer cleanup now reports the stored peer identifier
  in peer-unpeer events even when callers address the remote with different hex
  casing, keeping operator-visible teardown events aligned with the peer record
  that was actually removed.
- Remote fetch and download bridge-unavailable errors now mirror existing
  payload-backed live queue marks into active peer record snapshots before
  returning and mark the propagation sync lifecycle failed, so already queued
  relay work stays restart/export visible without leaving stale lifecycle state
  when no bridge is configured.
- Remote fetch and download bridge envelopes that return successfully while
  reporting `postponed` or `synced: false` now preserve the failed transfer
  lifecycle, source-peer backoff, peer event, and queue snapshot instead of
  importing an empty result and marking propagation complete.
- Successful remote fetch and download now also mirror existing payload-backed
  live queue marks into active peer record snapshots after applying imports, so
  restart/export state preserves queued retry work even when the remote
  transfer succeeds without consuming those local queued offers.
- Successful remote fetch and download now clear stale retry backoff on the
  active source peer when newly accepted payloads prove the source recovered,
  so later maintenance does not keep postponing a healthy replication peer.
- Successful remote fetch and download now also refresh the active source
  peer's sync-attempt timestamp while clearing stale backoff, so restart and
  status views reflect the successful recovery attempt instead of an obsolete
  failed transfer time.
- Remote peer-sync backoff postponements now mirror existing payload-backed live
  queue marks into active peer record snapshots before returning, so
  restart/export state preserves queued retry work even when sync is deferred.
- Remote peer-sync bridge-unavailable errors now mirror existing payload-backed
  live marks and restored peer-record queue IDs into active peer record
  snapshots for already known peers before returning, including
  case-insensitive requests, while still avoiding peer creation when the bridge
  is absent.
- Remote peer-sync bridge-unavailable errors for already known peers now also
  advance that peer's retry backoff, publish the failed peer-sync event, and
  mark the propagation sync lifecycle failed, keeping queue retry state
  observable without creating new peers.
- Peer sync RPC rows and events now preserve the Python-compatible peer `state`
  namespace while exposing backoff and policy postponement through separate
  scheduling fields; failed attempts continue to use the established error state.
- Successful remote peer-sync now also mirrors existing payload-backed live
  queue marks into active peer record snapshots after applying imports, so
  restart/export state preserves queued retry work even when the remote sync
  itself succeeds without transferring those local queued offers.
- Successful remote peer-sync imports now also refresh payload-backed queue
  snapshots for all active peers affected by imported payloads, so relay peers
  preserve complete restart/export-visible unhandled queues instead of only the
  newly imported IDs.
- Remote peer-sync now uses the stored peer ID case for the bridge call, import
  source accounting, state updates, and response envelope when callers use a
  case-variant peer request.
- Remote peer-sync bridge results that explicitly report `synced: false` or
  `postponed: true` now preserve the remote postponement in the peer-sync
  result/event and keep retry scheduling intact instead of clearing the peer's
  backoff as if the transfer completed.
- Failed remote unpeer attempts now mirror existing payload-backed live queue
  marks and restored peer-record queue IDs into active peer record snapshots
  before returning bridge-unavailable or bridge-execution errors, including
  case-insensitive peer requests, so restart/export state preserves queued retry
  work when peering teardown fails; these failed attempts also mark the
  propagation lifecycle failed instead of leaving stale idle/completed state.
- Failed remote unpeer bridge-unavailable errors for active peers now also
  publish the failed peer-sync event after queue snapshot refresh, keeping
  observer-visible peering failure state aligned with remote sync/fetch/download
  bridge-unavailable failures.
- Failed remote unpeer bridge-execution errors for active peers now also
  advance the peer's retry backoff window before refreshing queue snapshots, so
  failed peering teardown does not leave retryable queue work in an immediate
  retry loop.
- Failed remote unpeer bridge-execution errors for active peers now also
  publish the failed peer-sync event after queue snapshot refresh, keeping
  observer-visible peering failure state aligned with remote sync/fetch/download
  failures.
- Access-denied remote unpeer failures now follow the same local peering break
  path as access-denied remote sync/fetch/download, clearing local peer and
  propagation queue state instead of leaving denied teardown work retryable.
- Successful remote unpeer now also uses the stored peer ID case for the bridge
  call and nested bridge result when callers use a case-variant peer request,
  keeping remote teardown identity aligned with local queue cleanup.
- Successful remote unpeer now clears stale propagation lifecycle failures and
  error text left by earlier teardown attempts, so status reflects completed
  peer removal instead of a prior failed control operation.
- Shared transport dispatch now prunes interface records whose TX queues have
  closed, including virtual children that share the same queue, so failed
  interface paths cannot leave stale outbound routing state behind.
- Active outbound normal and propagation stamp generation now reports stored
  generation progress through `get_outbound_progress`, while terminal failed or
  cancelled stamp states continue to suppress stale progress values.
- Deferred normal and requested propagated sends now run expensive stamp work
  in the outbound background worker before delivery handoff. The worker exposes
  queued and in-flight stamp ownership through `delivery_pipeline`, serializes
  normal and propagation stamp generation, records retry/cancellation metadata,
  and prepares propagated resource payloads before link/resource delivery
  semaphores are acquired.
- Inbound reticulumd `/pn/peer/sync` and `/pn/peer/unpeer` control commands now
  resolve stored peer IDs case-insensitively before dispatching to daemon RPCs,
  so binary peer-control requests do not report not-found for restored or
  configured peers whose status rows preserve a different hex presentation;
  `/pn/peer/sync` also checks hidden unpeered peer records so operator-triggered
  rejoin paths can reach the daemon reactivation state machine.
- Payload-backed peer queue snapshot mirroring resolves stored peer IDs
  case-insensitively before reading live queue marks, so restart/export state
  preserves queued work when callers use Python-style peer case variants.
- Incremental peer queue snapshot updates also resolve stored peer IDs before
  checking completed live marks, preventing transfer-limited or handled work
  from being serialized as retryable unhandled queue state through case
  variants.
- Incremental peer queue snapshot helpers now canonicalize transient IDs before
  serializing handled or unhandled queue state, preventing padded or upper-case
  caller IDs from leaking into restart/export snapshots.
- Transfer-limited peer marks now remain terminal when later generic handled
  reports arrive, so transfer-limit retry decisions do not get reclassified as
  offered/handled work in peer queue accounting.
- Transfer-limited peer marks also remain terminal when later transferred
  reports arrive, so completed transfer-limit decisions cannot be reclassified
  as outgoing/offered work by a subsequent queue update.
- Transfer-limited peer marks also remain terminal when later received reports
  arrive, so completed transfer-limit decisions cannot be reclassified as
  incoming work by a subsequent propagation import.
- Terminal peer marks now clear case-variant unhandled rows for the same
  transient ID, so handled, transferred, received, and transfer-limited work
  cannot remain retryable under an alternate caller-case peer key.
- Peer sync unhandled transfer selection and retry cleanup now read and remove
  caller-case peer variants as one effective peer, so queued transfer work
  cannot be missed or left retryable under alternate peer casing.
- Prospective peer queue selection now also reads case-variant completed marks
  before returning unhandled work, so helper-level queue selection cannot reopen
  received, transferred, handled, or transfer-limited payloads under alternate
  peer casing.
- Static-only propagation peer replacement now routes removed static peers
  through the same local unpeer cleanup as explicit unpeer, so handled,
  received, transfer-limited, and unhandled queue marks are cleared and
  accounted consistently.
- Completed peer mark helpers now write and read received/transferred live
  marks under the stored peer ID case when a peer record already exists, keeping
  live queue state and serialized restart/export snapshots on the same peer key.
- Restored Python peer records now update their serialized queue ID snapshot
  when peer sync handles, transfers, or transfer-limits queued offers, reducing
  restart/export drift after live offer-response processing.
- Peer sync offer acceptance now validates all transfer payload hex before
  marking any offered payload transferred, handled, or transfer-limited, so a
  malformed response batch cannot partially mutate live marks or serialized
  restart/export queue snapshots.
- Restored Python peer records now parse fractional `propagation_sync_limit`
  values through Python's integer-kilobyte restore path before peer-sync queue
  selection, preventing restored fractional sync limits from transferring work
  that Python would leave queued.
- Restored Python peer records now coerce numeric stamp, stamp-flexibility, and
  peering costs through Python's integer restore path before peering checks, so
  float-valued snapshots can still transfer queued stamped offers.
- Restored Python peer records now also coerce numeric `sync_strategy` through
  Python's integer restore path, so float-valued persistent-peer snapshots keep
  draining queued offers across sync-limit batches.
- Restored Python peer records now accept Python `time.time()` float
  timestamps for heard/sync/backoff fields, so restart-loaded peers can still
  reach queued transfer instead of failing restore before sync.
- Restored Python peer records now coerce numeric message and byte counters
  before peer-sync accounting, so restart-loaded peers keep cumulative
  offered/outgoing/incoming totals while transferring newly queued work.
- Restored Python peer records now preserve serialized LXMPeer metadata through
  Rust peer record round trips, so restart/export snapshots do not drop
  peer-specific metadata before later queue work resumes.
- Live propagation announces now retain Python PN metadata on active peer
  records, so announce-derived peer metadata survives into later peering and
  queue restart/export snapshots.
- Python-style `lxmd` `[lxmf] announce_interval` now drives peer/delivery
  announce cadence separately from `[propagation] announce_interval`, which
  remains the propagation-node announce cadence.
- Outbound propagated delivery now resolves selected propagation-node
  `propagation_stamp_cost` case-insensitively, so Python-style hash casing does
  not fall back to the default propagation stamp cost.
- Direct `reticulumd` `[propagation_node]` config now activates the
  Python-shaped propagation/control destinations, advertises configured stamp
  costs, exposes outbound propagation cost lookup, and stores self-selected
  propagated payloads locally instead of linking to its own node.
- Normal and propagation stamp retry metadata now clears stale stamp error
  fields when later work re-enters generating/ready state, so status no longer
  reports a prior failed attempt after a successful retry.
- Peer sync queue creation also records newly queued existing propagation IDs in
  the peer record snapshot, so postponed syncs can restart/export with the same
  unhandled queue visible in live status.
- Local peer offer-error responses now publish failed peer-sync state fields at
  both the top-level peer event and nested propagation result while preserving
  the retryable peer queue, improving parity with the peer sync state machine.
- Ordinary full-offer peer sync now validates the propagation payload batch
  before marking any queued ID transferred, so a later malformed queued payload
  cannot partially drain peer retry state.
- Inbound and remotely imported propagation payloads update active peer record
  snapshots when they queue new unhandled IDs or mark source peers handled,
  keeping restart/export state aligned with live queue fan-out and source
  accounting.
- Duplicate inbound peer propagation payloads now still apply source-aware
  fan-out to active relay peers while keeping the source peer handled, so a
  known local payload does not skip relay queue creation.
- The typed ZeroMQ SDK backend now supports multiple real, persistent
  Reticulum service identities on one transport through
  `sdk_identity_create_v2` and session-scoped list/activate/import/export/send,
  targeted announces, source-specific signing, inbound event filtering, and
  reconnect retention. This matches the Python delivery-identity registration
  model needed by REM/RCH without conflating a service key or display name
  with the daemon transport/propagation identity.
- The typed ZeroMQ SDK backend now also exposes
  `ZmqPipelineBackendClient::identity_announce` for capability-rich announces,
  preserving local identity, display name, callsign, REM capability flags, RCH
  announce-slot metadata, and extensions over `sdk_identity_announce_now_v2`
  while keeping the no-argument `identity_announce_now` compatibility path.
- The typed ZeroMQ SDK backend now exposes
  `ZmqPipelineBackendClient::workflow_peer_ready`, preserving display names,
  callsigns, trust, bootstrap intent, and REM/RCH capability metadata while
  optionally announcing before use, so saved-peer setup has a direct typed path.
- The typed ZeroMQ SDK backend now exposes
  `ZmqPipelineBackendClient::peer_directory`, merging saved contacts and
  announce-derived presence over `sdk_identity_contact_list_v2` and
  `sdk_identity_presence_list_v2` while preserving display names, callsigns,
  REM capability flags, RCH announce-slot metadata, online state, and
  first/last-seen timestamps.
- The typed ZeroMQ SDK backend now exposes
  `ZmqPipelineBackendClient::peer_directory_since` and a
  `min_last_seen_ts_ms` presence-list filter, so REM/RCH can suppress stale
  announce rows over the SDK path while keeping saved contacts visible offline.
- The typed ZeroMQ SDK backend now exposes saved-peer lifecycle calls through
  `ZmqPipelineBackendClient::peer_connect`, `peer_disconnect`, and
  `peer_reconnect`, routing `sdk_peer_*_v2` methods while preserving identity,
  display name, correlation ID, callsign, REM capability flags, RCH
  announce-slot metadata, and per-call extensions.
- The typed ZeroMQ SDK backend now also covers the operation registry and SDK
  envelope execution path, including `app.message.history.list` and
  `app.delivery.destination_hash` plus delivery ticket generation over
  `app.delivery.ticket.generate`, so REM/RCH direct-chat history, runtime
  delivery-destination lookups, and ticket convenience flows can stay on
  `ZmqPipelineBackendClient` instead of constructing raw RPC/HTTP envelopes.
- Paper-message encode/decode now ride the registered SDK envelope path as
  `app.paper.encode` and `app.paper.decode` in both the daemon and SDK app
  registries, with typed envelope payloads for `sdk_paper_encode_v2` and
  `sdk_paper_decode_v2` aliases. The typed SDK also exposes
  `paper_decode_with_metadata` while preserving legacy `paper_decode` Ack
  compatibility, so paper-ingest duplicate/transient/destination/size metadata
  is available over both RPC and ZeroMQ backend paths. Duplicate bridge-backed
  paper decodes now also emit bounded `inbound_dropped` events without a second
  inbound store/event. The `lxmf`/`lxmf-cli` command surface now exposes the
  same SDK-backed paper flow through `paper-encode --message-id` and
  `paper-decode --uri`.
- The typed ZeroMQ SDK backend now exposes durable direct-chat history through
  `ZmqPipelineBackendClient::list_message_history`, preserving message bodies
  with links, receipt status, basic LXMF fields, one-to-one
  `peer_id`/`conversation_id` filters, `include_receipts`, and restart
  pagination cursors through the daemon `app.message.history.list` SDK envelope
  path.
- The typed ZeroMQ SDK backend now exposes durable direct-chat conversation
  summaries through `ZmqPipelineBackendClient::list_conversations`, preserving
  peer display names, unread counts, last-message previews with links, receipt
  inclusion intent, and restart pagination cursors through
  `app.message.conversation.list` on the SDK envelope path.
- The native SDK app domain now exposes `app.messages().history(...)`,
  `app.messages().conversations(...)`, and `app.messages().cancel(...)` on
  the existing `Client` surface, so direct-chat clients can bind message-list,
  conversation-list, and cancellation UI without decoding raw SDK envelopes or
  dropping to the root client handle.
- `ZmqPipelineBackendClient::list_message_history` now accepts both canonical
  `id`/`content` records and legacy direct-chat `message_id`/`body` records
  from `app.message.history.list`, keeping restart-recovered conversation
  history readable without raw envelope decoding.
- The typed ZeroMQ SDK backend now exposes the local runtime delivery
  destination through `ZmqPipelineBackendClient::local_delivery_destination_hash`,
  while still routing `app.delivery.destination_hash` through SDK envelope
  execution, so REM/RCH direct-chat source selection does not need raw RPC/HTTP
  status calls.
- The typed ZeroMQ SDK backend now tracks negotiated receipt terminality for
  delivery status, so direct-chat status reports match the SDK contract:
  `sent` is terminal until `sdk.capability.receipt_terminality` is negotiated,
  after which `delivered` is the terminal receipt state.
- The typed ZeroMQ SDK backend now exposes burst sends through
  `ZmqPipelineBackendClient::send_batch` and still routes
  `app.delivery.send_batch` envelope calls to `sdk_send_batch_v2`, preserving
  ordered per-message acceptance and rejection results without raw RPC
  envelopes.
- `BatchSendItem` now carries per-message idempotency keys, TTL, correlation
  IDs, and SDK extensions into each batch message's `_sdk` field metadata, so
  burst direct-chat retries can remain stable across client restarts.
- The typed ZeroMQ SDK backend and operation registry now expose direct-chat
  cancellation through both `ZmqPipelineBackendClient::cancel` and
  `app.delivery.cancel` envelope execution, preserving daemon cancellation
  outcomes without raw RPC envelopes.
- The native SDK app facade now routes `app.delivery.cancel` locally through
  `Client::cancel_delivery`, preserving typed cancellation results for app
  callers instead of falling through to generic remote-command dispatch.
- `app.delivery.cancel` now cancels queued/pre-handoff outbound work before
  bridge delivery, persists `receipt_status = cancelled`, records delivery
  trace and event state, exposes cancel metadata through raw and envelope SDK
  lifecycle traces, and keeps ZeroMQ direct/envelope cancellation result
  variants typed without claiming hardware or external-client coverage.
- Ordinary delivery stamp policy control now rides the same SDK envelope path
  as `app.delivery.stamp_policy.get` and `app.delivery.stamp_policy.set`, with
  `ZmqPipelineBackendClient::delivery_stamp_policy_get` and
  `delivery_stamp_policy_set` projecting typed `DeliveryStampPolicyState`
  fields while preserving the daemon's raw `stamp_policy` payload.
- The typed ZeroMQ SDK backend now starts the final propagation-first branch
  with `ZmqPipelineBackendClient::propagation_peer_sync`, routing
  `app.propagation.peer_sync` over `sdk_envelope_execute_v2` to the daemon's
  existing `peer_sync` lifecycle while preserving offer, transfer, postponed,
  retry, and persistent queue metadata in the typed response.
- `PropagationPeerSyncResult` now projects daemon `messages` and `propagation`
  queue fields into a typed `queue` snapshot, including offered/outgoing/
  incoming/unhandled counters and handled, unhandled, transferred, skipped,
  rejected, and transfer-limited transient IDs while retaining raw payloads.
- The typed peer-sync `queue` snapshot now also exposes transferred, skipped,
  rejected, and transfer-limited counters plus their byte totals, so retry and
  sync-limit callers do not need raw propagation JSON for queue accounting.
- `PropagationPeerSyncResult` now falls back to propagation-level transfer and
  sync limits and exposes target stamp cost plus stamp cost flexibility, so
  propagation policy metadata stays typed for REM/RCH clients.
- `PropagationPeerSyncResult` now also exposes typed failure kind,
  timeout/access-denied classification, and existing retry scheduling fields
  for postponed peer-sync attempts, so offer and queue retry callers do not
  need raw propagation JSON for common failure branching.
- `PropagationPeerSyncResult` now also falls back to propagation-level
  `postponed` and `postpone_reason` fields when the peer-sync envelope omits
  top-level values, keeping remote nested peer-sync retry state fully typed.
- The same ZeroMQ SDK propagation branch now exposes remote router status,
  fetch, download, sync, and unpeer lifecycle calls through typed
  `ZmqPipelineBackendClient` methods and registered `app.propagation.*`
  envelopes, preserving daemon propagation, peer-sync, transfer, denial,
  timeout, and queue-cleanup payloads without requiring REM/RCH to use raw RPC.
- `PropagationRemoteSyncResult` now also projects nested remote-sync
  `peer_sync` payloads into typed `peer_sync_state`, so remote propagation sync
  callers can inspect sync status and queue transient IDs without parsing raw
  JSON while still retaining the original daemon payload.
- `PropagationRemoteSyncResult` now also projects top-level remote-sync
  propagation cleanup IDs into a typed `queue` snapshot, so transferred,
  skipped, rejected, and transfer-limited sync work is visible without raw
  propagation JSON even when nested peer-sync state is incomplete.
- `PropagationRemoteSyncResult` now also projects its propagation lifecycle and
  result payloads into typed `transfer_state`, so sync timeout, denial, retry,
  next-attempt, and last-error handling are visible without raw propagation
  JSON parsing.
- `PropagationRemoteStatusResult` now projects remote router status into typed
  `status_state`, covering lifecycle state, selected node/peer, queue depth,
  failure kind, timeout/access-denied classification, retry count, next sync
  attempt, and last error while preserving raw status JSON.
- `PropagationRemoteStatusResult` now also projects Python-shaped
  `/pn/get/stats` payloads into typed `stats`, covering message-store counts,
  bytes, limits, client served/received counters, unpeered counters, peer
  counts, and router cost/limit fields while preserving raw status JSON.
- `PropagationRemoteTransferResult` now projects remote fetch/download result
  and propagation lifecycle payloads into typed `transfer_state`, covering
  sync/postpone status, imported IDs/counts, transferred bytes, progress, and
  last error while retaining the original daemon JSON.
- `PropagationRemoteTransferResult` now also projects remote fetch/download
  propagation queue IDs into typed `queue`, so transferred, skipped, rejected,
  and transfer-limited transient IDs are visible without raw propagation JSON.
- `PropagationRemoteTransferState` now also exposes failure kind, timeout and
  access-denied booleans, retry count, and next sync attempt for remote
  fetch/download results, so clients can branch on denial and timeout recovery
  without parsing raw propagation JSON.
- `PropagationRemoteTransferState` now also exposes `last_sync_started` and
  `last_sync_completed` for remote fetch/download/sync/unpeer lifecycle
  results, keeping transfer freshness visible without raw propagation JSON.
- `PropagationRemoteTransferState` now also exposes selected router context
  through `selected_node` and `selected_peer` for remote fetch/download/sync/
  unpeer lifecycle results, keeping peer/router selection visible without raw
  propagation JSON.
- Remote fetch/download/sync/unpeer SDK envelopes now convert denied, timed
  out, and retryable bridge failures into typed result payloads with daemon
  propagation recovery state, so REM/RCH clients can stay on
  `ZmqPipelineBackendClient` for failure recovery instead of dropping to raw
  RPC errors.
- `PropagationRemoteUnpeerResult` now projects remote unpeer `messages` and
  propagation cleanup payloads into a typed `queue` snapshot, so denial and
  teardown cleanup callers can inspect handled, unhandled, transferred,
  skipped, rejected, and transfer-limited IDs without parsing raw JSON.
- `PropagationRemoteUnpeerResult` now also projects teardown lifecycle payloads
  into typed `transfer_state`, so denied or failed unpeer attempts expose
  failure kind, access-denied/timeout classification, retry scheduling, and
  last error without parsing raw propagation JSON.
- The same branch now exposes propagation sync completion/failure
  acknowledgement as
  `ZmqPipelineBackendClient::propagation_acknowledge_sync_completion` and
  `app.propagation.acknowledge_sync_completion`, preserving daemon recovery
  state for retry, timeout, and restart flows on the typed ZeroMQ SDK path.
- `PropagationStatusResult` and `PropagationAcknowledgeSyncResult` now project
  their propagation payloads into typed `recovery_state`, so status, enable,
  and acknowledgement callers can inspect sync state, retry counts, queue
  depth, and last error without parsing raw JSON.
- `PropagationRecoveryStateResult` now also exposes failure kind, timeout and
  access-denied booleans, and next sync attempt, so local recovery and sync
  acknowledgement callers can branch on denial/timeout handling without raw
  propagation JSON.
- `PropagationRecoveryStateResult` now also exposes the propagation lifecycle
  `timestamp`, so restart/recovery status callers can inspect daemon recovery
  freshness without parsing raw propagation JSON.
- `PropagationRecoveryStateResult` now also exposes local propagation config
  fields for `auth_required`, `control_allowed`, `static_peers`, and
  `sync_limit`, so status and enable/config callers can verify recovery policy
  without raw propagation JSON.
- `PropagationRecoveryStateResult` now also exposes propagation storage and
  transfer-limit config for `store_root`, `target_cost`,
  `message_storage_limit_mb`, and `propagation_limit`, keeping durable queue
  policy visible on the typed ZeroMQ SDK path.
- `PropagationRecoveryStateResult` now also exposes the remaining propagation
  enable/status config for `stamp_cost_flexibility`, `delivery_limit`,
  `autopeer`, `autopeer_maxdepth`, `max_peers`, `from_static_only`,
  `retain_synced_on_node`, `peering_cost`, and `remote_peering_cost_max`, so
  router/peering policy is visible through the typed ZeroMQ SDK path.
- The typed propagation branch also exposes outbound propagation router
  selection and listing as `ZmqPipelineBackendClient::propagation_node_get`,
  `propagation_node_set`, and `propagation_node_list`, backed by
  `app.propagation.node.*` envelopes that preserve selected-node and node-list
  metadata without raw RPC.
- `PropagationNodeListResult` now projects listed router candidates into typed
  `PropagationNodeRecord` entries, exposing peer, display name, last-seen time,
  selected flag, and capability strings while retaining the raw node JSON.
- `PropagationNodeSelectionResult` now projects node get/set `meta` into typed
  `selection_state`, exposing selected peer, selection flag, queue depth,
  failure kind, timeout/access-denied classification, retry scheduling, and
  last error without parsing raw router metadata.
- The typed propagation branch now also exposes local propagation status,
  enable/config, delivery policy get/set, and peer maintenance through
  `ZmqPipelineBackendClient` methods and `app.propagation.*` envelopes, keeping
  daemon policy, stale-peer cleanup, and retry/maintenance state visible without
  raw RPC.
- `PropagationPeerMaintenanceResult` now projects maintenance-triggered
  `peer_sync` payloads into typed `peer_sync_state`, so stale-peer cleanup and
  automatic retry/rotation callers can inspect sync timing and queue transient
  IDs without parsing raw JSON.
- The typed propagation branch now exposes local propagation payload ingest and
  fetch as `ZmqPipelineBackendClient::propagation_ingest` and
  `propagation_fetch`, backed by `app.propagation.ingest` and
  `app.propagation.fetch` envelopes that preserve transient IDs, payload bytes,
  duplicate accounting, and durable store recovery through the ZeroMQ SDK path.
- `PropagationIngestResult` and `PropagationFetchResult` now also preserve
  daemon propagation lifecycle payloads and project them into typed
  `recovery_state`, so disconnected-client ingest/fetch callers can inspect
  selected node, sync state, queue depth, and local ingest/serve counters
  without parsing raw propagation JSON.
- `PropagationDeliveryPolicyResult` now projects delivery policy payloads into
  typed `policy_state`, so propagation-first clients can inspect auth-required
  mode plus allowed, denied, ignored, and prioritised destination sets without
  parsing raw policy JSON.
- The router policy surface now also exposes Python-style incremental
  `set_authentication`, `requires_authentication`, `allow`, `disallow`,
  `ignore_destination`, `unignore_destination`, `prioritise`, and
  `unprioritise` operations through legacy RPC aliases and SDK envelope
  operation IDs. The daemon aliases validate 16-byte destination hashes and
  suppress duplicate list entries, while typed ZeroMQ helpers preserve
  unrelated auth, allow, deny, ignore, and priority fields through get/set.
- Direct inbound LXMF packet/resource drops and propagated local delivery-policy
  drops now emit bounded raw `inbound_dropped` RPC/SDK event-stream entries
  without storing messages or updating peer activity; identifier fields use the
  existing event redaction path by default and events distinguish `packet`,
  `resource`, and `propagation` delivery kinds. Direct delivery resources now
  also enforce the advertised delivery transfer limit before decode or storage,
  emitting the same bounded drop signal for oversized completed resources. The
  propagated local-delivery coverage includes local envelope ingest plus
  decryptable remote fetched and remote downloaded propagation payloads that
  reach local decode, stamp, or delivery-policy handling, plus local-addressed
  pre-decode rejects for short or undecryptable local envelopes and strict remote
  fetch/download local-import rejects for short payloads, destination mismatches,
  and decrypt failures, so those router-coupled drops remain observer-visible
  instead of only counted as rejected imports.
- The native SDK app event mapper now projects inbound delivery, receipt, and
  drop payloads into typed helpers on the existing event path, preserving
  message IDs, source/destination hashes, raw LXMF bytes, delivery kind,
  receipt status, signature/stamp metadata, drop reason, remote propagation
  operation/transient/peer context, and lifecycle state without requiring
  REM/RCH clients to parse raw event JSON for normal message and status UI.
- Typed inbound message helpers now expose the richer handler metadata already
  carried in raw inbound events: signature validity, stamp validity,
  propagation stamp validity, LXMF method, and direct transport encryption
  fields.
- Receipt lifecycle events now preserve handler/bridge stage provenance in the
  pollable SDK payload and typed app lifecycle helper, so transport-origin
  delivery receipts can be distinguished from other receipt publishers without
  raw JSON parsing.
- RPC-layer propagation rejects for ignored destination hashes now emit bounded
  `inbound_dropped` events before returning `PermissionDenied` from
  `propagation_ingest` and remote fetch/download/sync imports. The events use
  `delivery_kind = "propagation"`, preserve transient/operation context, and
  rely on the default event redaction path for destination identifiers, keeping
  ignored payloads observer-visible without storing or queueing them.
- Locally delivered propagated LXMF payloads now store the same `_lxmf`
  signature metadata as direct packet/resource delivery paths and include it in
  the emitted raw inbound event. Local envelope ingest, remote fetch imports,
  and remote-control download imports pass through the shared transport-aware
  signature annotation path when available. Unknown source identities now record
  `signature_checked = false`, `signature_valid = false`, and
  `signature_status = "source_identity_unknown"` instead of omitting signature
  status from handler-facing state.
- Successful direct packet/resource LXMF deliveries now have focused
  SDK-pollable inbound callback evidence: `sdk_poll_events_v2` returns raw
  `lxmf_bytes_hex`, stored/event identity, content, and metadata consistency,
  direct Curve25519 transport metadata, and verified signature metadata for
  both packet and resource delivery paths.
- Python-style targeted local delivery announces now have focused daemon/RPC
  coverage through `announce_delivery`: matching the local LXMF delivery hash
  sends only the delivery announce bridge path, while non-local hashes are
  rejected without firing broader propagation/control announce behavior.
- Focused local propagated-delivery tests now also feed a real source announce
  through a transport interface channel before delivery, covering known-source
  propagated messages surface `signature_checked = true` with
  `signature_status = "verified"` for valid LXMF signatures and
  `signature_status = "signature_invalid"` for corrupted signatures in both
  stored records and raw inbound events.
- Local propagated-delivery accepts now also write the persistent processed
  transient marker used by daemon propagation ingest, so replaying the same
  stamped transient through `propagation_ingest` reports
  `ingested_count = 0` / `duplicate_count = 1` and does not increment local
  receive counters a second time. Replayed local propagated delivery of an
  already processed transient, or of an already stored message carried by a
  fresh transient, now also emits one bounded `inbound_dropped` duplicate event
  with redacted destination identifiers and the transient ID, without storing a
  second message or incrementing receive counters.
- Propagation announce handling now gates peer/queue side effects on active
  local propagation handling, matching Python `Handlers.py` behavior while
  still recording the announce for observability.
- Remote propagation imports from fetch, download, and sync now keep duplicate
  payloads observer-visible by emitting bounded `inbound_dropped` duplicate
  events with operation, transient, byte-length, and optional peer context while
  preserving peer queue side effects for still-stored duplicates, avoiding
  unservable peer marks for processed-only duplicates, and avoiding duplicate
  storage/upsert work.
- The typed propagation branch now also exposes
  `ZmqPipelineBackendClient::propagation_recovery_state`, projecting
  `app.propagation.status` into structured sync state, selected-node,
  last-error, retry count, queue depth, timestamp, and local ingest/serve
  counters while retaining the raw propagation payload for queue recovery
  diagnostics.
- Locally delivered inbound peer propagation payloads now also store the
  accepted transient and apply source-aware relay fan-out without double
  counting source peer activity, so local delivery does not bypass relay queue
  creation.
- Inbound peer propagation ingest now also marks inactive identified sources
  as received before later activation, so newly peered sources are not offered
  payloads they supplied while still unpeered.
- Inbound propagation message-get serving now admits or refreshes the remote
  propagation peer before marking served payloads transferred, so peer transfer
  accounting is preserved even when the peer fetches before a prior offer row.
- Inbound propagation message-get serving now previews fetchable payloads and
  passes peer admission before mutating served counters, so peers rejected by
  static-only or capacity policy do not look like successful transfers.
- Inbound propagation message-get listing now also applies peer admission before
  returning non-empty payload ID lists, so rejected peers cannot enumerate
  queued transfers they are not allowed to fetch.
- Inbound propagation message-get `haves` handling now applies peer admission
  before purging matching local payloads, so rejected peers cannot delete queued
  transfers they are not allowed to acknowledge.
- Inbound propagation message-get `haves` handling now also records matched
  haves as received/completed work for the requesting propagation peer after
  purge, so reintroduced payloads are not queued back to peers that already
  declared them.
- Retained propagation payload listings now filter IDs already completed by
  the requesting peer, so `retain_synced_on_node` keeps payloads available for
  other peers without re-offering them to the peer that declared the haves.
- Link-based remote propagation downloads now wait for the final haves
  acknowledgement response after imported or duplicate payloads are reported,
  and also after all-known listings are acknowledged with purge-only haves, so
  node-side rejection or timeout is surfaced instead of reporting a completed
  download before remote cleanup is confirmed.
- Inbound propagation message-get purge-only requests now return the
  Python-style boolean success response after haves are applied, and payload
  purge cleanup preserves completed peer accounting for other peers while
  removing stale unhandled marks, so reintroduced payloads are not offered back
  to peers that already completed them.
- Inbound propagation message-get requests now mark wanted payloads skipped by
  the peer's transfer budget as transfer-limited completed work after peer
  admission, so oversized fetch attempts do not remain retryable queue entries.
- Inbound propagation message-get transfer-budget handling now leaves payloads
  skipped only by the cumulative response budget retryable for a later request,
  while still completing individually oversized wanted payloads as
  transfer-limited.
- Inbound propagation offer requests with too-short list payloads now follow
  Python's caught-exception nil response path without validating the link or
  admitting a propagation peer.
- Valid inbound propagation offers now answer Python's `False`, `True`, or
  wanted-ID list responses after peering-key validation without admitting the
  remote peer or queuing local propagation payloads before a real transfer or
  message-get admission point.
- Inbound propagation offers now validate every offered transient ID before
  applying any source-accounting marks, so malformed mixed offers cannot leave
  partial received/completed queue state behind.
- Inbound propagation message-get requests now validate every wanted and have
  transient ID instead of silently filtering malformed entries, so malformed
  mixed `/get` lists cannot fetch or purge queue state behind the rejected
  request.
- Inbound propagation offers now deduplicate validated offered transient IDs
  before building wanted-ID responses or applying source-accounting marks, so a
  duplicate offer cannot request or account the same payload more than once.
- Remote fetch and download imports now mark inactive source peers as received
  before later activation, so a propagation node is not offered back payloads it
  previously supplied just because it was not yet an active peer record.
- Remote import batches now deduplicate accepted transient IDs before applying
  peer queue and incoming-message side effects, so duplicate payloads in one
  fetch/download/sync response do not inflate peer queue accounting.
- Remote import batch byte accounting now uses the same deduplicated accepted
  IDs, so duplicate payloads in one fetch/download/sync response do not inflate
  transferred byte totals or source peer receive byte counters.
- Local propagation ingest now persists processed transient IDs separately
  from retained payload entries, so reintroduced payloads after purge or peer
  acknowledgement can refresh relay state without inflating local received or
  ingested counters.
- Propagation peer maintenance now expires local processed-transient markers
  after the Python six-message-expiry cache window, so duplicate suppression
  does not retain stale transient IDs indefinitely.
- Propagation payload ingest now enforces the configured node message-storage
  byte limit against retained propagation entries, using age, size, and
  prioritised-destination weighting while clearing retryable peer queue marks.
- Link-based remote downloads now wait for the propagation node's `/get` haves
  acknowledgement and surface peer/control errors, so failed remote cleanup does
  not look like a completed replication drain.
- Link-based remote propagation control waits now surface authenticated
  link-close peer/control signals immediately, so denied or closed remote
  fetch/download/sync requests do not sit until the request timeout.
- Remote fetch/download acknowledgements now use canonical propagation
  transient IDs for stamped payloads, so `/get` haves purge the peer's offered
  queue entry instead of acknowledging the stamped payload bytes under a
  different hash.
- Repeated remote fetch/download/sync imports now increment source peer
  incoming counts and receive bytes only for payload IDs not already marked
  received from that source, while still replaying known payloads into relay
  queues when their live marks were cleared.
- Repeated peer-origin propagation ingests now also avoid double-counting
  source peer incoming counts and receive bytes for already received payload
  IDs, while still refreshing relay queue marks for peers that need the
  payload.
- Remote peer-sync imports now accept transferred payload arrays from full
  Python-style responses where top-level `messages` is a peer counter object
  and payloads live under `propagation.messages`/`propagation.payloads`, as
  well as legacy top-level `messages`/`payloads` envelopes.
- Propagation purge cleanup removes deleted local payload IDs from active peer
  record snapshots, so restart/export state does not retain purged queue entries
  after the live peer marks have been cleared.
- Duplicate or replayed propagation queue attempts respect already-completed
  peer marks when updating peer record snapshots, avoiding restart/export drift
  that would reopen handled IDs as unhandled.
- Duplicate or replayed propagation queue attempts also respect case-variant
  completed live marks, so a handled, transferred, received, or
  transfer-limited ID cannot be serialized as retryable unhandled work through
  the stored peer key.
- Peer sync queue replay records preexisting live unhandled marks into the peer
  record snapshot even when the store did not insert new rows, preserving
  restart/export visibility for already-queued work.
- Peer activation now also snapshots preexisting live completed marks, so
  transfers recorded before the peer record exists survive restart/export as
  handled IDs once the propagation peer is active.
- Peer activation also merges case-variant preexisting live completed marks
  into the activated peer key before queue replay, avoiding restart/export
  drift when transfer accounting arrives before the peer record case is known.
- Selected propagation node activation now reuses the existing peer record case
  before queue replay and canonicalizes merged live marks, so caller-case
  variants do not leave duplicate peer queue rows.
- Peer unpeer cleanup now clears case-variant propagation marks as one peer,
  so completed marks merged during activation cannot survive teardown and
  reappear as handled work when that peer is later reactivated.
- Peer unpeer cleanup now also removes the peer from configured static
  propagation membership, so an explicit unpeer cannot be undone by the next
  static-peer activation pass.
- Peer unpeer cleanup accounting now also reads case-variant live queue marks
  as one effective peer before clearing them, so the response and event report
  the same handled/unhandled IDs and byte totals that teardown actually
  removes.
- Reactivating a persisted `unpeered` record clears stale serialized peer queue
  snapshots before the peer becomes active again, avoiding restart/export
  resurrection of pre-unpeer propagation work.
- Reactivating a persisted `unpeered` record also clears stale live completed
  propagation marks before queue replay, so still-local payloads are offered
  again after the peer rejoins as manual or configured static.
- Persisted `unpeered` non-static records now re-run peer admission before
  reactivation, so static-only propagation policy cannot be bypassed by a
  stale teardown record.
- Static peer activation now clears stale serialized queue snapshots when it
  revives a persisted `unpeered` record, so configured static peering cannot
  resurrect pre-unpeer propagation work on restart/export.
- Reactivating a persisted `unpeered` record now also clears stale sync
  backoff postponement fields, so rejoined manual or configured static peers
  are not blocked by pre-unpeer retry scheduling.
- Peer sync reactivation now bypasses stale pre-unpeer backoff postponements
  before admission and queue replay, so manual rejoins are not returned as
  postponed `unpeered` peers.
- Peer sync reactivation now also applies the active peer type even when a
  restored `unpeered` record has a future `last_seen` timestamp, so clock-skewed
  restart state cannot leave a successfully rejoined peer marked unpeered.
- Peer sync stale queue cleanup now removes matching unhandled and completed
  IDs from active peer record snapshots when the underlying propagation payload
  no longer exists, keeping export/restart state aligned with live queue
  cleanup.
- Peer sync stale queue cleanup now also treats case-variant live peer marks as
  the same peer, so stale unhandled or completed rows cannot survive under a
  caller-case variant and later reappear in restart/export state.
- Restored peer records now accept Python MessagePack binary
  `destination_hash`, handled, and unhandled IDs, prune serialized queue IDs
  whose payloads are missing during replay, and canonicalize/deduplicate the
  surviving IDs, avoiding restart/export drift when Python snapshot entries
  outlive or duplicate local propagation storage.
- Early transfer-limit decisions made before peering-key handling now update
  active peer record snapshots as completed work, keeping serialized state in
  sync with the live transfer-limited mark.
- Early transfer-limit handling now also ignores explicit "wants none" offer
  responses before peering-key gates, so oversized queued entries complete as
  transfer-limited instead of remaining retryable behind a postponed sync.
- Persistent peer sync now preserves explicit offer-response boundaries by
  leaving sync-limit-skipped IDs queued for the next offer instead of
  auto-transferring messages outside the peer's current response.
- Peer maintenance now replays payload-backed restored unhandled queue
  snapshots before choosing a sync candidate, so restart-loaded peers can be
  selected and transferred without waiting for a manual `peer_sync`.
- Peer maintenance rotation now also replays restored queue snapshots before
  low-acceptance drop decisions, so restart-loaded peers with pending transfer
  work are not rotated out as if their queues were empty.
- Shared unpeer cleanup now replays restored queue snapshots before computing
  and clearing propagation marks, so policy culls and explicit teardown do not
  discard restart-loaded peer queue work without cleanup accounting.
- Inbound propagation offers now mark already-known offered payload IDs as
  received from the offering peer after peering-key validation, so later peer
  admission does not queue those source payloads back to the sender.
- Valid inbound propagation offers now start the peer offer throttle window
  after peering-key and transient-ID validation, so repeated replication offers
  from the same peer take the throttled response path even when the peer changes
  the offered transient-ID set.
- Propagation ingest and Python-served alias ingest now reject payloads for
  ignored destinations before storing or queueing them, emit bounded
  `inbound_dropped` events through the RPC/SDK event stream, and enforce local
  replication policy before relay state is created.
- Inbound propagation message-get `haves` completion now applies only to
  locally known payloads or existing peer queue marks, preventing unknown haves
  from suppressing future propagation work for the declaring peer.
- The live Python compatibility gate now includes a Python-origin propagation
  `/get` haves-only case against Rust `reticulumd`, covering `true`
  acknowledgement, Rust-side payload purge, and suppression of retryable
  unhandled peer queue state for the declaring propagation peer, plus a
  Python-origin `/offer` case covering partial wanted-ID responses,
  repeated-offer throttling, and source-peer completed marks before broad
  peer/router interop is claimed.
- Link-based propagation-control waits now treat matching resource transfer
  failure and cancellation as terminal remote fetch/download outcomes instead
  of waiting for the generic response timeout.
- The live Python compatibility gate now also splits out a Python-origin
  `/offer` peer-queue lifecycle case, covering post-sync handled IDs,
  absence of retryable missing IDs, and cleared sync backoff after the Rust
  peer row is created by transfer.
- Pinned Python path-discovery interop now includes a scoped/tagged
  `rnpath-rs --on-iface --tag-hex` refresh over a learned Python delivery
  route, extending scoped daemon path-request dispatch/result evidence beyond
  local Rust-only mesh smokes.

## Current Release State

Stable `v0.9.9` publication is historical. Stable `v0.10.0` publication is
complete on the software, hosted-PR, release-artifact, independent-interop,
provenance, OCI, and crates.io axes for immutable commit
`5436ee715f94f81e18abb0808cfca52fcd7cc9bc`. The performance comparison and all
four public performance assets are verified and tracked in
`docs/status/v0.10.0-release.md`.

Physical RNode/RNodeMulti, Weave, VR-N76, BLE, serial-radio, public I2P,
public Reticulum networks, and Sideband/MeshChatX/Columba or other
third-party-client claims remain separate deferred evidence tracks. They do not
downgrade the completed software inventory and must not be described as
validated without their own evidence.

## Active Execution Order

1. Keep the generated RNS 1.5.2 inventory and both parity matrices at zero
   partial or unmapped software entries as maintenance changes land.
2. Preserve exact-SHA Python-reference, independent-interoperability, and
   performance evidence for release-facing changes.
3. Treat physical hardware, public-network operation, and third-party clients
   as separate evidence programs for the v1.0 boundary.
4. Preserve the exact-SHA release ledger and evidence while maintenance work
   proceeds toward the next release boundary.

## Verification Baseline

- Primary CI: `.github/workflows/ci.yml`
- Pinned Python interop: `.github/workflows/verify.yml` (repository-native `cargo xtask hil` controller)
- Reference revisions are declared in `.github/workflows/verify.yml` and the
  repository-owned HIL case definitions rather than copied into status prose.
- Current run status belongs in GitHub Actions, not in this maintained document.
- A passing Python-reference workflow proves only the scenarios it executes.

## Status Rules

- `implementation=complete` requires active implementation; evidence is
  recorded independently and must name the validation level actually run.
- `hardware-unverified` must never be described as physical or production
  validation.
- A local model, RPC projection, or SDK state machine alone does not establish
  Python protocol/runtime parity.
- A passing interop workflow does not promote unrelated matrix rows.
- Update this file and the affected matrix in the same change.
- Keep implementation history in Git and historical plans, not in this file.
