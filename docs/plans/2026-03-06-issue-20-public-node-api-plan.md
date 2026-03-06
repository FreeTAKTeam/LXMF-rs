# Issue #20 Public Node API Implementation Plan

## Goal

Implement the public embedded node API requested in GitHub issue `#20` so host and firmware integrations can control a native node through a stable, FFI-safe facade instead of the current low-level tick-and-wire entrypoints only.

Requested surface from the issue:

- `Node::new()`
- `start(config: NodeConfig)`
- `stop()`
- `restart(config: NodeConfig)`
- `get_status() -> NodeStatus`
- `send(destination, data, options) -> SendReceipt`
- `broadcast(data, options) -> SendReceipt`
- `set_log_level(level)`
- `subscribe_events() -> EventSubscription`
- `EventSubscription::next(timeout_ms) -> NodeEvent?`
- `EventSubscription::close()`
- structured `NodeError`

## Current State

The relevant implementation is split across three crates:

- [crates/libs/rns-embedded-core/src/lib.rs](../../crates/libs/rns-embedded-core/src/lib.rs)
  provides protocol/storage/transport primitives and the current `EmbeddedError`.
- [crates/libs/rns-embedded-runtime/src/lib.rs](../../crates/libs/rns-embedded-runtime/src/lib.rs)
  provides `EmbeddedNodeRuntime`, queueing, lifecycle transitions, and internal `RuntimeEvent`s.
- [crates/libs/rns-embedded-ffi/src/lib.rs](../../crates/libs/rns-embedded-ffi/src/lib.rs)
  exports the current C ABI: create/free, manual tick, link-state setters, inbound/outbound wire transfer, and message queueing.

Current gaps against issue `#20`:

1. There is no explicit started/stopped node state distinct from transport lifecycle.
2. There is no stable public event model or subscription handle.
3. There is no `NodeError` model for lifecycle/configuration failures.
4. The exposed C ABI remains transport-centric rather than node-centric.
5. The header file at [crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h](../../crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h) will need to evolve in lockstep with any ABI additions.
6. There is no capability-discovery contract for clients that need to adapt to `std` vs `alloc` feature differences.
7. There is no runtime epoch model to distinguish stale receipts/events after `restart(config)`.
8. Ownership and blocked-wait semantics are not explicit enough for cross-language SDKs.

## Non-Goals

- Full parity with `lxmf-sdk` desktop semantics.
- Full group-membership discovery, persistence, and policy management in this issue; the node/SDK may expose fanout, but group resolution policy should stay explicit.
- Replacing the existing low-level FFI entrypoints in the first slice.
- Removing manual tick semantics from embedded integrations.
- Delivering blocking `next(timeout_ms)` semantics for `alloc` firmware builds before a monotonic time/synchronization contract exists.

## Locked Decisions Required Before Coding

1. `broadcast(...)` semantics must be fixed before implementation.
   Prototype input from `FreeTAKTeam/reticulum_mobile_emergency_management` is useful here:
   - its `broadcast_bytes` command fans out bytes to connected peers
   - its send outcomes distinguish `SentDirect` vs `SentBroadcast`
   Recommended decision: do not define `broadcast` as an announce-style transport primitive.
   Instead, define `broadcast` as higher-level fanout over a destination set resolved by `BroadcastOptions`, for example:
   - explicit destination list
   - connected-peer set
   - named group/chat membership resolved by the caller or SDK layer
   Rejected alternatives:
   - announce-style transport broadcast, because that is a different primitive with different receipt/event semantics
   - implicit network-wide broadcast with no target set, because it is too ambiguous for delivery status and error reporting

2. `next(timeout_ms)` semantics must remain non-panicking and deterministic.
   Recommended decision:
   - in `std` builds, `next(timeout_ms)` blocks on a condition variable that is signaled by concurrent producer calls such as `tick`, `send`, `broadcast`, `start`, and `stop`
   - `next()` never advances transport progress by itself
   - in single-threaded/manual-tick usage, callers use `next(0)` after their own `tick()` loop
   - in `alloc` builds for this issue, only non-blocking polling is in scope

3. ABI compatibility strategy must be incremental.
   Recommended decision: add the new node-centric entrypoints while preserving existing low-level functions as compatibility shims for at least one release cycle.

4. Constructor semantics must be single-path and consistent.
   Recommended decision: `Node::new()` creates a stopped node with default internal state and no active runtime session; `start(config)` is the only path that applies a caller-provided runtime config.
   Compatibility rule:
   - keep the existing `rns_embedded_node_new(const RnsEmbeddedNodeConfig *config)` entrypoint as a low-level compatibility constructor
   - add a new no-arg node-centric constructor for the higher-level API

5. Delivery scope for the first slice must be explicit.
   Recommended decision: the full `start/stop/restart/subscribe_events/next(timeout_ms)` contract is release-blocking for the `std` FFI profile first.
   For `alloc` firmware builds in this issue:
   - preserve manual-tick compatibility
   - keep the low-level API operational
   - defer blocking wait semantics unless a monotonic clock and synchronization model are pinned

6. The `std` producer model must be mandatory, not implied.
   Recommended decision:
   - the `std` node-centric facade owns a managed driver thread started by `start(config)`
   - that driver thread advances the underlying manual-tick runtime and signals subscription waiters
   - the existing low-level/manual-tick entrypoints remain available for compatibility and `alloc` firmware builds

7. Capability discovery must be part of the stable contract.
   Recommended decision:
   - expose both compile-time header macros and a runtime capability probe
   - clients must be able to detect support for blocking `next(timeout_ms)`, managed runtime mode, broadcast target modes, event-gap signaling, and event/payload size limits before using them
   - ABI version probing alone is insufficient for feature negotiation across `std` and `alloc` profiles

8. ABI evolution must use self-describing structs.
   Recommended decision:
   - every new public input/output struct begins with `struct_size` and `struct_version`
   - public structs include reserved bytes/fields for additive growth
   - unknown trailing fields are ignored; missing trailing fields use documented defaults

