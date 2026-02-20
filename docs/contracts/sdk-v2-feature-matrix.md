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

## CI Mapping Matrix

| Gate | Status |
| --- | --- |
| `sdk-schema-check` | required |
| `sdk-conformance` | required |
| `sdk-property-check` | required |
| `sdk-api-break` | required |
| `sdk-security-check` | required |
| `sdk-migration-check` | required |
| `sdk-matrix-check` | required |
| `embedded-alloc` profile build | required |
