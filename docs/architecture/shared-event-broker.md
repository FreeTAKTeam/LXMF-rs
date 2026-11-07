# Shared event broker extraction

## Boundary review before code movement

Canonical source: `crates/libs/r3akt-event-broker` in LXMF-rs. Dafrion must use
this package from an immutable Git revision, never a source copy. The explicit
directory layout takes precedence over the attachment's suggested package name.

The current `rns-rpc::broker` module mixes reusable journal/consumer contracts
with LXMF admission options, propagation custody and SDK negotiation. Extract
only reusable contracts and retain compatibility re-exports in `rns-rpc`.
Existing JSON field names and defaults are part of the public wire contract.

`storage/broker.rs` and `storage/broker/execution.rs` own authenticated issued
ranges, monotonic journal positions, replay and checkpoint/pruning logic. These
are transport independent. Their owner and scope arguments must be resolved by
the application: request fields and socket session IDs do not establish identity.
MACs bind journal incarnation, consumer, owner, scope and issued position range.
An acknowledgement advances durable custody only, never business completion.

The same modules also own LXMF message fingerprints, message rows, outbound
options, prepared signed payloads and network custody reservations. Those remain
in `rns-rpc`, alongside message dispatch and `DaemonStored` semantics. Generic
publishing must not call message admission or inherit LXMF raw-message limits.

`storage/broker/profile.rs` mixes generic journal schema/WAL configuration with
message snapshot migration, operation tables and pending-message delete triggers.
Split the reusable storage primitives while retaining application migrations.
Keep existing table/column names and journal secrets/incarnations when reopening
v0.14.0+ databases. Never introduce another LXMF database or writer.

`MessagesStore::broker_command` submits to the existing bounded writer. Its
2.5-second reply deadline permits uncertain commits; replay-safe calls must use
the same operation ID or receipt. The shared primitives must participate in an
application-owned SQLite transaction, including rollback when a message/event
write fails. The standalone broker may own its connection for other consumers.

The validated profile requires disk SQLite >=3.51.3, WAL, synchronous FULL,
bounded page count, logical accounting, physical spill/commit admission and
checkpoint maintenance. Ordinary admission preserves completion/control
headroom. Retained idempotency records count against admission; exhaustion must
fail explicitly. Existing completion reservations and recovery of scheduled
network work remain application responsibilities.

`rpc/daemon/sdk_broker.rs` authenticates service ownership and preserves SDK RPC
method behavior. `lxmf-sdk`'s ZeroMQ pipeline remains its transport adapter.
`rpc/event_sink.rs` contains webhook/MQTT transport concerns; those do not belong
in the journal. Tokio notifications may wake consumers but are never durable
state. The shared crate must have no LXMF, ZeroMQ, HTTP or Dafrion model dependency.

## Implementation and acceptance

Use concrete connection/transaction functions rather than a new storage trait
hierarchy. This follows rusqlite's transaction ownership and rollback semantics
and the repository's composition/encapsulation principles. The existing
implementation is the source of truth: move reusable logic and call it from both
the LXMF adapter and the standalone API; remove the superseded implementation.

Acceptance covers old database reopen, atomic rollback, immutable idempotency,
forged/cross-principal/stale receipts, independent checkpoints, restart/replay,
uncertain results, bounded admission and reserved completion. Run existing LXMF
broker/RPC/SDK regressions, independent-crate tests, architecture checks and the
Dafrion integration tests. RCH consumer inspection and Dafrion integration
findings will be recorded here before their integration is changed.

## Core contract details established before extraction

The generic SQLite schema keeps `broker_meta`, `broker_events`, and
`broker_consumers` with their current names and columns. Scope is deliberately
opaque (the historic column is `local_destination`). The LXMF adapter alone
installs the existing unique `(owner,destination)` index: generic applications
may have multiple independent consumers of the same topic. Position scans are
bounded independently of visible results, and invisible positions still advance
an issued range. An outstanding issued batch replays exactly before new limits
are considered. Checkpoints advance only for the exact authenticated outstanding
range; ACK retries remain valid after that range has been pruned.

Core functions borrow the application connection and never commit, including
schema initialization, append, resume, fetch, ACK and logical accounting. WAL
configuration occurs before migration transactions. The LXMF migration retains
one atomic transaction for generic schema, message-specific schema and historical
snapshot; its prior secret, incarnation, logical accounting and budget survive
reopen. The caller owns commit-error mapping and its existing bounded writer.
Generic standalone publishing and processing records have separate tables and
never reuse LXMF operation or completion meanings.

Default batch limits remain exactly 128 events and 12 MiB for LXMF. Generic
applications may explicitly select application-specific bounded limits through a shared
`Limits` value; append and fetch use the same selected bounds. Transport wakeups,
principal authentication, payload validation, routing/topic selection and business
processing are adapters, not implicit journal authority. WAL outcome types move
with maintenance while the daemon's metrics counters and environment cadence
remain application-owned.

