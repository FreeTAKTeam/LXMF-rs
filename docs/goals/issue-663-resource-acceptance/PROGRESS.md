# Issue 663 follow-through progress

Started 2026-10-24 from main `8f872639` after the issue was reopened on
2026-10-23. Scope for this pass: [PLAN.md](PLAN.md) items 1-3, one PR each,
sequential branches. The 2026-10-26 continuation below records subsequent work.

| Item | State | Branch / PR / head | Evidence |
| --- | --- | --- | --- |
| 1. Fixture and cgroup sampler | Merged | [#682](https://github.com/FreeTAKTeam/LXMF-rs/pull/682) | Harness code, Python unit tests, 10-minute idle and ingress smoke runs on the baseline daemon (see [performance record](../../performance/rch-resource-fixes-663.md)). |
| 2. Operation-level metrics | Merged | `corvo/issue-663-02-metrics`, [#683](https://github.com/FreeTAKTeam/LXMF-rs/pull/683) | `storage.refill`, `storage.wal`, `memory` and `providers` blocks in `/metrics`; four-package suite 3,293 passed, 0 failed. |
| 3. Refill coalescing, writer fairness, WAL maintenance | Merged | `corvo/issue-663-03-refill-wal`, [#685](https://github.com/FreeTAKTeam/LXMF-rs/pull/685), `c6f834b9` | Read-only refill gate, rowid-floor incremental rescan, priority-writer yield, timer-driven WAL truncate, dispatch pump backoff; merged review regressions also cover SQLite rowid reuse and inventory reordering. Before/after smoke table in the resource regression record. |
| 4. Anonymous-memory attribution and bounds | Software implemented; acceptance open | `corvo/fix-678-663` | Persistence shares immutable cached packets instead of deep-cloning every route's payload; focused COW regression and a 100k-route allocation estimate. Existing protocol population bounds remain in force. |
| 5. Poll-failure stage correlation | Local regressions passed; production correlation open | `corvo/fix-678-663` | `/metrics` exports stage outcomes and latency buckets plus a 64-entry incident ring with bounded method labels, queue depth, cumulative writer wait, refill/persistence activity and cgroup high counter. Local SDK exchange failure details include a stable category label. |
| 6. Production-shaped soak and acceptance record | Exploratory comparison recorded; multi-hour acceptance open | `corvo/fix-678-663` | Paired 10-minute idle/ingress profiles use 200k propagation entries, 100k signed route announces and the production memory/CPU limits. Reads and ingress writer wait fell, but memory/reclaim still grow and startup/listener limitations prevent acceptance. Multi-hour delivery/restart/lost-ACK soak remains outstanding. |

## 2026-10-26 continuation: issues 678 and 663

The user requested both fixes and explicitly asked for Luna agents. Three Luna
lanes cover reference-aware cache collection, persistence allocation, and polling
diagnostics; the combined branch integrates their shared persistence changes.
This is a coordinated continuation of items 4-5 and #678, rather than repeating
the already merged items 1-3. No production access, deployment, or issue closure
is part of the local evidence.

The harness also respects a shared `CARGO_TARGET_DIR`, gives parallel runs unique
systemd unit names, uses short Unix RPC socket paths from each run root, rejects
premature daemon/poller exit, and uses measured run duration when deciding whether
the multi-hour requirement was met.

The instrumented baseline identified another maintenance cost: a five-minute
retention pass over the fixture's expired peer history and over-budget payload
inventory. The continuation adds a covering retention index and 512-row
expired/orphan peer-history deletion transactions, with identity and current
expiry/orphan rechecks between pages. Candidate selection stays outside the
writer and now streams 512-row keyset pages with a captured upper rowid, avoiding
a temporary vector of the complete expired population. Per-peer and global quota
enforcement keeps the original atomic SQL.
Existing TTLs, priority weights and limits are retained. The combined five-package
suite passed 3,345 tests (135 suite ignores), the explicit allocation probe passed,
and the Python harness passed 13 tests. Final keyset paging passed a separate RPC
and daemon rerun with 1,816 tests (121 ignored); its performance is not inferred from the earlier
candidate binary.

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

## Item 2 notes

- `storage.refill` and `storage.wal` in the daemon metrics snapshot
  (`/metrics`), fed by `MessagesStore` atomics: refill invocations, pages,
  candidates examined, rows inserted, capacity stops, wall and max page time,
  skip and yield counters; WAL checkpoint attempts and outcomes, emergency
  reclaims and last WAL size.
- `memory`: process `smaps_rollup` plus the daemon's own cgroup v2
  `memory.current`, `memory.stat`, `memory.swap.current`, `memory.events`,
  `memory.high`, `memory.max`, reported as unavailable (not zero) elsewhere.
- `providers.announce_persistence` and `providers.transport_tables` from the
  daemon binary through a `MetricsProvider` registry; the path-table save now
  returns a per-save report (examined, skipped known, verified, written).
- No behavior change except that absorbed persistence triggers are counted and
  logged at debug instead of a warning on every announce while a save runs.

## Item 3 notes

- Read-only refill gate on the read connection; completed-and-unchanged or
  full-queue peers never take the write mutex.
- Incremental rescan above a persisted rowid floor; `received_at` updates and
  freed capacity force a full rescan; legacy rows migrate on first pass.
  Review regressions also cover pruning the largest rowid (SQLite can reuse it)
  and reordering inventory between full or incremental pages. A persisted rescan
  epoch invalidates the floor across these mutations without penalizing ordinary
  arrivals during a scan.
- Priority waiter count on the writer; refill pages park on a condition variable in 5 ms slices, bounded to 50 ms.
- WAL: cached budget makes the per-write emergency check SQL-free;
  `journal_size_limit = max(64 MiB, budget/64)`; a 60 s daemon timer
  (`LXMD_STORAGE_WAL_MAINTENANCE_INTERVAL_SECS`) runs PASSIVE then TRUNCATE and
  reports busy rather than blocking on readers.
- Durable dispatch pump backs off from 25 ms to 250 ms while its queue is
  empty, reset by any work or inbound command.
- Deviation from PLAN.md: no per-peer minimum refill interval; the gate makes a
  repeat refill two indexed point reads, and a timer could hide a real
  inventory change.

## Baseline findings to carry forward

- Idle: about 80 store reads per second with three writes; half is the durable
  dispatch pump polling its queue every 25 ms (item 3 adds a backoff), the rest
  is unattributed. The SDK poll handler does no store reads.
- Idle: cgroup file cache grows from 125 to 488 MiB with 1,906 MemoryHigh
  events in ten minutes. The initial announce-cache attribution was an inference;
  the 2026-10-26 operation counters instead identify periodic propagation
  retention with zero announce-persistence runs during the idle burst.
- Ingress: anonymous memory grows about 60 MiB in ten minutes under announces
  from one peer; writers waited 19 s in total across 1,903 writes. Item 3
  targets the writer occupancy, item 4 the anonymous growth.
- Startup on the production-shaped fixture takes 290 to 400 s to `/readyz` on
  two CPUs. A SIGINT during that window rewrites `destination_table` empty and
  loses the cached routes; not an acceptance criterion but worth a fix.
