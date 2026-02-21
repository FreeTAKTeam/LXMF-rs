# SDK Contract v2.5 (Feature Matrix)

Status: Draft, implementation target  
Contract release: `v2.5`  
Schema namespace: `v2`

Legend:

- `required`: must be implemented and covered by contract tests for the profile
- `optional`: capability-gated
- `experimental`: not release-blocking in this cycle
- `unsupported`: intentionally unavailable

## API Method Matrix by Profile

| API | desktop-full | desktop-local-runtime | embedded-alloc |
| --- | --- | --- | --- |
| `start` | required | required | required |
| `send` | required | required | required |
| `send_group` | optional | optional | optional |
| `cancel` | required | required | required |
| `status` | required | required | required |
| `configure` | required | required | required |
| `tick` | optional | optional | required |
| `poll_events` | required | required | required |
| `subscribe_events` | required | optional | unsupported |
| `snapshot` | required | required | required |
| `shutdown` | required | required | required |
| `identity_announce_now` | optional | optional | optional |
| `identity_presence_list` | optional | optional | optional |
| `identity_contact_update` | optional | optional | optional |
| `identity_contact_list` | optional | optional | optional |
| `identity_bootstrap` | optional | optional | optional |

## Capability Matrix by Profile

| Capability ID | desktop-full | desktop-local-runtime | embedded-alloc |
| --- | --- | --- | --- |
| `sdk.capability.cursor_replay` | required | required | optional |
| `sdk.capability.async_events` | required | optional | unsupported |
| `sdk.capability.manual_tick` | optional | optional | required |
| `sdk.capability.token_auth` | optional | optional | optional |
| `sdk.capability.mtls_auth` | optional | optional | unsupported |
| `sdk.capability.receipt_terminality` | required | required | optional |
| `sdk.capability.config_revision_cas` | required | required | required |
| `sdk.capability.idempotency_ttl` | required | required | required (`effective_limits.idempotency_ttl_ms`) |
| `sdk.capability.topics` | optional | optional | optional |
| `sdk.capability.topic_subscriptions` | optional | optional | optional |
| `sdk.capability.topic_fanout` | optional | optional | optional |
| `sdk.capability.telemetry_query` | optional | optional | optional |
| `sdk.capability.telemetry_stream` | optional | optional | optional |
| `sdk.capability.attachments` | optional | optional | optional |
| `sdk.capability.attachment_delete` | optional | optional | optional |
| `sdk.capability.attachment_streaming` | optional | optional | optional |
| `sdk.capability.markers` | optional | optional | optional |
| `sdk.capability.identity_multi` | optional | optional | optional |
| `sdk.capability.identity_discovery` | optional | optional | optional |
| `sdk.capability.identity_import_export` | optional | optional | optional |
| `sdk.capability.identity_hash_resolution` | optional | optional | optional |
| `sdk.capability.contact_management` | optional | optional | optional |
| `sdk.capability.paper_messages` | optional | optional | optional |
| `sdk.capability.remote_commands` | optional | optional | optional |
| `sdk.capability.voice_signaling` | optional | optional | optional |
| `sdk.capability.group_delivery` | optional | optional | optional |
| `sdk.capability.event_sink_bridge` | optional | optional | optional |
| `sdk.capability.shared_instance_rpc_auth` | optional | optional | optional |
| `sdk.capability.key_management` | experimental (OS keystore/HSM hooks) | experimental (OS keystore/HSM hooks) | experimental (alloc-only key hook adapters) |

## Backend Support Matrix

| Backend | Status | Notes |
| --- | --- | --- |
| RPC-backed adapter (`rns-rpc`) | required | first implementation target for v2.5 |
| In-process runtime adapter | optional | deferred from foundation slice |
| External custom backend | experimental | allowed via backend SPI, not release-blocking |

## `no_std` / `alloc` Capability Audit

This table is the source of truth for constrained-device portability planning.

| Crate | std_required | alloc_target | status | removal_plan |
| --- | --- | --- | --- | --- |
| `lxmf-core` | `wire_fields` JSON bridge only (`std`-gated module) | message encode/decode primitives and msgpack payload model | `alloc-ready` | keep JSON conversion in `std` module and preserve alloc-only protocol core |
| `rns-core` | host entropy sources for random key generation (`rand_core/getrandom`) | packet/hash/destination/ratchet primitives | `alloc-ready` | follow-up hardening: injectable entropy adapter for `no_std` targets without OS RNG |

Status legend:
- `std-first`: currently std-coupled with documented `alloc` migration plan.
- `alloc-ready`: compile-tested in `alloc` mode.
- `planned`: identified but not yet audited.

## Security Feature Matrix

| Security Control | desktop-full | desktop-local-runtime | embedded-alloc |
| --- | --- | --- | --- |
| Local trusted auth mode (`local_trusted`) | required | required | required |
| Authz on read and mutating commands | required | required | required |
| Remote bind requires explicit auth mode (`token` or `mtls`) | required | required | required |
| Replay protection (token mode) | required | required | required |
| Per-IP/per-principal rate limits | required | required | required |
| Field redaction policy | required | required | required |
| Break-glass diagnostics with audit trail | optional | optional | unsupported |

