# Issue 663 follow-through: bounded daemon work, attribution and production-shaped acceptance

**Intent:** Close the acceptance gaps that remain after the #665 fix for
[issue 663](https://github.com/FreeTAKTeam/LXMF-rs/issues/663): bound and
measure the remaining daemon background work, attribute the new anonymous-memory
growth, correlate residual polling failures with daemon stages, and produce the
production-shaped before/after evidence the issue asked for.
**Current Behavior:** The deployed daemon (`9c8bc7b52c4ed5eeb94716247b900864d812fcb6`,
RCH preview.25) has indexed keyset refill and dirty-only announce-cache writes.
Production on 2026-10-23 still shows daemon MemoryHigh events rising by about
62,900 per day, process RSS near 399 MiB plus 162 MiB swap (roughly double the
pre-fix RSS), and about 0.12% of SDK event polls failing. No operation-level
metrics, refill coalescing, writer fairness or production-shaped soak exist.
**Expected Outcome:** Refill, announce persistence and WAL maintenance have
measured, bounded cost; the daemon exposes the counters operators need to tell
file cache from heap; anonymous growth is attributed and bounded; residual poll
failures carry stage evidence; a repeatable soak shows before/after numbers
under the production cgroup limits.
**Target-Perspective Output:** Operators read cgroup anon/file/swap, refill and
cache costs from the existing daemon metrics JSON. RCH sees fewer and better
classified poll failures. Maintainers get a soak harness they can rerun.
**Truth Owner:** `docs/status/current-roadmap.md` for posture and order,
`docs/status/lxmf-parity-matrix.md` / `reticulum-parity-matrix.md` for rows,
`docs/performance/rch-resource-fixes-663.md` for evidence. This package records
scope and execution only.
**Contract Boundary:** Keep the single SQLite writer, durable custody and
acknowledgement semantics, per-peer and global propagation limits, Python
announce/path persistence format, SDK v2 and typed ZeroMQ paths, and the
repository Rust policies (no new `unwrap`, 500 LOC modules, boundary checks).
**Cutover:** Extend existing owners (`MessagesStore`, `metrics.rs`,
`announce_persistence.rs`, `reticulum_path_store.rs`, xtask). No second metrics
stack, no new storage engine, no new daemon process.
**Kill Criteria:** Do not raise `MemoryHigh`, delete propagation history, weaken
acknowledgement or authorization, lengthen timeouts to hide stalls, or claim a
production plateau from a short local run. An unexplained RSS doubling is not
closed by file-cache arguments.
**Evidence Lane:** Software, local cgroup-constrained soak and hosted CI.
Production deployment evidence is read-only and comes from the operator.
**Plan Review Gate:** Review each PR against this plan before merge. The
issue is reopened; see "Tracking" below.

## Tracking

The operator closed #663 on 2026-10-16 and reopened it on 2026-10-23 for this
work after the same-day production follow-up. The reopening comment summarizes
this plan:
https://github.com/FreeTAKTeam/LXMF-rs/issues/663#issuecomment-6365604457.
Reference #663 in every PR, and post each item's evidence as a comment there.
Do not open a separate issue.

## What #665 already delivered (do not redo)

- `crates/libs/rns-rpc/src/storage/messages_parts/messagesstore_sections/peer_queue_refill.rs`:
  newest-first index, persisted per-peer keyset cursor, 128-candidate pages,
  inventory generation counter, writer released between pages.
- `crates/libs/rns-transport/src/transport/reticulum_path_store.rs` and
  `reticulum_announce_cache.rs`: in-memory fingerprint per cached packet,
  skip serialize/open/read/write for unchanged records, five-minute full
  verification, pruning of the in-memory map for removed routes.
- Durable-broker-only bootstrap creates the identity-capable transport.
- Regressions: `peer_queue_refill_is_bounded`, announce-cache and path-store
  unchanged-write tests, `profile_repeated_announce_cache_writes` (ignored).

## What the current source still lacks (verified at HEAD)

1. **Refill runs on every eligible announce.**
   `daemon_status_snapshot.rs:349` calls `queue_existing_propagation_for_peer`
   after each peer upsert; `default_ticket_expiry_secs.rs:70` always calls
   `mark_recent_propagation_unhandled_for_peer`. A completed, unchanged pass is
   cheap inside SQLite but still takes the shared write mutex, runs
   `mutation_admission`, and runs `reclaim_wal` (a `broker_meta` query plus
   `fs::metadata` on the WAL) for every call. There is no in-memory "nothing to
   do" check and no per-peer rate limit.
2. **No writer fairness.** `with_control_conn` in `sdk_domain_snapshot_key.rs:84`
   is a plain mutex; refill pages and broker commands compete with no priority
   or yield. Lock-wait totals exist (`write_lock_wait_ns_total`) but nothing
   says which operation held the lock.
3. **No operation-level metrics.** `crates/libs/rns-rpc/src/rpc/daemon/metrics.rs`
   has SDK, RPC, BLE and storage lock counters, but nothing for refill pages,
   candidates examined, rows inserted, refill wall time, announce-cache records
   examined/skipped/written/verified, snapshot duration, WAL size, checkpoint
   outcomes, or cgroup anon/file/swap/memory.events.
4. **WAL never shrinks in production.** `broker/profile.rs:165` truncates only
   when the WAL exceeds budget/8 (768 MiB on the production 6 GiB budget). The
   production WAL has sat at about 521 MiB since at least 2026-10-11, so both DB
   and WAL pages stay in the cached working set that trips `MemoryHigh`.
5. **Anonymous memory doubled and is unattributed.** Pre-fix daemon RSS was
   about 196 MiB; on 2026-10-23 it is about 399 MiB plus 162 MiB process swap.
   Candidates, none measured: `AnnounceTable` cache (default capacity 100,000
   packets, `transport/config.rs:13`), `PathTable` (expiry only, no capacity),
   `PacketCache` duplicate generations, the per-transport fingerprint map,
   `propagation_peer_refill_state` rows mirrored in memory, peers map, SDK event
   log and reply queues. Reticulum announce cache files numbered about 972,000
   on 2026-10-22 (#678), so the in-memory route/announce population is large.
6. **Poll failures have no stage evidence.** `zmq_metrics.rs` defines
   `ZmqStage` (DispatchWait, Handler, ResponseQueue, Delivery, ResponseConnect,
   ResponseSend) and the SDK has `failure_context.rs`, but the daemon metrics
   JSON does not export per-stage timeout/failure counts or a bounded ring of
   recent failures with lock-wait and in-flight-refill context.
7. **No production-shaped reproduction.** Nothing under `tools/scripts`,
   `xtask` or the test suites builds a ~1.1 GiB propagation DB with a large WAL
   and many routes, runs the daemon under `MemoryHigh=768M`/`MemoryMax=1G` on two
   cores, and samples cgroup/proc/metrics over hours.
8. **Disk coupling.** Any multi-hour ingress soak will refill the announce
   directory. [#678](https://github.com/FreeTAKTeam/LXMF-rs/issues/678) owns
   reference-aware cleanup; it is not implemented here but item 6 depends on it
   or on a bounded fixture.

## Ordered task board

Sequential, one PR per item, each merged before the next starts. Test-first
within each item. Every PR updates `docs/performance/rch-resource-fixes-663.md`
and, where posture changes, the roadmap and parity matrices.

### 1. Production-shaped fixture and sampler

- Owner: `xtask` (new subcommand, e.g. `resource-soak-663`) plus one Python
  sampler under `tools/scripts/` if `/proc` and cgroup reading is simpler there.
  Reuse `tcp_connection_memory_benchmark.py` and
  `collect_resource_measurements_for_wo.rs` patterns; do not add a new
  benchmarking framework.
- Fixture generator: creates a `reticulum.db` with configurable counts
  (target the production shape: on the order of 10^5 to 10^6 propagation
  entries so the file reaches about 1.1 GiB, several hundred peers with
  mixed-case histories, a WAL left near 500 MiB by not checkpointing), and a
  Reticulum storage root with on the order of 10^5 cached announce files and a
  matching `destination_table`/`tunnels`. Deterministic seed. Synthetic keys
  only; no production data.
- Runner: starts `reticulumd` in durable-broker mode under
  `systemd-run --user --scope -p MemoryHigh=768M -p MemoryMax=1G` pinned to
  two CPUs (`taskset`), or documents the equivalent cgroup v2 setup when
  `systemd-run` is unavailable. Drives the SDK with the real `lxmf-sdk` ZeroMQ
  client in two profiles: idle 100 ms event polls only; sustained announce
  ingress plus periodic inbound/outbound/fanout messages.
- Sampler: every N seconds records process RSS/swap (`smaps_rollup`), `io`
  (`rchar`, `syscr`, `read_bytes`, `write_bytes`), cgroup `memory.current`,
  `memory.stat` (anon, file, active_file, inactive_file, kernel),
  `memory.swap.current`, `memory.events` (high/max/oom), `memory.pressure`,
  `cpu.stat`, and the daemon metrics JSON. Writes JSON lines plus a Markdown
  summary with first/last-minute medians, deltas and the acceptance checks
  from the issue.
- Acceptance: a 10-minute smoke run on the baseline commit produces a report;
  the report format is reviewed before longer runs. No behavior change in this
  PR. Record the baseline numbers in the performance doc.
- Depends on: none. Parallel: no.

### 2. Operation-level metrics (issue item 5)

- Owner: `crates/libs/rns-rpc/src/rpc/daemon/metrics.rs`, `MessagesStore`
  counters next to `write_lock_wait_ns_total`, `reticulum_path_store.rs`
  (a `PathPersistenceStats` returned from `save_reticulum_path_table`),
  `announce_persistence.rs`, `broker/profile.rs`.
- Add, as monotonic counters and last-run gauges, no payloads or identities:
  refill invocations, skipped-as-complete, pages, candidates examined, rows
  inserted, capacity-reached, wall ns, write-lock wait ns attributed to
  refill; announce persistence runs, records examined, skipped-known,
  verified-unchanged, written, errors, snapshot duration ns, debounce
  coalesced triggers; WAL size bytes at last write, checkpoint attempts,
  outcomes, busy results; in-memory population gauges (announce table len,
  path table len, tunnel paths, packet cache len, fingerprint map len, peers,
  SDK event log depth is already present).
- Add a Linux-only cgroup block (anon, file, swap, memory.events high/max/oom,
  memory.current) read from `/sys/fs/cgroup` for the daemon's own cgroup, with
  explicit "unavailable" rather than zeros when not on cgroup v2.
- Tests: unit tests that each counter moves on the triggering operation;
  `cargo test -p reticulumd --test code_quality_issue_369` after touching
  diagnostics paths; module-size check on `metrics.rs` (split if needed).
- Depends on: 1. Parallel: no.

### 3. Refill coalescing, writer fairness and WAL maintenance (issue items 2 and 4)

- Owner: `peer_queue_refill.rs`, `default_ticket_expiry_secs.rs`,
  `daemon_status_snapshot.rs`, `sdk_domain_snapshot_key.rs`, `broker/profile.rs`.
- Coalescing: keep an in-memory per-peer `(observed_generation, scan_complete,
  last_refill_at)` mirror of `propagation_peer_refill_state` plus the current
  inventory generation; when a peer's pass is complete, the generation is
  unchanged and no capacity was freed, return before taking the write
  connection. Add a minimum per-peer refill interval (configurable, default on
  the order of tens of seconds) that is bypassed when capacity frees or new
  inventory arrives. Preserve static-peer behavior, completed-mark precedence,
  case-insensitive histories and the existing limits.
- Fairness: track waiting broker/control writers (a counter incremented in
  `with_control_conn` callers that serve SDK commands); between refill pages,
  if any are waiting, yield and let them run first. Bound a refill page's
  transaction wall time and record it. Do not move the scan to another thread
  without this, since the mutex still serializes.
- WAL: remove `reclaim_wal` from the per-write path. Run it from the existing
  maintenance timer with a `PASSIVE` checkpoint at a lower threshold (for
  example 64 MiB or budget/64, whichever is larger) and `TRUNCATE` only when
  no readers block it; keep the existing budget semantics. Record outcomes in
  the item-2 counters.
- Tests: refill is not invoked on the write connection when nothing changed
  (assert `write_ops_total` unchanged, as `resources.rs:109` already does for
  another path); refill resumes after inventory insert and after capacity
  frees; a broker command's lock wait stays under a stated bound while a
  multi-page refill runs; WAL shrinks under the maintenance path with an
  active reader present and without one; existing `peer_queue_refill_is_bounded`
  and durable-broker tests unchanged.
- Measure with item 1 (idle profile) before and after; report logical reads
  per second, write-lock wait, and refill counters.
- Depends on: 2. Parallel: no.

### 4. Anonymous-memory attribution and bounds

- Owner: `rns-transport/src/transport/{announce_table.rs,path_table.rs,
  packet_cache.rs,reticulum_path_store.rs}`, `reticulumd` bootstrap config,
  `rns-rpc` SDK event log and reply queues as found.
- Measure first: run the item-1 fixture with the ingress profile and sample
  the item-2 population gauges alongside RSS; add an ignored allocation probe
  (heaptrack or the existing `issue_657_allocation_peaks` pattern) that reports
  bytes per population unit. Identify which structures account for the
  growth from about 196 MiB to about 399 MiB.
- Bound what grows without bound, matching Python Reticulum where it defines
  behavior (path expiry, announce table retry/timeout, packet hash list size)
  and documenting any Rust-only cap as configuration with a safe default.
  Expose each cap and its current occupancy in metrics. Do not silently evict
  routes that Python would keep.
- Tests: growth probe stays flat after the cap under repeated announces;
  restore after restart still recovers referenced routes; existing path/tunnel
  restore tests unchanged.
- Depends on: 3. Parallel: no.

### 5. Poll-failure stage correlation (issue acceptance bullet 6)

- Owner: `zmq_metrics.rs`, `metrics.rs`, `lxmf-sdk/src/backend/zmq_pipeline/failure_context.rs`
  and the SDK poll path.
- Daemon: export `ZmqStage` x `ZmqStageOutcome` counters and latency histograms,
  plus a bounded ring (for example 64 entries) of recent failed or timed-out
  requests with timestamp, stage, method name, queue depth, write-lock wait at
  the time, whether a refill page or announce persistence was in flight, and
  the cgroup `memory.events.high` counter. No identities or payloads.
- SDK: confirm `failure_context` classification reaches the SDK poll error
  returned to RCH; add a stable error-class field if missing so RCH can count
  by class. Coordinate the RCH consumer change in RCH #276's follow-up; do not
  change RCH here.
- Tests: injected handler stall produces a ring entry with the right stage;
  injected response-connect failure likewise; serialization size bounded.
- Depends on: 4. Parallel: no.

### 6. Production-shaped soak and acceptance record

- Owner: item-1 harness, `docs/performance/rch-resource-fixes-663.md`, roadmap
  and parity matrices.
- Prerequisite: #678 reference-aware cleanup merged, or the ingress profile
  runs with a bounded announce population so the disk does not fill. State
  which was used.
- Runs, each on baseline `9c8bc7b5` and on the item-5 head: at least four
  hours idle polling, at least four hours sustained ingress with periodic
  fresh inbound, outbound and fanout deliveries, plus one restart and one
  lost-ACK recovery during the ingress run. Two CPUs, `MemoryHigh=768M`,
  `MemoryMax=1G`, fixture DB about 1.1 GiB with a large WAL.
- Report: CPU, logical reads, syscalls, anon/file/swap, memory.events deltas,
  WAL size over time, refill and persistence counters, poll error count and
  stage classes, delivery receipts and checkpoint stability. State plainly
  what did not improve. Publish the daemon test prerelease for the operator
  only if the user asks.
- Acceptance is the issue's list: lower idle CPU and read amplification,
  bounded working set and WAL, no continuing swap or reclaim growth, fresh
  delivery with stable checkpoints, and every remaining poll failure tied to
  a stage record. Anything unmet stays listed as open in the performance doc.
- Depends on: 5 and the #678 decision. Parallel: no.

## Common validation

Per PR: focused tests first, then `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
the four-package suite from the performance doc
(`cargo test --locked -p reticulum-rs-rpc -p reticulum-rs-transport -p lxmf-sdk -p reticulumd --tests --no-fail-fast -- --test-threads=4`),
`tools/scripts/check-boundaries.sh`, `tools/scripts/check-module-size.sh`,
`cargo run -p xtask -- architecture-checks`, and
`cargo test -p reticulumd --test code_quality_issue_369` for diagnostics
changes. Release-facing changes also run `cargo xtask release-check`.
Record PR URLs, heads, commands, results and merge SHAs in a `PROGRESS.md`
next to this plan.
