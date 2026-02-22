# SDK v2.5 Implementation Plan (Foundation + Full Parity Roadmap)

## Summary
This plan defines a decision-complete implementation roadmap for `lxmf-sdk` as a stable embeddable API layer, then expands it to parity domains needed by RCH, Columba, and Sideband.

Delivery is a hard-break refactor with additive `v2.5` capability expansion in `schema_namespace=v2`, staged in three releases:
1. Release A: SDK foundation and gates.
2. Release B: RCH-first domain parity (topics, telemetry, attachments, markers).
3. Release C: advanced parity (identity, paper/QR, remote commands, voice signaling).

## Locked Decisions
1. Version strategy: `contract_release=v2.5` with additive capability expansion only.
2. API style: typed domain APIs via extension traits, not generic command-bus primary API.
3. Topic identity model: canonical `topic_id`, optional `topic_path` metadata.
4. Attachment persistence: backend-owned, SDK-abstracted contracts.
5. Voice scope: signaling only, no media transport implementation in this roadmap.
6. Rollout structure: three staged releases with hard acceptance gates.
7. First parity driver: RCH workloads.

## Current State (Repo-Confirmed)
1. Core contract docs exist under `docs/contracts/`.
2. SDK v2 schemas exist under `docs/schemas/sdk/v2/`.
3. Workspace currently points toward `crates/libs/lxmf-sdk` but scaffold is not yet complete.
4. Legacy/runtime transition mapping exists at `docs/migrations/sdk-v2.5-cutover-map.md` and must be expanded to all consumers.

## Scope
In scope:
1. Buildable `lxmf-sdk` crate and RPC-backed adapter.
2. Foundation lifecycle/event/idempotency/security behavior.
3. RCH parity domain contracts and SDK surfaces.
4. Advanced parity contracts for identity, paper/QR, remote commands, voice signaling.
5. CI/xtask conformance and API-break enforcement.

Out of scope:
1. Raw LXMF/RNS wire protocol replacement.
2. Python packaging/interoperability ownership in this repo.
3. Voice media pipeline and codec transport behavior.

## Phase -1 Hard Gate: Workspace Buildability
1. Create minimal `crates/libs/lxmf-sdk/Cargo.toml`.
2. Create minimal `crates/libs/lxmf-sdk/src/lib.rs`.
3. Ensure workspace includes `crates/libs/lxmf-sdk` and does not block on removed crates.
4. Required pass:
- `cargo metadata --format-version 1 --no-deps`
- `cargo check --workspace --all-targets`

No behavioral SDK implementation begins before this gate passes.

## Phase 0 Hard Gate: Compatibility Inventory
1. Expand `docs/migrations/sdk-v2.5-cutover-map.md` with complete in-repo consumer inventory.
2. Classify each path as `keep|wrap|deprecate`.
3. Include owner + removal version for every `wrap`.
4. Validate map completeness in CI (`sdk-migration-check`).

## Public Core API Contract (Authoritative for Release A)
All methods return `Result<_, SdkError>`.

```rust
pub trait LxmfSdk {
    fn start(&self, req: StartRequest) -> Result<ClientHandle, SdkError>;
    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError>;
    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError>;
    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError>;
    fn configure(
        &self,
        expected_revision: u64,
        patch: ConfigPatch,
    ) -> Result<Ack, SdkError>;
    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError>;
    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError>;
    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError>;
}

pub trait LxmfSdkManualTick {
    fn tick(&self, budget: TickBudget) -> Result<TickResult, SdkError>;
}

pub trait LxmfSdkAsync {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;
}
```

### Core Type Requirements
`StartRequest` is mandatory and includes:
1. `supported_contract_versions`
2. `requested_capabilities`
3. `config` (`SdkConfig`)

This ensures negotiation is representable in API and aligned with `command.schema.json`.

`start(StartRequest)` re-entry policy is locked:
1. If runtime is `Running` and normalized `StartRequest` equals active session request, return existing handle.
2. If runtime is `Running` and any of `supported_contract_versions`, `requested_capabilities`, or immutable config differs, return `SDK_RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG`.
3. Re-entry must not renegotiate partial session state.

`poll_events` cursor representation is locked:
1. `None` in SDK API serializes to `null` cursor on transport.
2. `Some(cursor)` serializes to opaque cursor string.
3. Degraded-stream recovery reset is always `cursor=None`.

