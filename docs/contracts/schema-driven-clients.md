# Schema-Driven Client Generation Strategy

Status: Draft, implementation target

This contract defines deterministic client generation inputs for cross-language SDK consumers.

## Goals

1. Generate stable client surfaces from versioned JSON schemas.
2. Keep Go/JavaScript/Python clients aligned with SDK v2.5 contracts.
3. Prevent silent drift between schema contracts and generated client stubs.

## Source of Truth

Client generation manifest:

- `docs/schemas/sdk/v2/clients/client-generation-manifest.json`

Smoke fixtures:

- `docs/schemas/sdk/v2/clients/smoke-requests.json`

Core schema inputs:

- `docs/schemas/sdk/v2/rpc/sdk_negotiate_v2.schema.json`
- `docs/schemas/sdk/v2/rpc/sdk_send_v2.schema.json`
- `docs/schemas/sdk/v2/rpc/sdk_poll_events_v2.schema.json`
- `docs/schemas/sdk/v2/rpc/sdk_snapshot_v2.schema.json`
- `docs/schemas/sdk/v2/error.schema.json`

## Target Client Languages

- Go
- JavaScript/TypeScript
- Python

Each target must include:

1. schema-derived request/response types,
2. machine-code error mapping,
3. transport-agnostic RPC envelope handling.

## Versioning and Backward Compatibility

1. Contract namespace (`v2`) is immutable for generated client major version.
2. Additive schema fields must be optional in generated clients.
3. Breaking schema changes require migration notes and regenerated baselines.

## Generation and Validation

Run:

```bash
cargo run -p xtask -- schema-client-check
```

The gate runs the OpenAPI-first pipeline:

1. manifest parse and validation,
2. method extraction from schema contracts,
3. canonical OpenAPI 3.1 spec generation at `generated/clients/spec/openapi.json`,
4. canonical spec compatibility conversion to OpenAPI 3.0 for generator compatibility,
5. generator execution for Go/JavaScript/Python targets via Docker runtime,
6. generated output/hash stability check controlled by `output_validation` in manifest:
   - `committed_artifacts`: compare generated files against checked-in artifacts,
   - `target_hashes`: compare generated artifact checksums against `target_hash_file`.
7. smoke vector coverage for all discovered methods,
8. optional generated output compile checks for Go/Python artifacts (best-effort),
9. drift report emission to `target/interop/schema-client-smoke-report.txt` and `target/schema-client/spec.hash`.

Additional command:

```bash
cargo run -p xtask -- schema-client-generate
cargo run -p xtask -- schema-client-generate --check
```

`schema-client-generate` writes generated clients from the manifest and generated/updates artifacts; `--check` validates drift using manifest policy.