9. Restart semantics must be explicit.
   Recommended decision:
   - a successful `start(config)` or `restart(config)` increments a monotonic `epoch: u64`
   - `epoch` is included in `NodeStatus`, `NodeOperationReceipt`, `NodeEvent`, and subscription state
   - subscriptions must deterministically surface `NodeRestarted` or equivalent stale-generation signaling rather than silently mixing generations

## Success Criteria

1. A caller can create a node, start it, stop it, restart it with a new config, and inspect status without touching transport internals directly.
2. A caller can subscribe to a stable event stream and read events through an opaque subscription handle.
3. Lifecycle/configuration/transport failures map to deterministic `NodeError` values.
4. No panic crosses the FFI boundary in any `std` or `alloc` build profile.
5. Existing firmware bridge and `rnx` diagnostics remain functional during the migration.
6. The new ABI surface is reflected in the public header and covered by tests.
7. For `std` builds, node and subscription handles are safe for concurrent use because the node-centric facade is internally synchronized and owns a managed producer loop.
8. Clients can discover supported features and limits without trial-and-error.
9. Events and receipts can be correlated reliably across restart boundaries.

## Architecture Direction

### 1. Keep the existing crate split

- `rns-embedded-core` remains protocol-focused and should not absorb node lifecycle policy.
- `rns-embedded-runtime` becomes the source of truth for node state, public runtime events, and status snapshots.
- `rns-embedded-ffi` remains the only unsafe boundary and owns handle management, null checking, and ABI-safe translations.

This keeps the existing workspace boundary rules in [Cargo.toml](../../Cargo.toml) intact.

### 2. Add a node-centric runtime facade inside `rns-embedded-runtime`

Introduce a new public facade around `EmbeddedNodeRuntime` rather than pushing orchestration into the FFI crate. The runtime crate should own:

- `NodeConfig`
- `NodeStatus`
- `NodeRunState`
- `NodeEvent`
- `EventSubscriptionState`
- `NodeError`
- `NodeOperationReceipt`

The FFI crate should only translate those types into C-compatible forms.

Required prerequisite before the facade can actually own lifecycle:

- move concrete backend ownership out of the FFI crate and behind a runtime-owned backend/session layer
- today the FFI crate owns `JournaledEmbeddedStore` and concrete transports such as `BleShimTransport`, while `EmbeddedNodeRuntime` only operates on borrowed `transport` and `store` values during `tick(now_ms)`
- chosen implementation pattern for this issue:
  - introduce a runtime-owned `RuntimeSession` object
  - `RuntimeSession` owns the active `EmbeddedNodeRuntime`, node-owned event log, concrete store instance, concrete backend transport, and managed-mode synchronization primitives
  - `RuntimeSession` is created by `start(config)`, replaced on successful `restart(config)`, and dropped by `stop()`
  - the concrete backend transport lives behind a closed runtime-owned enum such as `NodeBackend`, for example BLE now and TCP variants in `std` builds later
  - manual mode and managed `std` mode share the same `RuntimeSession`; the only difference is whether a driver thread is attached to advance it
- until that ownership move happens, `start/stop/restart` cannot truthfully live in `rns-embedded-runtime` as more than thin wrappers around FFI-owned state
- the same prerequisite applies to synchronization: the node-centric path needs a runtime-owned synchronized interior state model, and all v1 FFI entrypoints must dispatch through that synchronized handle rather than the current raw `*mut` to `&mut` reinterpretation
- compatibility entrypoints that remain available on node-centric handles must also route through the synchronized handle layer or reject by validated handle kind; otherwise the `std` concurrency guarantees in this plan are not achievable

### 3. Preserve manual tick in the core runtime, but add a managed `std` facade

Issue `#20` asks for `start` and `stop`, but the current embedded runtime is explicitly manual-tick driven. The plan is:

- `new()` creates a stopped node facade with default internal state.
- `start(config)` validates config, constructs or reconfigures the runtime-owned backend session, marks the node runnable, and in `std` builds starts a managed driver thread.
- the driver thread owns periodic `tick(now_ms)` progression for the high-level `std` API.
- `tick(now_ms)` remains available on the low-level/manual API and remains the underlying execution primitive.
- `stop()` disables further progression and causes send/broadcast operations to fail with `NotRunning`.
- `restart(config)` performs `stop + reconfigure + start` atomically from the caller’s perspective.

This keeps the protocol engine manual-tick based while giving the `std` high-level API a real producer model for `next(timeout_ms)`.

`NodeConfig` must be the single complete source of runtime backend configuration for this facade. That means it cannot stop at logical node fields only; it must also carry the transport/store parameters currently required to construct the embedded backend, for example:

- transport kind/mode
- BLE transport tuning such as MTU hint, inbound/outbound frame capacities, and ordering mode
- any future TCP/client/server parameters needed to build the concrete transport
- storage/session limits needed during backend construction

Design rule:

- `Node::new()` is no-arg and does not allocate or bind any transport/store backend
- `start(config)` is the first point where backend-specific configuration is consumed
- `restart(config)` fully replaces the prior backend session using the same config shape
- the compatibility constructor may continue to accept `RnsEmbeddedNodeConfig`, but that struct should become a compatibility projection of the new `NodeConfig` shape rather than a separate source of truth

Managed `std` time semantics must also be fixed as part of this API slice:

- chosen decision for this issue:
  - the managed driver thread uses `std::time::Instant` as the monotonic source of truth and derives `now_ms` from elapsed milliseconds since successful `start(config)`
  - `occurred_at_ms`, timeout accounting, and announce scheduling all use that same monotonic timeline
  - the managed driver thread runs with a target tick cadence of `25ms`
  - the conformance contract is a maximum driver tick interval of `50ms` under normal operation; implementations may wake earlier on API activity or shutdown signals
  - `next(timeout_ms)` timeout behavior in managed mode is therefore validated against the same monotonic clock plus the `<=50ms` driver cadence bound
- manual/compatibility `tick(now_ms)` remains caller-supplied, but the managed path must not depend on ambient wall-clock APIs with unspecified drift semantics

