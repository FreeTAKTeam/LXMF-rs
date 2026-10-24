# Issue 663 follow-through progress

Started 2026-10-24 from main `8f872639` after the issue was reopened on
2026-10-23. Scope for this pass: [PLAN.md](PLAN.md) items 1-3, one PR each,
sequential branches. Items 4-6 remain open and are not claimed here.

| Item | State | Branch / PR / head | Evidence |
| --- | --- | --- | --- |
| 1. Fixture and cgroup sampler | PR open | `corvo/issue-663-01-harness` | Harness code, Python unit tests, 10-minute idle and ingress smoke runs on the baseline daemon (see [performance record](../../performance/rch-resource-fixes-663.md)). |
| 2. Operation-level metrics | Pending | — | — |
| 3. Refill coalescing, writer fairness, WAL maintenance | Pending | — | — |
| 4. Anonymous-memory attribution and bounds | Not started | — | — |
| 5. Poll-failure stage correlation | Not started | — | — |
| 6. Production-shaped soak and acceptance record | Not started | — | Depends on #678 or a bounded fixture; only 10-minute smoke runs exist. |

## Item 1 notes

- Fixture generator: `crates/apps/reticulumd/examples/resource_soak_663_fixture.rs`
  writes a durable-broker `reticulum.db` (bulk propagation inventory, peer history
  with mixed-case keys, broker budget), grows the WAL under a pinned reader, and
  writes a Python-compatible Reticulum storage root through
  `Transport::save_reticulum_path_table`. Synthetic identities and payloads only.
- Client driver: `crates/apps/reticulumd/examples/resource_soak_663_client.rs`
  negotiates SDK v2 over the legacy dual-endpoint ZeroMQ path RCH uses and polls
  events every 100 ms, reporting JSON lines.
- Runner and sampler: `tools/scripts/resource_soak_663.py` (`fixture`, `run`,
  `report`), wrapped by `cargo xtask resource-soak-663 -- ...`. Samples
  `smaps_rollup`, `io`, CPU ticks, cgroup v2 memory/cpu files, WAL size and the
  daemon `/metrics` JSON every few seconds; renders `report.md` with first/last
  minute medians, rates and the issue's acceptance checks.
- Limits: the ingress profile drives real announces from a second daemon but no
  inbound/outbound/fanout messages; a 10-minute run is a harness smoke check.

## Baseline findings to carry forward

- Idle baseline: about eight store read operations per empty SDK poll and about
  2,080 read syscalls per second with three writes in ten minutes. Attribute
  the request path in item 2 and bound it in item 3.
- Idle baseline: anonymous memory grew by about 35 MiB in ten minutes with no
  traffic. Item 4 owns the attribution; item 2 adds the table gauges it needs.
- Startup on the production-shaped fixture took 206 s to `/readyz` on two
  CPUs (WAL replay plus 100,000 cached announces). Not an acceptance criterion,
  but it bounds how fast a restart recovers.
