# Queue Pressure and Sizing Guide

This runbook defines queue defaults, overflow behavior, and production tuning steps for SDK v2 RPC event delivery.

## Runtime queue ceilings (hard bounds)

| Surface | Bound | Enforcement |
| --- | --- | --- |
| Legacy event queue (`event_queue`) | `32` events | hard cap in daemon push path |
| SDK sequenced event log (`sdk_event_log`) | `1024` events | hard cap in daemon push path |
| Broadcast channel (`events`) | `64` events | Tokio broadcast channel capacity |
| Outbound delivery scheduler | `LXMD_DELIVERY_QUEUE_CAPACITY`, default `16384` messages | semaphore-backed admission bound in `reticulumd` |

These queues are intentionally bounded and must never grow unbounded in-process.

## Outbound delivery scheduler

`reticulumd` accepts `sdk_send_v2` into a persistent delivery scheduler instead
of spawning a new OS thread and Tokio runtime for each message. Direct delivery
reuses an active per-peer link and defaults to ordered one-at-a-time delivery per
destination while still allowing concurrency across destinations.

Tune with environment variables before starting `reticulumd`:

| Variable | Default | Effect |
| --- | --- | --- |
| `LXMD_DELIVERY_QUEUE_CAPACITY` | `16384` | Maximum accepted delivery work retained in the daemon |
| `LXMD_DELIVERY_GLOBAL_CONCURRENCY` | `32` | Maximum delivery tasks actively sending at once |
| `LXMD_DELIVERY_PER_PEER_IN_FLIGHT` | `1` | Maximum active delivery tasks for one destination |

Inspect `delivery_pipeline` in `daemon_status_ex`, `sdk_snapshot_v2`, or
`sdk_status_v2` for `queued_total`, `queued_by_peer`, `in_flight_total`,
`in_flight_by_peer`, `accepted_total`, `completed_total`, and
`rejected_queue_full_total`.

## Overflow policy semantics

`overflow_policy` controls behavior when a queue reaches capacity.

| Policy | Behavior | Operational tradeoff |
| --- | --- | --- |
| `reject` | keep oldest events, reject new arrivals | preserves early context, drops newest |
| `drop_oldest` | evict oldest event, keep new arrival | favors fresh telemetry for active consumers |
| `block` | wait up to `block_timeout_ms` for capacity | reduces drop risk at cost of producer latency |

Notes:
- `block` requires `block_timeout_ms`.
- `drop_oldest` increments `dropped_count` and emits stream-gap metadata on reset polls.

## Recommended production defaults

| Deployment profile | `overflow_policy` | `event_stream.max_poll_events` | `event_stream.max_event_bytes` | `event_stream.max_batch_bytes` |
| --- | --- | --- | --- | --- |
| `desktop-full` | `drop_oldest` | `64` | `32768` | `1048576` |
| `desktop-local-runtime` | `drop_oldest` | `32` | `8192` | `262144` |
| `embedded-alloc` | `reject` | `256` | `65536` | `1048576` |

For highly bursty operators, switch to `block` only with measured latency budgets and explicit `block_timeout_ms`.

## Tuning workflow

1. Start with profile defaults and `drop_oldest`.
2. Monitor `dropped_count`, `StreamGap` frequency, and client poll cadence.
3. Increase client poll frequency before increasing event payload size.
4. Raise `event_stream.max_poll_events` in controlled increments.
5. Keep `max_batch_bytes >= max_event_bytes`; reject invalid configs.
6. Use `block` only for short windows and bounded timeout.

## Validation gate

Run the queue pressure gate before merging runtime/config changes:

```bash
cargo run -p xtask -- sdk-queue-pressure-check
```

This executes a sustained-load test proving both event queues remain bounded while under pressure.