### 4. Introduce a stable event contract above `RuntimeEvent`

`RuntimeEvent` is currently an internal detail. Add a public `NodeEvent` surface with the issue’s requested categories:

- `StatusChanged`
- `PacketReceived`
- `PacketSent`
- `Log`
- `Error`
- `Extension`

Mapping guidance:

- `LifecycleChanged` and run-state transitions map to `StatusChanged`
- inbound frames and decoded LXMF payloads map to `PacketReceived`
- outbound frame flushes map to `PacketSent`
- backpressure/replay/integrity failures map to `Error`
- log emission is capability-limited and may initially be sourced from explicit runtime log hooks only
- peer-related information should not be frozen as a core v1 event until the embedded runtime has a stable peer identity/source model; if needed before then, expose it as an `Extension` event guarded by capabilities

The stable event contract should distinguish between:

- stable core events: lifecycle, log, error
- compatibility events: legacy/raw wire or packet-oriented events needed by older bridges
- extension events: namespaced optional events such as peer snapshots, announce notifications, or hub-specific updates that are not guaranteed in every profile

This prevents the first public event ABI from freezing every current integration quirk into the permanent core contract.

`NodeEvent v1` freeze table:

- `StatusChanged`
  - stability: `core`
  - required envelope: `event_id`, `epoch`, `occurred_at_ms`, `kind`
- `Log`
  - stability: `core`
  - required envelope: `event_id`, `epoch`, `occurred_at_ms`, `kind`
- `Error`
  - stability: `core`
  - required envelope: `event_id`, `epoch`, `occurred_at_ms`, `kind`
- `PacketReceived`
  - stability: `compat`
  - required envelope: `event_id`, `epoch`, `occurred_at_ms`, `kind`
- `PacketSent`
  - stability: `compat`
  - required envelope: `event_id`, `epoch`, `occurred_at_ms`, `kind`
- `Extension`
  - stability: `extension`
  - required envelope: `event_id`, `epoch`, `occurred_at_ms`, `kind`
  - required payload fields: `extension_id`, extension payload

Rules:

- v1 discriminants are frozen once assigned
- `core` kinds must not change meaning within v1
- wrappers must ignore unknown `extension` kinds safely
- extensions do not allocate new top-level v1 discriminants; they use the fixed `Extension` discriminant plus a namespaced `extension_id`

### 5. Make subscriptions bounded and explicit

Subscriptions must not imply unbounded queue growth. The runtime should expose a bounded event-log/subscription mechanism with:

- fixed capacity derived from config
- deterministic overflow policy
- explicit close semantics
- no background worker requirement

Recommended policy:

- each subscription tracks its own cursor into a bounded node event log
- when a subscriber falls behind the retention window, return a deterministic gap/error event instead of silently replaying corrupted history
- the underlying event log uses globally monotonic `event_id: u64` values scoped to the node plus `epoch`
- gap detection uses those monotonic ids, not implicit array offsets
- subscriptions must not observe mixed generations without explicit `epoch` signaling

This is safer than copying every event into a separate per-subscriber queue.

### 6. Lock event delivery semantics under manual tick

`EventSubscription::next(timeout_ms)` must not implicitly drive transport progress. The contract should be:

- `tick(now_ms)` or another producer path is the only mechanism that generates new runtime events
- in `std` builds, `start(config)` launches the managed driver thread, and `next(timeout_ms)` waits on a condition variable for events produced by that thread or by concurrent API operations
- `next()` never calls `tick()`, performs I/O, or advances protocol state on its own
- if no producer progresses the node during the timeout window, `next(timeout_ms)` returns timeout/none deterministically
- in single-threaded/manual-tick usage, callers pair `tick()` with `next(0)` polling
- for `alloc` firmware builds in this issue, blocking wait is not required; non-blocking polling remains sufficient until a time source contract is pinned
- `close()`, `stop()`, `restart()`, and node destruction must wake blocked waiters deterministically
- `timeout_ms=0` means poll-only
- if an infinite-wait sentinel is supported, it must be explicit in the ABI and capability probe rather than implied
- in managed `std` mode, timeout accounting uses the facade-owned monotonic clock and the documented driver tick cadence; this timing contract is part of the conformance surface, not an implementation detail

This keeps manual tick and timeout semantics compatible instead of letting `next()` accidentally become a second execution loop.

### 7. Add capability discovery as a first-class contract

The node-centric API will serve `std` hosts, `alloc` firmware builds, mobile FFI bridges, and compatibility clients. Those consumers need stable introspection before they can safely call optional features.

Recommended capability surface:

- profile kind (`std_managed`, `alloc_manual`, or equivalent)
- supports blocking `next(timeout_ms)`
- supports managed driver thread
- supports explicit broadcast destination lists
- supports connected-peer fanout
- supports event-gap signaling
- maximum event payload bytes projected through the ABI
- maximum subscriptions
- whether compatibility/raw wire entrypoints are present

The capability probe should distinguish:

- compile-time capabilities
  - features compiled into the build/profile
- effective runtime limits
  - payload/event/subscription limits exposed by the active runtime mode

Capability discovery should exist at:

- compile time: header macros for SDK authors that compile against the C ABI
- runtime: a probe function returning feature bits and limits for dynamic consumers

Capability evolution rules:

- add `capability_schema_version` to the probe result
- unknown bits must be ignored by clients unless explicitly required
- published feature bits must never be repurposed
- additive fields/limits must preserve prior semantics when absent

### 8. Define ownership and lifetime semantics explicitly

The plan must be explicit about copy-vs-borrow and handle invalidation rules. Recommended contract:

- `send` and `broadcast` copy payload bytes during the call; callers may release input buffers immediately after return
- `subscription_next` writes into caller-provided storage only; event payload bytes are never borrowed from internal runtime memory across the ABI boundary
- if payload projection is truncated, the result must expose both the truncation flag and required/full length
- node owns the event log; subscriptions reference node-owned state through synchronized ownership
- `close()`, `stop()`, `restart()`, and node destruction must invalidate or wake blocked subscriptions in a documented way
- freeing the node before closing subscriptions must not permit use-after-free; the exact ref-counting or invalidation strategy should be documented in the runtime and FFI design

