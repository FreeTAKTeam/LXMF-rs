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

## Non-Goals

- Full parity with `lxmf-sdk` desktop semantics.
- Topic/group fanout or remote-control APIs that belong at the SDK/RPC layer.
- Replacing the existing low-level FFI entrypoints in the first slice.
- Removing manual tick semantics from embedded integrations.
- Delivering blocking `next(timeout_ms)` semantics for `alloc` firmware builds before a monotonic time/synchronization contract exists.

## Locked Decisions Required Before Coding

1. `broadcast(...)` semantics must be fixed before implementation.
   Recommended decision: define `broadcast` as enqueueing a native announce-style broadcast frame for the embedded transport domain.
   Rejected alternatives:
   - topic/group fanout, because that belongs closer to `lxmf-sdk`
   - peer-list fanout, because peer tracking does not exist in the embedded runtime today

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

## Success Criteria

1. A caller can create a node, start it, stop it, restart it with a new config, and inspect status without touching transport internals directly.
2. A caller can subscribe to a stable event stream and read events through an opaque subscription handle.
3. Lifecycle/configuration/transport failures map to deterministic `NodeError` values.
4. No panic crosses the FFI boundary in any `std` or `alloc` build profile.
5. Existing firmware bridge and `rnx` diagnostics remain functional during the migration.
6. The new ABI surface is reflected in the public header and covered by tests.
7. For `std` builds, node and subscription handles are safe for concurrent use because the node-centric facade is internally synchronized and owns a managed producer loop.

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

### 3. Preserve manual tick in the core runtime, but add a managed `std` facade

Issue `#20` asks for `start` and `stop`, but the current embedded runtime is explicitly manual-tick driven. The plan is:

- `new()` creates a stopped node facade with default internal state.
- `start(config)` validates config, marks the node runnable, and in `std` builds starts a managed driver thread.
- the driver thread owns periodic `tick(now_ms)` progression for the high-level `std` API.
- `tick(now_ms)` remains available on the low-level/manual API and remains the underlying execution primitive.
- `stop()` disables further progression and causes send/broadcast operations to fail with `NotRunning`.
- `restart(config)` performs `stop + reconfigure + start` atomically from the caller’s perspective.

This keeps the protocol engine manual-tick based while giving the `std` high-level API a real producer model for `next(timeout_ms)`.

### 4. Introduce a stable event contract above `RuntimeEvent`

`RuntimeEvent` is currently an internal detail. Add a public `NodeEvent` surface with the issue’s requested categories:

- `StatusChanged`
- `PeerChanged`
- `PacketReceived`
- `PacketSent`
- `Log`
- `Error`

Mapping guidance:

- `LifecycleChanged` and run-state transitions map to `StatusChanged`
- inbound frames and decoded LXMF payloads map to `PacketReceived`
- outbound frame flushes map to `PacketSent`
- backpressure/replay/integrity failures map to `Error`
- peer-related transport state changes map to `PeerChanged` when the runtime has enough information; until then, emit only when link-state identity actually changes
- log emission is capability-limited and may initially be sourced from explicit runtime log hooks only

### 5. Make subscriptions bounded and explicit

Subscriptions must not imply unbounded queue growth. The runtime should expose a bounded event-log/subscription mechanism with:

- fixed capacity derived from config
- deterministic overflow policy
- explicit close semantics
- no background worker requirement

Recommended policy:

- each subscription tracks its own cursor into a bounded node event log
- when a subscriber falls behind the retention window, return a deterministic gap/error event instead of silently replaying corrupted history

This is safer than copying every event into a separate per-subscriber queue.

### 6. Lock event delivery semantics under manual tick

`EventSubscription::next(timeout_ms)` must not implicitly drive transport progress. The contract should be:

- `tick(now_ms)` or another producer path is the only mechanism that generates new runtime events
- in `std` builds, `start(config)` launches the managed driver thread, and `next(timeout_ms)` waits on a condition variable for events produced by that thread or by concurrent API operations
- `next()` never calls `tick()`, performs I/O, or advances protocol state on its own
- if no producer progresses the node during the timeout window, `next(timeout_ms)` returns timeout/none deterministically
- in single-threaded/manual-tick usage, callers pair `tick()` with `next(0)` polling
- for `alloc` firmware builds in this issue, blocking wait is not required; non-blocking polling remains sufficient until a time source contract is pinned

This keeps manual tick and timeout semantics compatible instead of letting `next()` accidentally become a second execution loop.

## Public API Shape

## Rust Runtime Surface

Recommended additions in `rns-embedded-runtime`:

