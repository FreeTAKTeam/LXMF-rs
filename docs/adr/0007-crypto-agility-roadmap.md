# ADR 0007: Cryptographic Agility and Algorithm Negotiation Roadmap

- Status: Accepted
- Date: 2026-02-21
- Decision owners: `FreeTAKTeam`

## Context
Current SDK/RPC operation relies on one practical algorithm profile. Future hardening
must allow additive algorithm upgrades without ecosystem breakage or silent downgrade.

## Decision
Adopt a versioned algorithm-set policy with explicit negotiation semantics:

1. Algorithm sets are identified by stable ids (`rns-a1`, `rns-a2`, ...).
2. Negotiation is capability-style:
   - client offers ordered `supported_algorithm_sets`
   - server responds with one `selected_algorithm_set`
3. Selection must fail closed when no overlap exists.
4. Downgrade is explicit and observable via negotiation result and event metadata.
5. Payload and RPC contracts carry the same algorithm-set id surface.

## Consequences
1. Crypto evolution can happen additively while preserving contract clarity.
2. Backward compatibility is explicit per set id, not inferred from release numbers.
3. CI can gate new algorithms with deterministic compatibility tests.
4. Adding a new algorithm set requires:
   - contract updates (`rpc-contract.md`, `payload-contract.md`)
   - migration notes
   - negotiation compatibility tests