### 9. Separate stable API from compatibility API

To keep the SDK broadly usable, the plan should formally distinguish:

- stable node-centric API: lifecycle, status, capability discovery, send/broadcast, subscriptions, structured errors
- compatibility API: manual tick, raw inbound/outbound wire helpers, legacy queueing helpers
- extension API/events: optional namespaced features used by particular clients such as announce or hub-directory notifications

That separation keeps the stable contract small while still supporting current clients.

### 10. Make the permanent abstraction dual-surface explicitly

To avoid drifting between packet-oriented and message-oriented usage, the public contract should explicitly commit to a dual-surface model:

- stable core surface:
  - lifecycle and status
  - capability discovery
  - send and broadcast/fanout
  - subscriptions and structured events
  - machine-readable errors and operation correlation
- compatibility transport surface:
  - raw packet/wire helpers
  - manual tick
  - legacy queueing
  - packet-oriented events needed by older bridges
- extension surface:
  - optional namespaced events and controls for client-specific behaviors

Design rule:

- new clients should default to the stable core surface
- existing packet-oriented clients may continue using the compatibility surface during migration
- compatibility transport features should not expand the stable core surface unless they become broadly required across client types
- extension capabilities and event kinds must use namespaced identifiers and versioning rules so unknown extensions are safely ignorable by wrappers

Promotion policy:

- compatibility-surface behavior moves into stable core only when:
  - at least two distinct maintained client types need it
  - the behavior has wrapper conformance coverage
  - maintainers explicitly approve the promotion and migration note
- until then, compatibility APIs remain non-normative for new SDKs

Recommended extension identifier format:

- reuse the extension registry scheme from [docs/contracts/extension-registry.md](../contracts/extension-registry.md)
- canonical form: `<scope>.<domain>.<name>.v<major>`
- lowercase ASCII only
- wrappers must ignore unknown extension identifiers unless explicitly required by a capability contract

## Public API Shape

## Rust Runtime Surface

Recommended additions in `rns-embedded-runtime`:

```rust
pub struct NodeConfig {
    pub runtime: RuntimeConfig,
    pub backend: NodeBackendConfig,
}
pub struct NodeStatus { ... }
pub struct NodeOperationReceipt {
    pub operation: NodeOperationKind,
    pub operation_id: u64,
    pub epoch: u64,
    pub accepted_bytes: usize,
    pub queued: bool,
    pub target_count: u32,
}

pub enum NodeError {
    InvalidConfig,
    IoError,
    NetworkError,
    ReticulumError,
    AlreadyRunning,
    NotRunning,
    Timeout,
    InternalError,
}

pub enum NodeEvent { ... }

pub enum NodeBackendConfig {
    Ble(BleNodeBackendConfig),
    #[cfg(feature = "std")]
    TcpClient(TcpClientNodeBackendConfig),
    #[cfg(feature = "std")]
    TcpServer(TcpServerNodeBackendConfig),
}

pub struct EmbeddedNode {
    ...
}

impl EmbeddedNode {
    pub fn new() -> Self;
    pub fn start(&self, config: NodeConfig) -> Result<(), NodeError>;
    pub fn stop(&self) -> Result<(), NodeError>;
    pub fn restart(&self, config: NodeConfig) -> Result<(), NodeError>;
    pub fn get_status(&self) -> NodeStatus;
    pub fn send(&self, destination: [u8; 16], data: &[u8], options: SendOptions)
        -> Result<NodeOperationReceipt, NodeError>;
    pub fn broadcast(&self, data: &[u8], options: BroadcastOptions)
        -> Result<NodeOperationReceipt, NodeError>;
    pub fn set_log_level(&self, level: NodeLogLevel) -> Result<(), NodeError>;
    pub fn subscribe_events(&self) -> Result<EventSubscription, NodeError>;
}
```

`EmbeddedNodeRuntime` can remain as the protocol engine beneath this facade if that keeps refactoring smaller. The low-level/manual-tick layer can continue to use `&mut self`; the managed `std` facade should present the synchronized `&self` API above it.

Implementation note:

- this facade is expected to own the concrete backend session for its active epoch
- `EmbeddedNodeRuntime` remains the protocol engine, but the new `EmbeddedNode` layer owns the store, transport/backend instance, event log, and synchronization primitives
- `rns-embedded-ffi` should construct or call into that facade rather than continuing to own the backend directly for the node-centric path
- the concrete implementation should use an internal shape equivalent to:

```rust
struct RuntimeSession {
    epoch: u64,
    runtime: EmbeddedNodeRuntime,
    store: NodeStore,
    backend: NodeBackend,
    event_log: NodeEventLog,
    #[cfg(feature = "std")]
    driver: Option<DriverState>,
}
```

- `EmbeddedNode` then owns synchronized access to `Option<RuntimeSession>` plus stable node-level metadata needed while stopped

Recommended stable metadata carried by all events:

- `event_id: u64`
- `epoch: u64`
- `occurred_at_ms: u64`
- `kind`
- optional `operation_id: u64`

Recommended stable poll result model:

```rust
pub enum PollResult {
    Event(NodeEvent),
    Timeout,
    Closed,
    Gap { next_event_id: u64 },
    NodeStopped,
    NodeRestarted { epoch: u64 },
}
```

Recommended stable machine-readable error contract:

- keep `NodeError` as the coarse semantic category
- add a stable machine-readable numeric `error_code: u32` on all runtime/FFI error projections
- reserve numeric ranges for future additive growth and extension-specific codes
- never reuse retired numeric codes for different meanings
- require `error_code` stability across additive enum growth so wrappers and telemetry pipelines can match on it safely

Recommended baseline registry:

- `0` = `UNKNOWN`
- `1` = `INVALID_CONFIG`
- `2` = `IO_ERROR`
- `3` = `NETWORK_ERROR`
- `4` = `RETICULUM_ERROR`
- `5` = `ALREADY_RUNNING`
- `6` = `NOT_RUNNING`
- `7` = `TIMEOUT`
- `8` = `INTERNAL_ERROR`
- `9` = `INVALID_HANDLE`
- `10` = `INVALID_POINTER`
- `11` = `MODE_CONFLICT`
- `12` = `SUBSCRIPTION_CLOSED`
- `13` = `NODE_RESTARTED`
- `14` = `EVENT_GAP`
- `15` = `QUEUE_PRESSURE`

Policy:

- `1..=1023` reserved for stable core node-centric errors
- `1024..=8191` reserved for compatibility-surface errors
- `8192+` reserved for extension-defined namespaced errors
- wrappers must tolerate unknown numeric codes and map them to `UNKNOWN`

## C ABI Surface

Recommended FFI additions in [crates/libs/rns-embedded-ffi/src/lib.rs](../../crates/libs/rns-embedded-ffi/src/lib.rs) and [crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h](../../crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h):

- `rns_embedded_v1_node_new(void)`
- `rns_embedded_v1_abi_version(void)`
- `rns_embedded_v1_get_capabilities(...)`
- opaque `RnsEmbeddedEventSubscription`
- `rns_embedded_v1_node_start`
- `rns_embedded_v1_node_stop`
- `rns_embedded_v1_node_restart`
- `rns_embedded_v1_node_get_status`
- `rns_embedded_v1_node_send`
- `rns_embedded_v1_node_broadcast`
- `rns_embedded_v1_node_set_log_level`
- `rns_embedded_v1_node_subscribe_events`
- `rns_embedded_v1_subscription_next`
- `rns_embedded_v1_subscription_close`

Compatibility shims to keep for the first rollout:

- `rns_embedded_node_tick`
- `rns_embedded_node_push_inbound_wire`
- `rns_embedded_node_take_outbound_wire`
- `rns_embedded_node_queue_message`

Handle coexistence rules must be explicit for the migration window:

- legacy compatibility handles and v1 node-centric handles must be distinguishable by the library, not just by header naming
- the implementation should use an internal tagged handle/header for all exported opaque pointers so every entrypoint can validate handle kind before dispatch
- calling a legacy/manual function on a managed v1 handle must fail deterministically with `MODE_CONFLICT` or `INVALID_HANDLE`; it must never reinterpret the pointer as a legacy node layout and race internal state
- calling a v1 function on a legacy compatibility handle must fail the same way
- subscription handles need the same kind/version validation so stale or mixed-handle calls cannot pass raw-pointer checks accidentally
- this tagging/dispatch rule is required before promising that legacy `tick` returns `InvalidState` on managed handles, because the current FFI raw-pointer model is not sufficient on its own

Where possible:

- implement `send` on top of `queue_message`
- implement `broadcast` on top of a fanout queue helper that iterates a resolved destination set
- keep `queue_message` documented as low-level/legacy rather than removing it immediately
- document `rns_embedded_node_new(const RnsEmbeddedNodeConfig *config)` as a compatibility constructor distinct from the new node-centric constructor
- expose an ABI version macro in the header and a runtime probe function so consumers can reject mismatched headers/libraries cleanly
- when the managed `std` node-centric mode is running, legacy `rns_embedded_node_tick` on that handle must return `InvalidState` rather than racing the driver thread
- every new ABI struct must be self-describing via `struct_size`, `struct_version`, and reserved fields
- all node-centric errors projected through the ABI should include both semantic category and machine-readable stable code

Receipt semantics for `broadcast` must be explicit:

- `NodeOperationReceipt` means queue acceptance, not network-wide delivery confirmation
- for `broadcast`, `target_count` is the number of resolved destinations accepted into the fanout operation
- later `PacketSent` or per-destination error events confirm transmission attempts for each destination
- no group-wide delivery ACK semantics are implied

## Error Model

### Runtime Error Ownership

`EmbeddedError` in `rns-embedded-core` should remain focused on transport/protocol/storage failures. `NodeError` should live in `rns-embedded-runtime` and wrap or classify:

- config validation problems
- lifecycle violations
- timeout semantics
- translated transport/storage/runtime errors

Recommended mapping rules:

- invalid zero identity/address, invalid capacities, or illegal config combinations -> `InvalidConfig`
- using `start` when already started -> `AlreadyRunning`
- using `stop`, `send`, or `broadcast` while stopped -> `NotRunning`
- transport disconnection and link-level failures -> `NetworkError`
- replay/integrity/packet framing failures -> `ReticulumError`
- bounded wait exhaustion -> `Timeout`
- impossible state transitions or poisoned internal invariants -> `InternalError`

### FFI Error Ownership

The current `RnsEmbeddedStatus` enum is too low-level to represent the issue cleanly. Recommended approach:

- keep `RnsEmbeddedStatus` as the transport/ABI status code
- add `RnsEmbeddedNodeError` as the structured semantic error code
- define one uniform result pattern for all new node-centric FFI functions:
  - function return value is `RnsEmbeddedStatus`
  - every fallible node-centric function takes `RnsEmbeddedNodeError *out_node_error`
  - operation data continues to flow through explicit out parameters or fixed-size result structs

This removes ambiguity about where authoritative error detail lives while avoiding a breaking rewrite of the existing low-level API.

Precedence rule:

- `RnsEmbeddedStatus` reports ABI-level call status such as success, invalid pointer, invalid handle, buffer-too-small, or internal FFI failure
- `out_node_error` reports semantic node/runtime failure only when the ABI-level call itself succeeded enough to evaluate the requested operation
- pointer/handle validation failures must be represented by `RnsEmbeddedStatus` first and may leave `out_node_error` unset or set to `INVALID_POINTER` / `INVALID_HANDLE` deterministically according to the documented ABI convention

### Event ABI Ownership

Node events must not require cross-boundary heap ownership. Recommended approach:

