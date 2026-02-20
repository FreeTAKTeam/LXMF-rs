# Release Readiness Checklist

This checklist is the publication gate for the Rust workspace.

## 1. Parity truth

- LXMF parity status is tracked in `docs/plans/lxmf-parity-matrix.md`.
- Reticulum parity status is tracked in `docs/plans/reticulum-parity-matrix.md`.
- Both matrices must be updated when feature behavior or contracts change.

## 2. Contract and schema gates

- RPC contract remains aligned with `docs/contracts/rpc-contract.md`.
- Payload contract remains aligned with `docs/contracts/payload-contract.md`.
- Contract v2 schema artifacts remain valid:
  - `docs/schemas/contract-v2/payload-envelope.schema.json`
  - `docs/schemas/contract-v2/event-payload.schema.json`
- RPC contract checks in this repo and external interop gate are kept aligned with `docs/contracts/rpc-contract.md` and migration evidence.

## 3. API stability gates

- Public API surface checks pass for:
  - `lxmf-core`
  - `lxmf-sdk`
  - `rns-core`
  - `rns-transport`
  - `rns-rpc`
- Breaking changes are called out in migration docs under `docs/migrations/`.

## 4. CI quality gates

- `lint-format`
- `build-matrix` (stable + MSRV)
- `test-nextest-unit`
- `test-integration`
- `doc`
- `security`
- `unused-deps`
- `api-surface-check`

## 5. Local release checks

```bash
cargo xtask release-check
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

Optional soak:

```bash
./tools/scripts/soak-rnx.sh
```

## 6. Release metadata

- Workspace versions bumped intentionally.
- `Cargo.lock` committed for reproducible builds.
- Changelog/release notes summarize API and migration impacts.
- Relevant migration notes updated in `docs/migrations/`.
- RC execution and tagging follow `docs/runbooks/release-candidate-runbook.md`.