## Event and Cursor Contract Requirements (Release A)
Mandatory event envelope fields:
1. `event_id`
2. `runtime_id`
3. `stream_id`
4. `seq_no`
5. `contract_version`
6. `ts_ms`
7. `event_type`
8. `severity`
9. `source_component`
10. `payload`
11. Optional: `operation_id`, `message_id`, `peer_id`, `correlation_id`, `trace_id`, `extensions`

Cursor rules:
1. Cursor scope: `{runtime_id, stream_id, schema_namespace}`.
2. Invalid/out-of-scope cursor returns `SDK_RUNTIME_INVALID_CURSOR`.
3. Expired cursor returns `SDK_RUNTIME_CURSOR_EXPIRED`.
4. Non-recoverable stream loss causes next `poll_events` to return `SDK_RUNTIME_STREAM_DEGRADED`.
5. Recovery from degraded state requires explicit reset: `poll_events(cursor=None, max=...)`.
6. Recovery batch must include `StreamGap`.

Limit enforcement:
1. `poll_events(max)` where `max > effective_limits.max_poll_events` must fail with `SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED`.
2. No clamping for contract violations.

## Unknown-Field and Compatibility Rules
1. Unknown fields in event payloads must be ignored.
2. Unknown command fields must be rejected with `SDK_VALIDATION_UNKNOWN_FIELD`, except explicitly declared extension fields for that command (including `extensions`).
3. Unknown keys under `configure.patch` must be rejected with `SDK_CONFIG_UNKNOWN_KEY`.
4. Unknown enum variants in event payload parsing must map to `Unknown` in runtime model parsing.
5. Unknown enum values in command/config inputs must be rejected with validation errors.
6. All major payloads include optional `extensions`.

## Security Model (Bind/Auth Split, Release A)
Bind modes:
1. `local_only`
2. `remote`

Auth modes:
1. `local_trusted`
2. `token`
3. `mtls`

Required behavior:
1. `bind_mode` and `auth_mode` are required config inputs.
2. Baseline secure local profile is `bind_mode=local_only` + `auth_mode=local_trusted`.
3. Remote bind requires explicit `token` or `mtls`.
4. Token mode must enforce replay rejection (`jti`) and claim checks (`alg`, `iss`, `aud`, `nbf`, `exp`, `iat`, `kid`).
5. Commands (read and mutating) require authn/authz.
6. Secret fields are never emitted in events/errors/logs.
7. Token verifier configuration must be explicit in config via either:
- `jwks_url` + cache policy, or
- static keyset with key IDs.
8. Allowed token algorithms must be configured as an explicit allowlist.

Release A schema alignment requirement:
1. Extend `docs/schemas/sdk/v2/config.schema.json` token-auth definitions to include verifier-source and algorithm allowlist fields required above.
2. Keep these fields optional only when `auth_mode != token`.

Deterministic security policy semantics (release-blocking):
1. Rate limits use token-bucket policy with one-minute refill windows.
2. Default limits:
- auth failures: `20/min/IP`
- command submissions: `120/min/principal`
- event polling: `240/min/principal`
3. Rate-limit violation returns `SDK_SECURITY_RATE_LIMITED` and emits audited security event.
4. Sensitive field transform policy modes:
- `hash`: SHA-256 hex digest truncated to 16 hex chars.
- `truncate`: preserve first 8 and last 4 characters, replace middle with `...`.
- `redact`: replace value with literal `[REDACTED]`.
5. Transform mode is required config for each sensitive field class and must be deterministic.

## Delivery and Idempotency Rules
1. Exactly one terminal delivery state per message, CAS-enforced.
2. Without `sdk.capability.receipt_terminality`, terminal set is `sent|failed|cancelled|expired|rejected`.
3. With `sdk.capability.receipt_terminality`, terminal set is `delivered|failed|cancelled|expired|rejected` and `sent` is non-terminal.
4. `CancelResult` for conformant `v2.5` profiles is:
- `Accepted`
- `AlreadyTerminal`
- `NotFound`
- `TooLateToCancel`
5. `Unsupported` is not valid for conformant `v2.5` profiles.
6. Idempotency scope: `(source, destination, idempotency_key)`.
7. Same key + same payload hash within TTL returns original `MessageId`.
8. Same key + different payload hash returns `SDK_VALIDATION_IDEMPOTENCY_CONFLICT`.
9. TTL evaluation uses monotonic runtime clock, not wall clock.