- define a fixed-size `RnsEmbeddedNodeEvent` struct with:
  - `struct_size`
  - `struct_version`
  - `kind`
  - `event_id`
  - `epoch`
  - `occurred_at_ms`
  - optional `operation_id`
  - scalar metadata fields
  - bounded inline payload buffers for optional bytes/text
  - `payload_len`
  - `required_payload_len`
  - `truncated` flag
- `rns_embedded_subscription_next` writes into caller-owned storage
- no extra free function is required for event payloads in the first slice

This keeps the event ABI simple and avoids leaks or use-after-free risk.

`rns_embedded_subscription_next` should also expose a poll-result enum rather than overloading null/timeout/error cases implicitly.

### State and Call Matrix

The plan should define a normative call matrix for both the runtime and FFI layers.

Recommended minimum matrix:

- Stopped:
  - allowed: `get_status`, capability probe, `start`, `subscribe_events`
  - rejected: `send`, `broadcast` with `NOT_RUNNING`
  - `subscribe_events` returns a valid subscription; `next(0)` returns `Timeout` until lifecycle events are produced
- Running managed (`std`):
  - allowed: `get_status`, `send`, `broadcast`, `subscribe_events`, blocking `next`
  - rejected: legacy `tick` on the same handle with deterministic `MODE_CONFLICT`
- Running manual (`alloc`/compatibility):
  - allowed: `tick`, `get_status`, non-blocking `next(0)` if subscriptions exist
  - blocking `next(timeout_ms>0)` only if capability probe says supported
- Restarting/stopping:
  - blocked waiters must wake with deterministic poll result / error
  - in-flight subscriptions must observe generation change explicitly

The plan should also define the linearization point for:

- `start`
- `stop`
- `restart`
- `send`
- `broadcast`
- `close`

so wrapper authors know when state transitions become externally visible.

Required normative results:

- `start` from `Stopped` -> success or `INVALID_CONFIG`
- `start` from running state -> `ALREADY_RUNNING`
- `stop` from running state -> success
- `stop` from `Stopped` -> success or a documented idempotent no-op; for this plan choose success/idempotent no-op
- `restart` from `Stopped` -> equivalent to `start(config)` and increments `epoch` on success
- `subscribe_events` is always allowed on a valid node handle
- `close` on an already-closed subscription -> success/idempotent no-op

### Backpressure and Performance Contract

The plan should define minimal normative behavior for queue pressure and slow consumers.

Recommended rules:

- `send` under queue pressure:
  - deterministic reject by default with stable error code `QUEUE_PRESSURE`
  - alternate behavior must be capability-declared
- `broadcast` under queue pressure:
  - resolved destination set is snapshotted once
  - partial fanout is allowed only when receipt/event semantics report accepted target count and failures deterministically
  - if no targets are accepted because of pressure, return `QUEUE_PRESSURE`
  - if some targets are accepted and some rejected because of pressure, the receipt/event model must report partial acceptance plus per-target failure signaling
- event-log overflow:
  - bounded retention is required
  - slow consumers observe `Gap`/overflow signaling rather than silent loss
- compatibility events may be deprioritized before core events only if documented and capability-exposed

Recommended observable signals:

- queue depth and queue-capacity limit
- rejected send/broadcast count due to pressure
- event-gap count
- dropped compatibility-event count

## Thread Safety and FFI Safety Requirements

Issue `#20` explicitly requires thread safety and no panics across the FFI boundary. The implementation plan must satisfy both for the managed `std` node-centric API delivered in this issue.

### Thread Safety Plan

1. Treat the managed `std` node facade as the unit of synchronization.
2. In `std` builds, node and subscription handles must be internally synchronized so `start/stop/status/send/subscribe/next/close` are safe under concurrent host use.
3. The managed `std` facade owns the driver thread responsible for advancing the runtime and signaling event waiters.
4. In `alloc`/firmware builds, preserve the existing single-threaded/manual-tick model and do not market the higher-level blocking subscription API as available until a time/synchronization contract exists.
5. Do not claim cross-thread safety in the header or docs unless the implementation actually enforces it.
6. Document linearization and mode exclusivity explicitly:
   - which calls are legal in stopped/running/managed/legacy modes
   - what happens if legacy `tick` is called while managed mode is active
   - what blocked `next()` calls observe during `stop`, `restart`, and `close`

Recommended implementation:

- use interior synchronization for mutable node state in the `std` node-centric facade
- make the driver-thread lifecycle part of `start/stop/restart` correctness
- ensure subscription cursors and event-log retention are concurrency-safe
- keep low-level alloc-mode entrypoints available without forcing a threading model onto firmware

### Panic Containment Plan

1. Every new extern must validate pointers before dereference.
2. Every new opaque handle must have strict ownership rules and one free/close path.
3. `SAFETY:` comments must be adjacent to every unsafe site.
4. The unsafe inventory in [docs/architecture/unsafe-inventory.md](../architecture/unsafe-inventory.md) must be updated in the same PR.
5. In `std` builds, every new extern must use panic containment wrappers; no unwind may cross the ABI boundary.

## Work Plan

### Phase 0: Design Lock

1. Publish this plan and resolve the two open semantics questions:
- `broadcast` means explicit fanout over a resolved destination set, not announce-style transport broadcast
- `next(timeout_ms)` in `std` mode depends on the managed driver thread and returns timeout deterministically if that producer loop emits nothing during the wait window
2. Confirm ABI migration policy:
- additive entrypoints first
- no removal of current low-level FFI calls in the first release
- add header/runtime ABI version probes in the same slice
 - add capability discovery in the same slice
