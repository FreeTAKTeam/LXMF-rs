# Documentation Map

Not every file under `docs/` serves the same purpose. Some files are source of
truth, some are historical planning records, and some are test fixtures that
just happen to live under `docs/`.

## Source-of-Truth Docs

These are the first places to update when behavior changes:

- `docs/status/`: current repository-wide delivery and parity status
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

These are useful for context, but they are not the primary source of truth for
current behavior:

- `docs/migrations/`: cutover notes and migration guidance
- `docs/releases/`: release-specific notes
- `docs/plans/`: planning documents and parity tracking
- `docs/plans/framework-parity-roadmap.md`: retained parity roadmap context

If one of these becomes obsolete and has no remaining references, it can be
deleted or folded into a newer source-of-truth doc.

## Directory Guide

- `docs/status/current-roadmap.md`: current repo-wide delivery and parity status
- `docs/sdk/README.md`: starting point for SDK integrators
- `docs/lxmf-rs-api.md`: API surface and stability summary
- `docs/lxmf-cli.md`: operator CLI quick reference
- `docs/runbooks/release-readiness.md`: release gate checklist
- `docs/runbooks/crates-io-publish-plan.md`: crates.io naming, versioning, and publish order
- `docs/contracts/support-policy.md`: support and lifecycle guarantees
- `docs/architecture/overview.md`: architecture entry point
- `docs/architecture/json-lxmf-fields.md`: JSON-to-MessagePack and field-id details

## Retention Rules

- Prefer one maintained doc over several overlapping notes.
- When you add a new canonical doc, remove the superseded one in the same PR.
- Keep file paths portable. Do not commit `/Users/...` or other local absolute
  paths.
- Link from broad entry points (`README.md`, this file, package READMEs) to the
  current source-of-truth docs so stale notes do not become the default.
- If you are unsure whether a file is active, search for references in code,
  `xtask`, workflows, and other docs before deleting it.