## Default Limits (Explicit by Profile)
All defaults are authoritative unless overridden by negotiated `effective_limits`.

1. `max_poll_events`
- `desktop-full`: `256`
- `desktop-local-runtime`: `64`
- `embedded-alloc`: `32`
2. `max_event_bytes`
- `desktop-full`: `65_536`
- `desktop-local-runtime`: `32_768`
- `embedded-alloc`: `8_192`
3. `max_batch_bytes`
- `desktop-full`: `1_048_576`
- `desktop-local-runtime`: `1_048_576`
- `embedded-alloc`: `262_144`
4. `max_extension_keys`
- `desktop-full`: `32`
- `desktop-local-runtime`: `32`
- `embedded-alloc`: `32`
5. `idempotency_ttl_ms`
- `desktop-full`: `86_400_000`
- `desktop-local-runtime`: `43_200_000`
- `embedded-alloc`: `7_200_000`

Transport/runtime implementation defaults (non-negotiated in current v2 core limits contract):
1. `max_connections`
- `desktop-full`: `64`
- `desktop-local-runtime`: `16`
- `embedded-alloc`: `4`

## Release A: Foundation Implementation
Deliverables:
1. `crates/libs/lxmf-sdk` with core traits/types/state machine and capability descriptors.
2. Canonical-signature alignment across existing contracts/schemas:
- update `docs/contracts/sdk-v2.md` public API from `start(config)` to `start(StartRequest)`
- keep `configure(expected_revision, patch)` canonical everywhere
- ensure `poll_events(cursor: Option<EventCursor>, max)` semantics match schema `null` reset behavior
3. RPC adapter module mapping SDK calls to `rns-rpc`.
4. Core SDK RPC request/response schemas under `docs/schemas/sdk/v2/rpc/` for all core SDK methods.
5. Introduce capability-gated `SdkCommand` variant transport model in `docs/schemas/sdk/v2/command.schema.json` for additive non-core features, aligned with `docs/contracts/sdk-v2.md` trait-evolution rules.
6. `rns-rpc` v2 SDK methods:
- `sdk_negotiate_v2`
- `sdk_send_v2`
- `sdk_status_v2`
- `sdk_configure_v2`
- `sdk_poll_events_v2`
- `sdk_cancel_message_v2`
- `sdk_snapshot_v2`
- `sdk_shutdown_v2`
7. Core transport mapping is locked:
- `start` -> `sdk_negotiate_v2`
- `send` -> `sdk_send_v2` (fallback to `send_message_v2` is allowed only under migration contract constraints: release `N` transition window, `sdk_v25_enabled` switch path, and backward-compatibility schema preflight)
- `cancel` -> `sdk_cancel_message_v2`
- `status` -> `sdk_status_v2`
- `configure` -> `sdk_configure_v2`
- `poll_events` -> `sdk_poll_events_v2`
- `snapshot` -> `sdk_snapshot_v2`
- `shutdown` -> `sdk_shutdown_v2`
- `tick` -> local/manual capability path unless backend explicitly advertises remote tick capability
- `subscribe_events` -> adapter stream over poll cursor unless backend advertises native subscription capability
8. `test-support` conformance harness for core rules.
9. CI and xtask foundation gates.

Release A acceptance:
1. `cargo check --workspace` passes.
2. Core conformance suite passes.
3. API-break gate active.
4. SDK foundation path works against `reticulumd` for `start/send/cancel/status/configure/poll_events/snapshot/shutdown`.
5. Profile obligations pass:
- `desktop-full`: async subscribe enabled.
- `desktop-local-runtime`: async subscribe optional but cursor replay required.
- `embedded-alloc`: manual tick required and async subscribe unsupported.
6. Embedded build gate is required for release:
- `cargo check -p lxmf-sdk --no-default-features --features embedded-alloc`

## Release B: RCH-First Domain Parity
### Contracts and Schemas
Add:
1. `docs/contracts/sdk-v2-topics.md`
2. `docs/contracts/sdk-v2-telemetry.md`
3. `docs/contracts/sdk-v2-attachments.md`
4. `docs/contracts/sdk-v2-markers.md`
5. `docs/schemas/sdk/v2/topic.schema.json`
6. `docs/schemas/sdk/v2/telemetry.schema.json`
7. `docs/schemas/sdk/v2/attachment.schema.json`
8. `docs/schemas/sdk/v2/marker.schema.json`