3. Confirm constructor semantics:
- `new()` is no-arg and stopped
- `start(config)` is the only config application path in the node-centric API
4. Confirm first-release profile scope:
- full node-centric API is required for `std`
- alloc profile keeps compatibility API plus any safe additive pieces that do not require blocking waits
5. Confirm managed `std` execution model:
- `start(config)` launches the driver thread
- `stop()` joins/shuts down the driver thread cleanly
- `next(timeout_ms)` depends on that managed producer loop rather than caller-discovered concurrent ticking
- legacy `tick` on a managed-mode handle returns `InvalidState`
6. Confirm epoch and poll-result semantics:
- `epoch` increments on successful `start`/`restart`
- `next(timeout_ms)` returns explicit poll outcomes (`Event`, `Timeout`, `Closed`, `Gap`, `NodeStopped`, `NodeRestarted`)
- blocked waiters are woken on `close`, `stop`, `restart`, and node destruction

### Phase 1: Runtime API Foundation

Files:

- [crates/libs/rns-embedded-runtime/src/lib.rs](../../crates/libs/rns-embedded-runtime/src/lib.rs)
- [crates/libs/rns-embedded-runtime/src/node.rs](../../crates/libs/rns-embedded-runtime/src/node.rs)

Tasks:

1. Add `NodeConfig`, `NodeStatus`, `NodeRunState`, `NodeError`, `NodeOperationReceipt`, and any options structs.
2. Define `BroadcastOptions` so the target-set source is explicit:
- explicit destination list
- connected peers
- named group handle if the caller resolves membership externally
  Define target-resolution semantics precisely:
 - when the destination set is snapshotted
 - whether duplicates are removed
 - ordering guarantees
 - behavior when some targets fail resolution
3. Introduce explicit started/stopped state on top of the current lifecycle state machine.
4. Add runtime entrypoints for `start`, `stop`, `restart`, `get_status`, `send`, `broadcast`, and `set_log_level`.
5. Add managed driver-thread ownership for the `std` facade while preserving manual `tick` underneath.
6. Add capability discovery and `epoch` to runtime-visible state.

Exit criteria:

- runtime unit tests cover legal/illegal lifecycle transitions
- `AlreadyRunning` and `NotRunning` are emitted deterministically
- `start/stop/restart` correctly manage driver-thread lifetime in `std` builds

### Phase 2: Event System

Files:

- [crates/libs/rns-embedded-runtime/src/lib.rs](../../crates/libs/rns-embedded-runtime/src/lib.rs)
- new runtime module if needed for event-log/subscription state

Tasks:

1. Define stable `NodeEvent` variants and payloads.
2. Add a bounded event log with explicit retention semantics.
3. Implement subscription creation, next-with-timeout, and close.
4. Add deterministic gap handling when a subscriber falls behind retention.
5. Define fixed-size event payload bounds for the C ABI projection.
6. Add tests proving that `next(timeout_ms)` receives events under the managed `std` producer loop.
7. Add explicit event envelope metadata (`event_id`, `epoch`, `occurred_at_ms`, optional `operation_id`).
8. Add tests for blocked waiter wakeups on `close`, `stop`, and `restart`.

Exit criteria:

- event emission is covered for start/stop/restart/send/receive/error paths
- subscription close and timeout semantics are tested

### Phase 3: FFI Surface Expansion

Files:

- [crates/libs/rns-embedded-ffi/src/lib.rs](../../crates/libs/rns-embedded-ffi/src/lib.rs)
- [crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h](../../crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h)

Tasks:

1. Add opaque event-subscription handle type.
2. Add additive node-centric constructor and lifecycle entrypoints.
3. Add one uniform `RnsEmbeddedStatus` plus `out_node_error` pattern for new node-centric functions.
4. Add ABI version macro/function and require consumer-side version check in docs/examples.
5. Translate runtime `NodeError` and `NodeEvent` into ABI-safe fixed-size forms.
6. Keep existing low-level functions as compatibility shims.
7. Add self-describing ABI structs and freeze enum discriminants explicitly.
8. Add runtime capability probe for feature bits and limits.
9. Define payload ownership/copying semantics in header comments and tests.
10. Add a frozen machine-readable error-code table to the header/docs.
11. Add capability probe schema versioning and unknown-bit handling rules to the header/docs.

Exit criteria:

- header matches the Rust ABI exactly
- invalid-handle and null-pointer tests are present for all new functions
- concurrent host-use tests exist for the `std` node-centric API
- ABI version mismatch tests exist for new consumers

### Phase 4: Compatibility and Tooling

Files:

- [docs/contracts/sdk-v2-feature-matrix.md](../contracts/sdk-v2-feature-matrix.md)
- [docs/runbooks/esp32-native-runtime-ble.md](../runbooks/esp32-native-runtime-ble.md)
- [docs/runbooks/esp32-native-runtime-tcp.md](../runbooks/esp32-native-runtime-tcp.md)
- any host or firmware bridge call sites after the ABI is updated

Tasks:

1. Update documentation for the new node lifecycle and event flow.
2. Document which functions are stable and which remain low-level compatibility entrypoints.
3. Verify whether `rnx` tooling or firmware bridge code should adopt the higher-level API immediately or later.
4. Add an old-to-new mapping table for SDK maintainers and wrapper authors.
5. Document compatibility guarantees and deprecation timelines.
6. Add “golden path” integration flows for:
 - `std` host app
 - `alloc` firmware/manual tick
 - mobile/FFI wrapper

Each flow should show:

- create node
- probe capabilities
- start
- send
- receive/poll
- stop

The plan should also include canonical expected outcomes for those flows rather than leaving them to future prose docs.

Compatibility-surface migration posture:

- after Release 1, the compatibility surface is bugfix-only by default
- new features should target the stable core surface or extension surface first
- compatibility-surface additions require explicit justification
- compatibility-surface removal requires:
  - maintained wrapper/reference conformance passing
  - published migration guidance
  - usage/migration criteria agreed by maintainers

Exit criteria:

- docs and runbooks do not describe a stale ABI

### Phase 5: Verification and Safety Audit

Files:

- [crates/libs/rns-embedded-runtime/src/lib.rs](../../crates/libs/rns-embedded-runtime/src/lib.rs)
- [crates/libs/rns-embedded-ffi/src/lib.rs](../../crates/libs/rns-embedded-ffi/src/lib.rs)
- [docs/architecture/unsafe-inventory.md](../architecture/unsafe-inventory.md)

