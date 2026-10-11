# LXMF-rs v0.14.0

v0.14.0 brings the daemon, SDK, and 19 public packages onto one release train.
It includes the ZeroMQ reliability, resource-use, and durable-custody changes
merged since v0.13.0. Reticulum and LXMF reference pins and network wire formats
remain at the existing tested baseline.

## Changes

- ZeroMQ setup and exchanges have bounded deadlines and cancellation ownership.
  Stalled reply peers are isolated. SDK failure diagnostics retain correlation
  and execution certainty without accumulating successful exchanges.
- The opt-in durable broker commits inbound messages and journal events,
  consumer stored acknowledgements, outbound admission, prepared wire bytes,
  and operation receipts in SQLite. Replay-safe operations have finite recovery
  inside their original deadline. Custody acknowledgement does not mean peer
  delivery.
- Database and WAL admission, decoded frames, handshakes, connections, request
  lanes, dispatch, and terminal-receipt ownership have finite bounds. Full
  stores and pinned readers backpressure admission with observable errors.
- Propagation pending queues use their existing capacity, authoritative queue
  inventories stay in durable storage, and polls/events avoid copying complete
  payload and peer-history sets.
- PUSH/PULL replies reuse healthy connections tied to the SDK response-socket
  generation. The daemon retains at most 32 active plus idle reply connections,
  removes the former per-reply delay, discards failed sockets, and joins active
  deliveries on shutdown.
- Dedicated outbound workers use a bounded channel wait instead of sleeping
  after an empty poll. Queued work wakes the receiver directly; stop and durable
  dispatch checks retain their periodic timeout.
- `reticulum-rs-zeromq` publishes the existing patched transport under a distinct
  package name. The Rust import remains `zeromq`; registry consumers now receive
  the same transport corrections as the binary bundles. All 18 existing public
  packages, including the opt-in partial `meshchat-api`, also advance to 0.14.0.

## Upgrade and operations

Rust 1.88 or newer is required. The locked `time` family already requires
1.88, and the locked ICU family requires 1.86; this release corrects the stale
1.85 declaration and adds the supported minimum to the build matrix.

Rust struct literals for `ZmqRpcEnvelope` must include
`response_connection_id: None` when no response-socket generation is supplied.
Existing request/response constructors remain available. Unextended protocol-v1
frames retain their tuple encoding; extended requests use named MessagePack
fields accepted by older Rust decoders. Legacy reply connections remain
request-scoped.

Durable custody requires `reticulumd --zmq-durable-broker`, local persistent
storage, and explicit SDK capability negotiation. The bundled engine is SQLite
3.53.2 with WAL/FULL verification. The migration is forward-only: preserve
consistent backups before enabling it and follow the documented restore and
downgrade procedure. See the [v0.14.0 migration notes](migrations/v0.14.0-zmq.md)
and [durable-broker operations](goals/zmq-rch-durable-broker/OPERATIONS.md).

## Evidence and remaining work

Release preparation runs the normal quality, contract, packaging, and smoke
checks. GitHub publication carries platform archives and packages, SBOMs,
checksums, provenance, a multi-architecture OCI image, independent-interoperability
reports, and the maintained release-performance comparison. Their final status
is recorded in the public release after the workflows complete.

The paired source tests cover real SQLite and ZeroMQ sockets, restart/replay,
lost acknowledgement and admission-response recovery, capacity pressure,
receipt persistence, and cancellation. These do not establish a production
memory/swap plateau, hardware power-cut durability, or the cause of #657's
reported production timeouts. Those operational checks remain separate, as do
physical/client/public-network acceptance and the existing Reticulum feature
gaps. See the [roadmap](status/current-roadmap.md),
[broker evidence](goals/zmq-rch-durable-broker/PROGRESS.md), and
[#657 investigation](issue-657-investigation.md).

## Changes since v0.13.0

- [#653](https://github.com/FreeTAKTeam/LXMF-rs/pull/653): bounded ZeroMQ setup and isolated stalled peers.
- [#654](https://github.com/FreeTAKTeam/LXMF-rs/pull/654): daemon memory/disk work and SDK recovery.
- [#658](https://github.com/FreeTAKTeam/LXMF-rs/pull/658): bounded propagation queues and inventory allocation.
- [#660](https://github.com/FreeTAKTeam/LXMF-rs/pull/660): durable ZeroMQ custody and recovery.
- [#661](https://github.com/FreeTAKTeam/LXMF-rs/pull/661): bounded reusable reply connections.

[Full changelog](https://github.com/FreeTAKTeam/LXMF-rs/compare/v0.13.0...v0.14.0).
