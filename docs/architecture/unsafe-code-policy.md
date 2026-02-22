# Unsafe Code Policy

This workspace defaults to zero Rust `unsafe` usage.

## Guardrails
1. Workspace lint policy enforces `unsafe_code = "forbid"` for member crates.
2. Any temporary or targeted unsafe exception must be explicitly reviewed and documented.
3. Every `unsafe` block, function, impl, trait, or extern boundary must include a local
   `SAFETY:` comment explaining the required invariants.

## Inventory Process
1. Every active unsafe site must be recorded in `docs/architecture/unsafe-inventory.md`.
2. Inventory entries must include:
   - stable id
   - file and line
   - safety invariant summary
   - owner handle
   - last-reviewed date
3. Inventory and source must stay in lockstep:
   - new unsafe without inventory is rejected
   - stale inventory entries are rejected

## Reviewer Requirements
1. Unsafe governance artifacts must be CODEOWNERS-protected.
2. Unsafe changes require reviewer confirmation that invariants are complete and test-backed.
3. Unsafe changes must include targeted tests or justification in the same change set.

## CI Gate
Unsafe policy is enforced by:

- script: `tools/scripts/check-unsafe.sh`
- xtask stage: `cargo xtask ci --stage unsafe-audit-check`
- workflow job: `unsafe-audit-check` in `.github/workflows/ci.yml`