### SDK Extension Traits
Add typed extension traits:
1. `LxmfSdkTopics`
2. `LxmfSdkTelemetry`
3. `LxmfSdkAttachments`
4. `LxmfSdkMarkers`

### Capability IDs
1. `sdk.capability.topics`
2. `sdk.capability.topic_subscriptions`
3. `sdk.capability.topic_fanout`
4. `sdk.capability.telemetry_query`
5. `sdk.capability.telemetry_stream`
6. `sdk.capability.attachments`
7. `sdk.capability.attachment_delete`
8. `sdk.capability.markers`

### RPC Methods (v2)
1. `sdk_topic_create_v2`
2. `sdk_topic_get_v2`
3. `sdk_topic_list_v2`
4. `sdk_topic_subscribe_v2`
5. `sdk_topic_unsubscribe_v2`
6. `sdk_topic_publish_v2`
7. `sdk_telemetry_query_v2`
8. `sdk_telemetry_subscribe_v2`
9. `sdk_attachment_store_v2`
10. `sdk_attachment_get_v2`
11. `sdk_attachment_list_v2`
12. `sdk_attachment_delete_v2`
13. `sdk_attachment_download_v2`
14. `sdk_attachment_associate_topic_v2`
15. `sdk_marker_create_v2`
16. `sdk_marker_list_v2`
17. `sdk_marker_update_position_v2`
18. `sdk_marker_delete_v2`

Transport decision for Release B:
1. Domain APIs are represented as capability-gated `SdkCommand` variants in `docs/schemas/sdk/v2/command.schema.json`, plus typed extension traits in SDK.
2. Add dedicated domain schemas under `docs/schemas/sdk/v2/rpc/` for variant payload typing and adapter request/response validation.
3. Backend-specific RPC method names remain adapter-private and are not public SDK transport contract.

Release B acceptance:
1. Topic fan-out and subscription flows pass.
2. Telemetry query and stream flows pass.
3. Attachment lifecycle flows pass.
4. Marker lifecycle flows pass.
5. RCH compatibility suite passes.
6. `docs/contracts/sdk-v2-feature-matrix.md` is updated with all Release B capability IDs and validated by `sdk-matrix-check`.

## Release C: Advanced Parity (Columba + Sideband)
### Contracts and Schemas
Add:
1. `docs/contracts/sdk-v2-identity.md`
2. `docs/contracts/sdk-v2-paper.md`
3. `docs/contracts/sdk-v2-commands.md`
4. `docs/contracts/sdk-v2-voice-signaling.md`
5. `docs/contracts/sdk-v2-shared-instance-auth.md`
6. `docs/schemas/sdk/v2/identity.schema.json`
7. `docs/schemas/sdk/v2/paper.schema.json`
8. `docs/schemas/sdk/v2/command-plugin.schema.json`
9. `docs/schemas/sdk/v2/voice-signaling.schema.json`

### SDK Extension Traits
1. `LxmfSdkIdentity`
2. `LxmfSdkPaper`
3. `LxmfSdkRemoteCommands`
4. `LxmfSdkVoiceSignaling`

### Capability IDs
1. `sdk.capability.identity_multi`
2. `sdk.capability.identity_import_export`
3. `sdk.capability.identity_hash_resolution`
4. `sdk.capability.paper_messages`
5. `sdk.capability.remote_commands`
6. `sdk.capability.voice_signaling`
7. `sdk.capability.shared_instance_rpc_auth`

### RPC Methods (v2)
1. `sdk_identity_list_v2`
2. `sdk_identity_activate_v2`
3. `sdk_identity_import_v2`
4. `sdk_identity_export_v2`
5. `sdk_identity_resolve_v2`
6. `sdk_paper_encode_v2`
7. `sdk_paper_decode_v2`
8. `sdk_command_invoke_v2`
9. `sdk_command_reply_v2`
10. `sdk_voice_session_open_v2`
11. `sdk_voice_session_update_v2`
12. `sdk_voice_session_close_v2`

