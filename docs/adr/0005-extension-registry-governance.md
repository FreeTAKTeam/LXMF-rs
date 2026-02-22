# ADR 0005: Protocol Extension Registry Governance

- Status: Accepted
- Date: 2026-02-20
- Decision owners: `FreeTAKTeam`

## Context
SDK v2.5 introduces additive domains and extension fields across RPC/payload/event surfaces. Without a controlled namespace and ownership process, extension growth can fragment interop behavior and create undocumented compatibility breaks.

## Decision
Adopt a versioned protocol extension registry in:

- `docs/contracts/extension-registry.md`

with explicit namespace rules, owner attribution, status lifecycle (`active`, `deprecated`), and CI enforcement (`extension-registry-check`).

## Consequences
1. Extension changes become auditable and reviewable as contract changes.
2. Breaking extension changes require explicit major-versioned IDs.
3. Interop clients gain a stable source for optional capability interpretation.
4. Contract evolution cost increases slightly due added governance gates, but drift risk is materially reduced.
