# ADR-0001: Reliability-First Core Surface

## Status
Superseded by ADR-0003 for repository topology; core-surface intent remains accepted.

## Context
The repository had drifted toward mixed protocol/runtime concerns with broad dependency exposure.

## Decision
- Keep default dependency surfaces minimal.
- Keep CLI dependencies optional relative to protocol surfaces.
- Define explicit stable surface modules for message, identity, routing API, and errors.

## Consequences
- Breaking change for consumers relying on broad exports.
- Lower default dependency and compile footprint.
- Clearer separation for future operator tooling crates.