Transport decision for Release C:
1. Advanced parity operations also use capability-gated `SdkCommand` variants + extension traits.
2. Advanced variant payloads are defined in dedicated domain schemas under `docs/schemas/sdk/v2/rpc/`.
3. Any expansion of `SdkCommand` variant set requires simultaneous update to:
- `docs/contracts/sdk-v2.md`
- `docs/contracts/sdk-v2-feature-matrix.md`
- conformance and API-break baselines.

Release C acceptance:
1. Identity lifecycle and shared-instance auth flows pass.
2. Paper/QR flows pass with fixtures.
3. Remote command/reply authz flows pass.
4. Voice signaling state machine passes.
5. `docs/contracts/sdk-v2-feature-matrix.md` is updated with all Release C capability IDs and validated by `sdk-matrix-check`.

## CI and Gate Model
Canonical CI entrypoint:
1. `.github/workflows/ci.yml` must call `cargo xtask ci --stage <stage>` as the canonical orchestrator.
2. `Makefile` and local scripts must call the same `xtask` entrypoints.

Command mapping (must stay in sync):
1. `sdk-schema-check` -> validates all SDK schemas and fixtures.
2. `sdk-conformance` -> runs stage-scoped conformance suites.
3. `sdk-property-check` -> validates ordering/terminality/state-machine invariants.
4. `sdk-api-break` -> checks public SDK surface against baseline.
5. `sdk-security-check` -> validates auth/replay/rate-limit/redaction behavior.
6. `sdk-migration-check` -> validates cutover map and migration gates.
7. `sdk-matrix-check` -> validates feature matrix versus implemented capabilities.

## Release-Blocking Test Scenarios
P0:
1. Negotiation success and no-overlap failure.
2. Lifecycle legality for every API method.
3. Cursor monotonicity, invalid/expired behavior, and degraded-stream recovery.
4. `poll_events(max)` overflow returns `SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED`.
5. Snapshot boundary correctness.
6. Idempotent send and conflict behavior.
7. Cancel/send terminal race with single terminal result.
8. Config CAS conflict and atomic apply behavior.
9. Unknown command-field rejection and unknown event-field tolerance.
10. Bind/auth constraints and token replay rejection.
11. Secret redaction assertions in errors/events/logs.
12. API-break gate fails on intentional unclassified break.
13. Terminal-state behavior with and without `sdk.capability.receipt_terminality`.
14. Per-IP/per-principal rate-limit enforcement behavior.
15. Sensitive-field transform policy behavior (`hash|truncate|redact`) by config.

P1:
1. Monotonic-time invariants under wall-clock skew/rollback.
2. Duplicate event delivery dedupe by `{runtime_id, stream_id, event_id}`.
3. Alias warn/reject timeline behavior.
4. Matrix completeness validation.

P2:
1. Non-authoritative backend extension behavior checks.

## Important Public API / Type Additions
Core:
1. `StartRequest`
2. `SdkConfig`
3. `ConfigPatch`
4. `ClientHandle`
5. `MessageId`
6. `SendRequest`
7. `CancelResult`
8. `DeliverySnapshot`
9. `RuntimeSnapshot`
10. `TickBudget`
11. `TickResult`
12. `SdkEvent`
13. `EventBatch`
14. `EventCursor`
15. `EventSubscription`
16. `SubscriptionStart`
17. `SdkError`
18. `CapabilityDescriptor`
19. `NegotiationResponse`
20. `BindMode`
21. `AuthMode`
22. `OverflowPolicy`
23. `Severity`
24. `DeliveryState`
25. `RuntimeState`

RCH parity:
1. `TopicId`
2. `TopicRecord`
3. `TopicPath`
4. `TelemetryQuery`
5. `TelemetryPoint`
6. `AttachmentId`
7. `AttachmentMeta`
8. `MarkerId`
9. `MarkerRecord`
10. `GeoPoint`

Advanced parity:
1. `IdentityRef`
2. `IdentityBundle`
3. `PaperMessageEnvelope`
4. `RemoteCommandRequest`
5. `RemoteCommandResponse`
6. `VoiceSessionId`
7. `VoiceSessionState`

## Assumptions and Defaults
1. Repository remains Rust monorepo with Python interop kept external.
2. `Reticulum-Telemetry-Hub` is the primary parity driver for Release B.
3. Core SDK traits remain frozen for `v2.x`; parity is added via extension traits + capabilities.
4. Delivery semantics remain at-least-once with dedupe.
5. Voice parity in this roadmap is signaling-only.
6. Hard-break release model is accepted for this cycle.