```rust
pub struct NodeConfig { ... }
pub struct NodeStatus { ... }
pub struct NodeOperationReceipt {
    pub operation: NodeOperationKind,
    pub sequence: u32,
    pub accepted_bytes: usize,
    pub queued: bool,
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

## C ABI Surface

Recommended FFI additions in [crates/libs/rns-embedded-ffi/src/lib.rs](../../crates/libs/rns-embedded-ffi/src/lib.rs) and [crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h](../../crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h):

- `rns_embedded_node_new_v2(void)`
- `rns_embedded_abi_version(void)`
- opaque `RnsEmbeddedEventSubscription`
- `rns_embedded_node_start`
- `rns_embedded_node_stop`
- `rns_embedded_node_restart`
- `rns_embedded_node_get_status`
- `rns_embedded_node_send`
- `rns_embedded_node_broadcast`
- `rns_embedded_node_set_log_level`
- `rns_embedded_node_subscribe_events`
- `rns_embedded_subscription_next`
- `rns_embedded_subscription_close`

Compatibility shims to keep for the first rollout:

- `rns_embedded_node_tick`
- `rns_embedded_node_push_inbound_wire`
- `rns_embedded_node_take_outbound_wire`
- `rns_embedded_node_queue_message`

Where possible:

- implement `send` on top of `queue_message`
- implement `broadcast` on top of a new runtime queue helper
- keep `queue_message` documented as low-level/legacy rather than removing it immediately
- document `rns_embedded_node_new(const RnsEmbeddedNodeConfig *config)` as a compatibility constructor distinct from the new node-centric constructor
- expose an ABI version macro in the header and a runtime probe function so consumers can reject mismatched headers/libraries cleanly
- when the managed `std` node-centric mode is running, legacy `rns_embedded_node_tick` on that handle must return `InvalidState` rather than racing the driver thread

Receipt semantics for `broadcast` must be explicit:

- `NodeOperationReceipt` means queue acceptance, not network-wide delivery confirmation
- for `broadcast`, `sequence` identifies the queued announce-style frame and later `PacketSent` events confirm transmission attempts
- no peer-ack semantics are implied

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

### Event ABI Ownership

Node events must not require cross-boundary heap ownership. Recommended approach:

- define a fixed-size `RnsEmbeddedNodeEvent` struct with:
  - `kind`
  - scalar metadata fields
  - bounded inline payload buffers for optional bytes/text
  - `payload_len`
  - `truncated` flag
- `rns_embedded_subscription_next` writes into caller-owned storage
- no extra free function is required for event payloads in the first slice

This keeps the event ABI simple and avoids leaks or use-after-free risk.

## Thread Safety and FFI Safety Requirements

Issue `#20` explicitly requires thread safety and no panics across the FFI boundary. The implementation plan must satisfy both for the managed `std` node-centric API delivered in this issue.

### Thread Safety Plan

1. Treat the managed `std` node facade as the unit of synchronization.
2. In `std` builds, node and subscription handles must be internally synchronized so `start/stop/status/send/subscribe/next/close` are safe under concurrent host use.
3. The managed `std` facade owns the driver thread responsible for advancing the runtime and signaling event waiters.
4. In `alloc`/firmware builds, preserve the existing single-threaded/manual-tick model and do not market the higher-level blocking subscription API as available until a time/synchronization contract exists.
5. Do not claim cross-thread safety in the header or docs unless the implementation actually enforces it.

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
5. In `std` builds, new entrypoints should use panic containment wrappers if any code path could unwind unexpectedly.

## Work Plan

### Phase 0: Design Lock

1. Publish this plan and resolve the two open semantics questions:
- `broadcast` means announce-style runtime broadcast
- `next(timeout_ms)` in `std` mode depends on the managed driver thread and returns timeout deterministically if that producer loop emits nothing during the wait window
2. Confirm ABI migration policy:
- additive entrypoints first
- no removal of current low-level FFI calls in the first release
- add header/runtime ABI version probes in the same slice
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

### Phase 1: Runtime API Foundation

Files:

- [crates/libs/rns-embedded-runtime/src/lib.rs](../../crates/libs/rns-embedded-runtime/src/lib.rs)
- [crates/libs/rns-embedded-runtime/src/node.rs](../../crates/libs/rns-embedded-runtime/src/node.rs)

Tasks:

1. Add `NodeConfig`, `NodeStatus`, `NodeRunState`, `NodeError`, `NodeOperationReceipt`, and any options structs.
2. Introduce explicit started/stopped state on top of the current lifecycle state machine.
3. Add runtime entrypoints for `start`, `stop`, `restart`, `get_status`, `send`, `broadcast`, and `set_log_level`.
4. Add managed driver-thread ownership for the `std` facade while preserving manual `tick` underneath.

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
2. Update unsafe inventory rows for every new unsafe site.
3. Run the relevant repository checks.

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
- preserve existing low-level entrypoints
- update header and docs
- mark old entrypoints as low-level compatibility surface

### Release 2

- move primary firmware/bridge consumers onto the new node-centric calls
- gather feedback on whether the event contract is sufficient

### Release 3

- consider deprecating low-level entrypoints only after consumers have migrated

## Risks and Mitigations

1. `broadcast` is underspecified.
   Mitigation: lock it to announce-style transport broadcast in the design phase.

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
