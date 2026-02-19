# Async Contract Conformance Matrix

Last updated: 2026-02-09

This matrix defines first-pass scenarios for validating the async client contract in `docs/lxmf-async-api.yaml` across adapters and interop paths.

Status legend: `not-started` | `in-progress` | `done`.

## Targets

- Adapter A: Python LXMF adapter (contract wrapper around Python implementation).
- Adapter B: Rust lxmf-rs adapter (contract wrapper in `crates/libs/lxmf-runtime` and `crates/libs/lxmf-router`).

## Execution matrix

Every scenario should run in all four lanes unless marked optional.

| Lane | Sender backend | Receiver backend |
| --- | --- | --- |
| L1 | Python | Python |
| L2 | Python | Rust |
| L3 | Rust | Python |
| L4 | Rust | Rust |

## Core scenarios

| ID | Scenario | Contract assertions | L1 | L2 | L3 | L4 |
| --- | --- | --- | --- | --- | --- | --- |
| C01 | Direct send success | `send()` returns handle, progress in `0..100`, terminal `delivered` or `sent` (based on backend delivery signal model), no illegal transition | not-started | not-started | not-started | done |
| C02 | Queue then tick | Message is not terminal before `tick()`, `tick()` advances status, handle remains stable | not-started | not-started | not-started | done |
| C03 | Cancel queued message | `cancel(handle)=true` yields terminal `cancelled`, no later non-cancel terminal | not-started | not-started | not-started | done |
| C04 | Auth rejection | With `set_auth_required(true)` and no allowlist entry, terminal `rejected` | not-started | not-started | not-started | not-started |
| C05 | Allowlist success | Same destination passes when allowlisted, terminal not `rejected` | not-started | not-started | not-started | not-started |
| C06 | Ignore policy | Ignored destination yields terminal `failed` or `rejected` equivalent with normalized detail `ignored` | not-started | not-started | not-started | not-started |
| C07 | Progress monotonicity | Progress events never decrease and never exceed `100` | not-started | not-started | not-started | not-started |
| C08 | Event ordering | Per-handle event order is causal: progress before terminal | not-started | not-started | not-started | not-started |
| C09 | Unknown handle lookup | `status(unknown)=null`, `cancel(unknown)=false` | not-started | not-started | not-started | not-started |
| C10 | Adapter deferred path | Backend transport failure maps to normalized deferred/failed status without panic | not-started | optional | optional | not-started |

## Extension scenarios

| ID | Extension | Contract assertions | L1 | L2 | L3 | L4 |
| --- | --- | --- | --- | --- | --- | --- |
| E01 | Paper URI ingest | `paper.ingest_uri()` returns `destination`, `transient_id`, `duplicate` flag | not-started | not-started | not-started | not-started |
| E02 | Propagation ingest/fetch | `propagation.ingest()` count > 0 then `propagation.fetch()` succeeds | optional | not-started | not-started | not-started |
| E03 | Priority scheduling | Prioritised destination dequeues ahead of non-prioritised | not-started | not-started | not-started | not-started |
| E04 | Inbound callback bridge | Inbound backend callback appears as `inbound.received` contract event | not-started | not-started | not-started | not-started |

## Required release gate (first pass)

For migration confidence, require these before flipping clients to Rust-by-default:

- `C01` through `C09` are `done` in `L4`.
- `C01`, `C02`, `C03`, `C07`, and `C08` are `done` in `L2` and `L3`.
- `E01` is `done` in `L4`.

## Harness notes

- Use the same scenario fixture format for both adapters.
- Use deterministic tick cadence in tests; do not depend on wall-clock background loops for core assertions.
- Normalize backend-specific outcomes into contract states/events before asserting.
- Keep raw backend traces in artifacts for debugging parity failures.
