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
| `cancel` | required | required | required |
| `status` | required | required | required |
| `configure` | required | required | required |
| `tick` | optional | optional | required |
| `poll_events` | required | required | required |
| `subscribe_events` | required | optional | unsupported |
| `snapshot` | required | required | required |
| `shutdown` | required | required | required |

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
| `sdk.capability.markers` | optional | optional | optional |
| `sdk.capability.identity_multi` | optional | optional | optional |
| `sdk.capability.identity_import_export` | optional | optional | optional |
| `sdk.capability.identity_hash_resolution` | optional | optional | optional |
| `sdk.capability.paper_messages` | optional | optional | optional |
| `sdk.capability.remote_commands` | optional | optional | optional |
| `sdk.capability.voice_signaling` | optional | optional | optional |
| `sdk.capability.shared_instance_rpc_auth` | optional | optional | optional |

## Backend Support Matrix

| Backend | Status | Notes |
| --- | --- | --- |
| RPC-backed adapter (`rns-rpc`) | required | first implementation target for v2.5 |
| In-process runtime adapter | optional | deferred from foundation slice |
| External custom backend | experimental | allowed via backend SPI, not release-blocking |

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

## CI Mapping Matrix

| Gate | Status |
| --- | --- |
| `sdk-schema-check` | required |
| `sdk-conformance` | required |
| `sdk-property-check` | required |
| `sdk-api-break` | required |
| `sdk-security-check` | required |
| `sdk-fuzz-check` | required |
| `sdk-perf-budget-check` | required |
| `sdk-memory-budget-check` | required |
| `sdk-queue-pressure-check` | required |
| `sdk-migration-check` | required |
| `sdk-matrix-check` | required |
| `sdk-docs-check` | required |
| `sdk-cookbook-check` | required |
| `sdk-ergonomics-check` | required |
| `lxmf-cli-check` | required |
| `dx-bootstrap-check` | required |
| `supply-chain-check` | required |
| `reproducible-build-check` | required |
| `embedded-alloc` profile build | required |
