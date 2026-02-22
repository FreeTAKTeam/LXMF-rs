# ADR 0006: Unsafe Code Audit Governance

- Status: Accepted
- Date: 2026-02-21
- Decision owners: `FreeTAKTeam`

## Context
The SDK v2.5 hard-break direction requires deterministic and auditable safety posture.
The workspace currently has no Rust `unsafe` blocks, and workspace lint policy sets
`unsafe_code = "forbid"`. This is strong, but it is not sufficient alone:

1. New crates or targets could bypass lint inheritance by accident.
2. Future targeted `unsafe` exceptions could land without invariant review.
3. Review accountability must remain explicit in repository governance.

## Decision
Adopt an explicit unsafe governance model with both process and CI enforcement:

1. Source-of-truth policy: `docs/architecture/unsafe-code-policy.md`.
2. Source-of-truth inventory: `docs/architecture/unsafe-inventory.md`.
3. Automated gate: `tools/scripts/check-unsafe.sh`, executed via
   `cargo xtask ci --stage unsafe-audit-check`.
4. Reviewer enforcement: dedicated CODEOWNERS entries for unsafe policy/inventory/ADR/script.

The unsafe audit gate must fail when any of the following occurs:

1. Rust `unsafe` appears without a matching inventory record.
2. An inventory record is stale or points to non-unsafe code.
3. A Rust `unsafe` site lacks local `SAFETY:` invariant commentary.
4. Unsafe governance policy files are missing mandatory markers.
5. CI workflow or CODEOWNERS coverage for unsafe governance is removed.

## Consequences
1. Safety posture becomes continuously auditable instead of convention-based.
2. Introducing `unsafe` requires explicit design accountability and review trail.
3. Governance overhead increases slightly, but hidden memory-safety risk drops.
4. Future embedded/performance exceptions can be admitted only with documented invariants.
