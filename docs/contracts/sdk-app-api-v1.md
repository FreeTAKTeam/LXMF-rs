# SDK App API v1

Status: Draft, implementation target  
Contract family: `sdk-app`  
Contract release: `v1`  
Underlying core contract: `sdk-v2.5`

## Purpose

This document defines the default, app-facing SDK surface for clients that want the easiest possible integration path.

The app-api API is:

- event-first
- profile-aware
- policy-bearing
- backend-neutral from the consumer perspective

This contract is normative. Wrappers and language bindings must not invent their own lifecycle, retry, recovery, or ordering semantics.

## Scope

In scope:

- app-facing lifecycle API
- typed send and receive behavior
- default delivery and recovery policy
- callback and event-stream semantics
- configuration presets and policy ownership
- capability and version negotiation behavior

Out of scope:

- raw transport packet formats
- low-level manual-tick mechanics as part of the default API
- wrapper-specific syntax choices

## Design Rules

1. App API is the default API surface for normal app consumers.
2. Low-level polling, transport, wire, and manual-tick behavior are advanced-only.
3. All first-party wrappers must implement semantically identical behavior by default.
4. Policy belongs in SDK orchestration, not in each wrapper.
5. If a platform cannot support the default semantics, the deviation must be profile-declared and capability-exposed.

## Canonical App Model

The default client model consists of:

- `Node` or `Client`
- `Config`
- `SendRequest`
- `SendReceipt`
- `EventStream`
- `Error`
- `RuntimeStatus`

Default lifecycle methods:

- `start(config) -> Result<Handle, Error>`
- `stop(mode) -> Result<(), Error>`
- `send(request) -> Result<SendReceipt, Error>`
- `subscribe_events(options) -> Result<EventStream, Error>`
- `status() -> Result<RuntimeStatus, Error>`

Optional additive helpers:

- `restart(config)`
- `flush(timeout)`
- `reconnect()`
- `send_with_profile_defaults(request)`
- `send_with_options(request, options)`

## Lifecycle State Machine

App API runtime states:

- `New`
- `Starting`
- `Running`
- `Degraded`
- `Stopping`
- `Stopped`
- `Failed`

Rules:

1. `start()` is legal in `New` and `Stopped`.
2. `start()` in `Running` returns the existing active handle if the effective config is equivalent.
3. `start()` in `Running` with a non-equivalent effective config fails with `SDK_APP_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG`.
4. `send()` is legal in `Running` and may be legal in `Degraded` only if the active profile explicitly permits queued offline delivery.
5. `subscribe_events()` is legal in `Starting`, `Running`, `Degraded`, and `Stopping`.
6. `stop()` is idempotent.
7. `Failed` is terminal until explicit restart or fresh start semantics are invoked.

## Threading and Callback Guarantees

Bindings must document the execution context for callbacks/async streams, but semantic guarantees are fixed:

1. Event order is stable regardless of callback mechanism.
2. Default event delivery must not require consumer-managed locking to preserve SDK correctness.
3. Callbacks must not be invoked concurrently for the same subscription unless the binding explicitly opts into concurrent delivery and documents that profile.
4. `stop()` and `restart()` must deterministically wake blocked waiters/subscribers.
5. Event stream closure is explicit and must not silently disappear.

## Delivery Semantics

App API owns the default delivery policy.

Required default behaviors:

1. `send()` returns acceptance into the SDK-managed delivery pipeline, not final delivery proof.
2. Terminal delivery outcome is communicated by typed events and status transitions.
3. Retry scheduling, reconnect behavior, and queue-pressure policy are SDK-defined and profile-bound.
4. Idempotency and deduplication behavior must be stable across wrappers.
5. Receipts and events must remain correlatable across restart boundaries.

## Queue Pressure and Backpressure

Default consumers must not implement queue-pressure logic themselves.

Rules:

1. Queue pressure is surfaced as a typed app-api error.
2. The default policy for queue pressure is profile-defined and may be:
   - fail fast
   - bounded retry with backoff
   - queue-when-offline with durable admission
3. Partial acceptance must never be hidden from the caller.
4. If fanout helpers admit a subset of targets, that must be explicitly represented in the receipt and event stream. Silent partial success is forbidden.

## Timeout and Retry Policy

Default timeout and retry behavior must be centrally defined.

Rules:

1. Retry policy belongs to the SDK app-api layer.
2. Wrappers may expose override hooks, but overrides must start from a known contract-defined default.
3. Timeout behavior must be stable across languages.
4. “Retryable” vs “terminal” failure classification must be part of the typed error model.
5. The default Rust app surface may expose profile-derived delivery helpers that apply bounded retry and queue-pressure policy without requiring caller-owned retry loops.

## Persistence and Offline Recovery

App API must define whether caller-visible delivery survives restart and offline periods.

Required contract fields:

- durable queue support: `supported | unsupported`
- restart recovery scope
- offline send policy
- event replay/recovery guarantees

Rules:

1. If durable queueing is unsupported for a profile, that must be explicit.
2. If restart recovery is supported, queue, receipt, and event resumption semantics must be documented.
3. App API defaults must not imply durability unless the profile guarantees it.

## Capability and Version Negotiation

App API is layered on top of the broader SDK contract and may negotiate capabilities internally.

Rules:

1. Bindings must not require apps to reason about raw capability bits for the default path.
2. App API startup fails fast if required semantics are unavailable.
3. Unknown additive fields and unknown future capability flags must be ignored safely unless they are marked required-by-profile.
4. The effective app-api profile and policy set are frozen for the active session after successful start.

## Security and Identity Ownership

App API integrations must not guess security behavior.

Rules:

1. The contract must define who owns identity material, token acquisition, and secure storage integration.
2. Secrets must not appear in typed events or error payloads.
3. Remote or shared-instance modes must require explicit secure auth posture.
4. Default profiles must prefer least-privilege and redaction-enabled operation.

## Advanced Escape Hatches

The following are advanced-only:

- raw transport/wire APIs
- manual tick
- raw low-level event or poll surfaces
- profile bypasses that disable default policy behavior

Rules:

1. Advanced escape hatches must be clearly labeled non-default.
2. Using advanced mode may void some app-api guarantees and must be documented as such.
3. Wrappers should expose advanced surfaces separately from the default API.

## Acceptance Standard

This contract is complete when:

- wrapper authors can implement the default API without guessing behavior
- most apps do not need custom retry/reconnect/queue logic
- semantics are stable enough to back a language-agnostic conformance suite