### Rust implementation basis

The extraction uses connection-borrowing functions as the smallest change from
the existing writer, rather than introducing a storage trait or a parallel
journal implementation. rusqlite 0.40.2's transaction documentation establishes
that `Transaction` dereferences to `Connection`, rolls back on drop, and prevents
nested transactions through `Connection::transaction(&mut self)` (or runtime
checks for the existing shared writer's `unchecked_transaction`). See the
[rusqlite transaction API](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Transaction.html).
The standalone facade provides an owned connection for ordinary atomic calls and
a session over a caller-owned transaction for composition. The trade-off is an
explicit SQLite storage API; this avoids a speculative backend abstraction and
retains the production persistence/commit-hook semantics. Shared WAL maintenance
returns values; daemon metrics counters and environment selection stay adapters.

## Consumer integration boundaries

RCH's `crates/r3akt-rch-core/src/durable_inbox.rs` and
`crates/r3akt-rch-server/src/durable_broker.rs` were inspected in
`FreeTAKTeam/Reticulum-Community-Hub` at
`719c9dbc9ebad0790488a1e3b2dc387caea23156`. RCH persists custody before ACK and
maintains separate application/outbox processing lanes. Its schemas and
`RchStored`, `RchApplied`, `RchRejected` meanings remain application-owned. The
extraction preserves the wire envelopes consumed by RCH; no RCH source is copied
or changed and no live RCH deployment is claimed by local compatibility tests.

Dafrion's main baseline is `ad615d3b8a49a9f68b8c09a1dee274e1aa2f3ede`.
`ApplicationServices` is its established composition boundary. An explicit
`open_event_broker` helper returns the canonical crate's `Broker` directly under
the already resolved per-twin state root. Ordinary startup opens no broker;
there is no new process, endpoint, wire schema, forwarding service, or semantic
mutation path. Its existing governance journal and exact-plan approval lifecycle
are unchanged. The dependency pins both package version and immutable Git
revision, and final acceptance must use that Git source without a local patch.

The independent facade keeps publisher provenance and correlation IDs immutable,
stores operation/event fingerprints under bounded accounting, and separates
custody checkpoints from claimed/unknown/completed/failed processing states.
Its batch, payload, completion and lease bounds persist: incompatible reopen is
an explicit error. Late subscriptions explicitly choose the retained floor or
current tail and cannot reset an existing consumer. Generic event identities
cover both publication and completion, including after journal pruning.

## Local extraction validation

- Shared library: 13 unit tests and 2 independent storage integration tests pass,
  covering replay, idempotency, authenticated custody, reserved completion,
  explicit recovery, caller-transaction rollback and late subscriptions.
- Existing RPC suite: 887 unit tests, 14 durability tests and 7 propagation
  custody tests pass; existing ignored tests remain ignored. The physical guard
  regression moved with its implementation into the shared crate.
- Five new frozen-schema compatibility tests pass, including independently
  checked legacy receipt MAC bytes and failed-migration rollback.
- SDK library: 256 tests pass. The real daemon durable-broker bootstrap and
  issue-369 quality regression pass.
- Ten dependency-boundary tests pass, including rejection of any workspace
  dependency from the shared library. License/package notices pass for all 22
  publishable packages. `cargo package --locked -p r3akt-event-broker --allow-dirty`
  verifies the extracted package independently of workspace source paths.

These checks establish software compatibility for the exercised paths, not
production storage stability or physical power-cut qualification. Dafrion's
local integration evidence and exact dependency revision live in its
`docs/event-broker.md`; no live RCH deployment was part of this extraction.

Explicit generic subscription is a separate operation from legacy resume. A new
consumer may choose the current retained floor (`EarliestAvailable`) or skip
history at the current tail (`Latest`). The operation refuses to reset an
existing consumer, preserves owner/scope binding, charges metadata, and uses the
same capacity limit. Legacy LXMF resume still requires explicit recovery after
pruning, with no automatic bootstrap or history skip.

## SDK API baseline review

The extraction changes canonical ownership of three SDK broker method signatures
(`broker_ack_stored`, `broker_fetch`, `broker_resume`) as rendered by
`cargo-public-api`: the request/result types now originate in
`r3akt_event_broker::contracts`. The baseline records only those three name
changes. `rns_rpc::broker` continues to re-export the same concrete types;
a compile-time SDK regression assigns each method to a function pointer using
its previous `rns_rpc::broker` parameter and result paths. The existing legacy
JSON and authenticated receipt fixtures remain unchanged. This is a reviewed
ownership change, with no wire or caller type conversion added.
