# SDK Contract v2.5 (Migration and Cutover)

Status: Draft, implementation target  
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

1. Runtime switch:
- `sdk_v25_enabled`
- env override `LXMF_SDK_V25_ENABLED`
2. When disabled:
- SDK v2.5 methods return `SDK_CAPABILITY_DISABLED`
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

## Machine-Checkable Migration Gates

Migration gate is passing only when all checks pass:

1. `xtask sdk-migration-check`
2. `cargo test -p test-support sdk_migration -- --nocapture`
3. `cargo run -p xtask -- api-diff --contract sdk-v2.5`
4. `cargo run -p xtask -- sdk-schema-check`

## Release Readiness Requirements

Must pass:

- SDK schema checks
- SDK conformance suite
- API-break gate
- migration compatibility gate
- security dependency gates
