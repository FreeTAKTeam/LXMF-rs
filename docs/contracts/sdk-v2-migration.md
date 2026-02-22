# SDK Contract v2.5 (Migration and Cutover)

Status: Active, CI-enforced gates  
Contract release: `v2.5`  
Schema namespace: `v2`

## Migration Goals

1. Move consumers to `lxmf-sdk` contract methods and event semantics.
2. Preserve operational safety during hard-break rollout.
3. Keep fallback options bounded and explicit.

## Phase -1 Gate

Before behavioral migration:

1. `crates/libs/lxmf-sdk` scaffold must exist.
2. Workspace must pass:
- `cargo metadata --format-version 1 --no-deps`
- `cargo check --workspace --all-targets`

## Phase 0 Gate

Cutover map must be created and merged:

- `docs/migrations/sdk-v2.5-cutover-map.md`

The map must classify each current RPC/event consumer path:

- keep
- wrap
- deprecate

## Legacy Compatibility Window

Release index definitions:

- `N`: first release shipping SDK contract `v2.5`
- `N+1`: immediate next planned release after `N`
- `N+2`: second planned release after `N`

Compatibility window:

- Legacy switch support is allowed in `N` only.

Support-policy alignment:

- Release support windows and LTS expectations are governed by `docs/contracts/support-policy.md`.
- Migration plans must declare whether a release is `Current`, `Maintenance`, or `LTS` at cutover time.

1. Runtime switch:
- `sdk_v25_enabled`
- env override `LXMF_SDK_V25_ENABLED`
2. When disabled:
- SDK v2.5 methods may return `SDK_CAPABILITY_DISABLED` for disabled capabilities or disabled contract mode
- legacy path remains available

## Fallback Safety Rules

1. Legacy fallback is allowed only if schema compatibility preflight passes.
2. If schema compatibility fails, fallback must fail closed.
3. Operators must restore from backup for incompatible rollback.

## Storage Migration Rules

1. Ordered migrations tracked in `schema_migrations`.
2. Forward-only migrations.
3. Partial migration detection must stop startup.
4. Single-migrator lock required.
5. Backup checksum verification required before irreversible steps.

## Alias and Deprecation Timeline

Deprecated aliases must specify:

- `first_deprecated_in`
- `warn_until`
- `reject_from`
- `removed_in`
- `replacement`

Policy:

- usable in `N`
- warning in `N+1`
- rejected in `N+2`

Timeline scope clarification:

- This alias timeline applies to SDK-level method/event aliases.
- The legacy runtime switch (`sdk_v25_enabled` / `LXMF_SDK_V25_ENABLED`) is allowed only in `N`.

## Stability-Class Change Workflow

Stability classes are tracked in:

- `docs/contracts/sdk-v2-api-stability.md`

Required process for class changes:

1. Update stability classification rules in the same PR as API changes.
2. For `stable -> deprecated`, include replacement path and target removal release.
3. For `experimental -> stable`, include conformance evidence and contract updates.
4. For `internal -> stable`, include explicit compatibility commitment in release notes.
5. `cargo xtask sdk-api-break` must pass after updates.

## Marker Writer Migration (Hard Break)

`v2.5` marker semantics are no longer last-write-wins.

Required client changes:

1. Persist marker `revision` returned by `marker_create`/`marker_list`.
2. Send `expected_revision` on `marker_update_position` and `marker_delete`.
3. Handle `SDK_RUNTIME_CONFLICT` by refreshing marker state and retrying with the latest revision.
4. Treat `expected_revision=0` as invalid client input.

## Event Sink Bridge Migration

`v2.5` adds optional runtime event-sink fanout controls.

Required operator/client changes:

1. Treat sink fanout as additive to `poll_events`; do not replace cursor polling with sink delivery.
2. Use `configure.patch.event_sink` to enable sink fanout and set `allow_kinds`.
3. Keep `redaction.enabled=true` when `event_sink.enabled=true`; runtime rejects unsafe configs.
4. Update observability dashboards to monitor `sdk_event_sink_publish_total`, `sdk_event_sink_error_total`, and `sdk_event_sink_skipped_total`.

## Machine-Checkable Migration Gates

Migration gate is passing only when all checks pass:

1. `cargo xtask sdk-migration-check`
2. `cargo test -p test-support sdk_migration -- --nocapture`
3. `cargo xtask sdk-api-break`
4. `cargo xtask sdk-schema-check`
5. `cargo xtask sdk-conformance`
6. `cargo xtask changelog-migration-check`

API break baseline source of truth:

- `docs/contracts/baselines/lxmf-sdk-public-api.txt`

Generated migration notes artifact:

- `target/release-readiness/generated-migration-notes.md`

Generation command:

```bash
cargo xtask changelog-migration-check
```

## Release Readiness Requirements

Must pass:

- SDK schema checks
- SDK conformance suite
- API-break gate
- migration compatibility gate
- security dependency gates
- support policy gate (`cargo run -p xtask -- support-policy-check`)