## Compliance Deployment Profiles

Profile requirements are operationalized in `docs/runbooks/compliance-profiles.md`.

| Compliance profile | Target environment | Auth baseline | Key management baseline | Evidence bundle |
| --- | --- | --- | --- | --- |
| `regulated-baseline` | enterprise/internal regulated workloads | `token` or `mtls` for remote bind | `os_keystore` or approved custom backend with fallback checks | required |
| `regulated-strict` | high-assurance deployments | `mtls` required | `hsm` or approved `os_keystore` with rotation controls | required |

## Performance Budget Matrix

All budgets are enforced from Criterion sample data via `cargo run -p xtask -- sdk-perf-budget-check`.
Budgets are expressed as maximum latency (`p50`/`p95`/`p99` in nanoseconds) and minimum throughput (`ops/s`).

| Benchmark | p50 max (ns) | p95 max (ns) | p99 max (ns) | throughput min (ops/s) |
| --- | --- | --- | --- | --- |
| `lxmf_core/message_from_wire` | 1,500 | 2,500 | 3,500 | 500,000 |
| `lxmf_core/decode_inbound_message` | 5,000 | 9,000 | 12,000 | 150,000 |
| `lxmf_core/message_to_wire` | 2,000 | 3,000 | 4,000 | 350,000 |
| `lxmf_sdk/start` | 15,000 | 25,000 | 35,000 | 30,000 |
| `lxmf_sdk/send` | 2,000 | 3,000 | 4,500 | 350,000 |
| `lxmf_sdk/poll_events` | 300 | 450 | 650 | 20,000,000 |
| `lxmf_sdk/snapshot` | 1,500 | 2,000 | 2,500 | 600,000 |
| `rns_rpc/send_message_v2` | 100,000 | 150,000 | 220,000 | 25,000 |
| `rns_rpc/sdk_poll_events_v2` | 15,000 | 20,000 | 25,000 | 90,000 |
| `rns_rpc/sdk_snapshot_v2` | 25,000 | 35,000 | 45,000 | 45,000 |
| `rns_rpc/sdk_topic_create_v2` | 70,000 | 95,000 | 130,000 | 14,000 |

## Memory Budget Matrix

Profile memory ceilings are release-gated with `cargo run -p xtask -- sdk-memory-budget-check`.
Embedded code-size footprint is release-gated with `cargo run -p xtask -- embedded-footprint-check`.

| Profile | max_heap_bytes | max_event_queue_bytes | max_attachment_spool_bytes |
| --- | --- | --- | --- |
| `desktop-full` | 268,435,456 | 67,108,864 | 536,870,912 |
| `desktop-local-runtime` | 134,217,728 | 33,554,432 | 268,435,456 |
| `embedded-alloc` | 8,388,608 | 2,097,152 | 16,777,216 |

## Queue Pressure Matrix

Queue overflow behavior is release-gated with `cargo run -p xtask -- sdk-queue-pressure-check`.

| Surface | Bound | Policy support |
| --- | --- | --- |
| Legacy event queue (`event_queue`) | 32 events | `reject`, `drop_oldest`, `block` |
| SDK event log (`sdk_event_log`) | 1024 events | `reject`, `drop_oldest`, `block` |
| Runtime broadcast channel | 64 events | bounded channel drop behavior |
| Outbound store-forward spool (`messages` store) | configurable (`store_forward.max_messages`) | `reject_new`, `drop_oldest` + `oldest_first`/`terminal_first` eviction |
| Event sink envelope fanout | configurable (`event_sink.max_event_bytes`) | skip oversized envelope + kind-allowlist filtering |

## Operability Metrics Matrix

Metrics export is available at `GET /metrics` and covered by `cargo run -p xtask -- sdk-metrics-check`.

| Metric Surface | Export |
| --- | --- |
| SDK send/poll/cancel counters | required |
| SDK auth failures counter | required |
| SDK event-drop counters | required |
| SDK event-sink publish/error/skip counters | required |
| SDK send/poll/auth latency histograms | required |
| HTTP/RPC method counters | required |

## CI Mapping Matrix

| Gate | Status |
| --- | --- |
| `sdk-schema-check` | required |
| `sdk-conformance` | required |
| `sdk-property-check` | required |
| `sdk-api-break` | required |
| `sdk-security-check` | required |
| `sdk-fuzz-check` | required |
| `sdk-metrics-check` | required |
| `sdk-perf-budget-check` | required |
| `sdk-memory-budget-check` | required |
| `sdk-queue-pressure-check` | required |
| `sdk-migration-check` | required |
| `sdk-matrix-check` | required |
| `sdk-docs-check` | required |
| `correctness` (`miri` + `loom` + strict clippy) | required |
| `sdk-cookbook-check` | required |
| `sdk-ergonomics-check` | required |
| `sdk-incident-runbook-check` | required |
| `sdk-drill-check` | required |
| `sdk-soak-check` | required |
| `lxmf-cli-check` | required |
| `dx-bootstrap-check` | required |
| `supply-chain-check` | required |
| `reproducible-build-check` | required |
| `embedded-alloc` profile build | required |
