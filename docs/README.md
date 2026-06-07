# Documentation Map

Not every file under `docs/` serves the same purpose. Some files are maintained
guidance, while others are contracts, fixtures, schemas, or generated baselines
consumed by tests and tooling.

## Source-of-Truth Docs

These are the first places to update when behavior changes:

- `docs/status/current-roadmap.md`: repository-level posture, blockers, and
  execution order
- `docs/status/reticulum-parity-matrix.md`: maintained Reticulum row-level
  parity status
- `docs/status/lxmf-parity-matrix.md`: maintained LXMF row-level parity status
- `docs/contracts/`: public contracts, compatibility policy, support policy, API
  behavior, and protocol-facing guarantees
- `docs/sdk/`: integration guidance for embedding `lxmf-sdk`
- `docs/runbooks/`: operator and release procedures
- `docs/architecture/`: active architecture policy and governance docs
- `docs/adr/`: architecture decisions that explain why major directions exist

## Code-Adjacent Artifacts

These are documentation-shaped files, but they are also consumed by tests,
tooling, code generation, or CI:

- `docs/schemas/`
- `docs/fixtures/`
- `docs/openrpc/`
- `docs/contracts/baselines/`

Treat changes here with the same care you would apply to source code. Do not
delete these just because they are not linked from the root `README.md`.

## Historical and Change-Management Docs

`docs/migrations/` contains retained cutover guidance for users crossing public
API or architecture boundaries. Completed implementation plans and issue boards
are kept in Git history instead of the live documentation tree.

## Directory Guide

- `docs/status/current-roadmap.md`: current repo-wide posture and execution order
- `docs/status/reticulum-parity-matrix.md`: current Reticulum parity rows
- `docs/status/lxmf-parity-matrix.md`: current LXMF parity rows
- `docs/sdk/README.md`: starting point for SDK integrators
- `docs/lxmf-rs-api.md`: API surface and stability summary
- `docs/lxmf-cli.md`: operator CLI quick reference
- `docs/PerformancesComparison.html`: retained performance comparison snapshot;
  use the benchmarking runbook for current measurements
- `docs/runbooks/release-readiness.md`: release gate checklist
- `docs/runbooks/reticulumd-operational-deployment.md`: daemon deployment,
  probes, shutdown, and service manager examples
- `docs/runbooks/crates-io-publish-plan.md`: crates.io naming, versioning, and publish order
- `docs/contracts/support-policy.md`: support and lifecycle guarantees
- `docs/architecture/overview.md`: architecture entry point
- `docs/architecture/json-lxmf-fields.md`: JSON-to-MessagePack and field-id details

## Retention Rules

- Prefer one maintained doc over several overlapping notes.
- When you add a new canonical doc, remove the superseded one in the same PR.
- Keep completed implementation plans in Git history rather than maintaining a
  second status or roadmap system.
- Keep file paths portable. Do not commit `/Users/...` or other local absolute
  paths.
- Link from broad entry points (`README.md`, this file, package READMEs) to the
  current source-of-truth docs so stale notes do not become the default.
- If you are unsure whether a file is active, search for references in code,
  `xtask`, workflows, and other docs before deleting it.