Tasks:

1. Add runtime and FFI regression tests for:
- start/stop/restart
- status snapshots
- send/broadcast
- subscription timeout
- subscription close
- queue pressure
- invalid pointers/invalid handles
- concurrent host access for `std` builds
- ABI version match/mismatch behavior for new consumers
- managed driver-thread start/stop/restart behavior for `std` builds
- capability discovery behavior across `std` and `alloc`
- blocked waiter wakeups on `close`/`stop`/`restart`
- epoch changes and stale-generation signaling
- payload truncation and required-length reporting
- state/call matrix behavior across stopped, managed, and manual modes
- machine-readable error-code stability checks
2. Update unsafe inventory rows for every new unsafe site.
3. Run the relevant repository checks.

### Wrapper Conformance Fixtures

To make the API easy to adopt across languages, add a small wrapper-facing conformance suite and fixtures that non-Rust bindings can execute against a reference build.

Recommended cases:

- capability probe returns expected feature bits and limits
- `next(0)` poll behavior
- blocking `next(timeout_ms)` timeout behavior when supported
- `restart` increments `epoch`
- stale subscription or restarted node signaling
- payload truncation reporting includes required/full length
- machine-readable error codes for `NotRunning`, `AlreadyRunning`, and invalid-handle paths

Recommended artifact shape:

- documented fixture inputs
- expected result JSON or C-struct snapshots
- a small reference harness that wrapper authors can reuse
- deterministic transport/runtime stub so event order and payloads are reproducible
- normalized time/event-id expectations or fixture-controlled seeds
- fixture schema versioning so wrappers can pin expected outputs safely

Release posture recommendation:

- wrapper conformance should be part of Release 1 gating for the reference harness and maintained first-party wrappers

Canonical source-of-truth recommendation:

- maintain the stable numeric error-code registry in one checked-in contract artifact
- generate header/docs/tests from that artifact where practical to prevent drift

Recommended artifact:

- path: `docs/contracts/node-error-codes-v1.json`
- contents:
  - numeric code
  - symbolic name
  - semantic category
  - stability class
  - notes/mapping guidance
- generation direction:
  - contract artifact -> header constants / docs tables / test fixtures

### Golden-Path Reference Flows

The following flows should be part of the plan as authoritative reference sequences.

#### `std` managed host flow

1. Call ABI/version probe and capability probe.
2. Create node via `rns_embedded_v1_node_new`.
3. Subscribe before start.
4. Call `start(config)`.
5. Expect a lifecycle/status event carrying `epoch=1`.
6. Call `send`.
7. Poll subscription until either:
- delivery/packet outcome event carrying the same `operation_id`
- or terminal error event carrying the same `operation_id`
8. Call `stop`.
9. Expect blocked or future waits to observe `NodeStopped`/lifecycle result deterministically.

#### `alloc` manual-tick flow

1. Probe capabilities and confirm blocking wait is unsupported.
2. Create node.
3. Subscribe.
4. Start node.
5. Drive progress explicitly via `tick`.
6. Use `next(0)` polling only.
7. On restart, expect `epoch` increment and explicit restarted/generation-change signaling.

#### Mobile/FFI wrapper flow

1. Probe version and capabilities on load.
2. Create node and subscription eagerly.
3. Start node from UI/runtime settings.
4. Map `operation_id`, `epoch`, `event_id`, `error_code`, and poll results into wrapper-native types.
5. Treat unknown extension events and unknown error codes as forward-compatible values rather than fatal parser failures.

Required commands:

```bash
cargo test -p rns-embedded-runtime -p rns-embedded-ffi
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
./tools/scripts/check-boundaries.sh
```

## Compatibility Strategy

This issue should be delivered as an additive migration, not a flag day.

### Release 1

- add new runtime and FFI APIs
- add ABI version probe and header macro
- add capability discovery
- preserve existing low-level entrypoints
- update header and docs
- mark old entrypoints as low-level compatibility surface

### Release 2

- move primary firmware/bridge consumers onto the new node-centric calls
- gather feedback on whether the event contract is sufficient

### Release 3

- consider deprecating low-level entrypoints only after consumers have migrated

Deprecation policy:

- prefer time-based guarantees over a single release-cycle promise
- target at least `2` minor releases and `6` months before removing low-level compatibility entrypoints
- emit documentation and tooling warnings before removal

## Risks and Mitigations

1. `broadcast` is underspecified.
   Mitigation: lock it to explicit fanout semantics in the design phase and require `BroadcastOptions` to declare how the destination set is resolved.

2. Thread-safety language in the issue is broader than the current embedded execution model.
   Mitigation: make the managed `std` node-centric API truly concurrency-safe and explicitly limit the first release scope for alloc/manual-tick firmware builds.

3. Subscription backlog could silently lose events.
   Mitigation: use a bounded log with explicit gap signaling, not silent overwrite without notice.

4. Timeout semantics could become a hidden second execution loop.
   Mitigation: define `next(timeout_ms)` as waiting on events produced by the managed `std` driver thread or concurrent API operations only; it never drives transport work itself.

5. Header drift could break firmware integrations.
   Mitigation: update the Rust ABI and `rns_embedded_ffi.h` in the same PR and treat the header as a release artifact.

6. The FFI crate may accumulate more unsafe sites.
   Mitigation: keep all handle/pointer logic centralized, add adjacency `SAFETY:` comments, and update the unsafe inventory immediately.

## Deliverables

- New plan document for issue `#20`
- Runtime node facade and status/error/event types
- Additive C ABI entrypoints and opaque subscription handle
- Updated public header
- Regression tests for lifecycle, events, and FFI safety
- Updated unsafe inventory and runbooks

## Definition of Done

1. Issue `#20` API surface exists in runtime and FFI form with documented semantics.
2. Event subscriptions are bounded, closeable, and timeout-tested.
3. Structured node errors are emitted for lifecycle/config/runtime failures.
4. Existing low-level FFI consumers still compile and function during the migration window.
5. Repository tests and boundary checks pass.
