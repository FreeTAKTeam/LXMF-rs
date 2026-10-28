# RCH resource regressions: 663, 657 and 655

The [issue 663 investigation](https://github.com/FreeTAKTeam/LXMF-rs/issues/663)
attributes measured daemon reads to propagation refill and announce-cache
persistence. The earlier [657](https://github.com/FreeTAKTeam/LXMF-rs/issues/657)
and [655](https://github.com/FreeTAKTeam/LXMF-rs/issues/655) reports concern private
memory/swap growth and ZeroMQ timeouts. File-cache pressure and retained private
memory are distinct measurements; reducing one path does not establish the
cause of every historical timeout.

This work starts from upstream `634e0fc7ebe112a258cabe65b92b71ea5ab28d06`.
That baseline already includes remaining-capacity enforcement, durable peer
inventory, SDK socket cancellation/reset, timeout-stage diagnostics and bounded
daemon dispatch. Those protections are retained rather than replaced with
longer timeouts, weaker acknowledgement or authorization checks.

## Propagation refill

The inventory has a newest-first `(received_at DESC, transient_id DESC)` index.
A persisted per-peer keyset cursor records scan progress and completed passes.
Each transaction examines at most 128 candidate identifiers, uses indexed
membership checks, and counts effective unhandled entries through a partial
index capped at configured capacity. The public operation releases the shared
writer between pages. A singleton inventory generation tracks mutations without
updating every peer's cursor for each message.

Completed/received entries keep precedence over pending entries, including
case-variant peer histories. Existing per-peer capacity semantics and global
maintenance policy are retained. Reopen, mostly-known inventories, newly
arriving inventory and unchanged completed passes have focused regressions.
The approach reuses SQLite transactions and the existing writer owner;
it adds no competing connection or propagation queue authority.

## Announce cache

Each Transport keeps fingerprints of packet bytes and the bound interface for
only the current route snapshot and one storage root. A repeated known record
skips MessagePack encoding and cache-file open/read/write. Hashing still walks
packet data. Initial and five-minute verification use bounded exact-byte reads
to repair missing or corrupt files. Failed writes do not mark records clean;
removed routes are pruned even if a later snapshot write fails. A semaphore
serializes concurrent saves without holding transport locks across filesystem
I/O. Restart performs a fresh verification.

The explicitly run 10,000-record Linux cache-file qualification reported four
read syscalls attributable to sampling `/proc/self/io`, no write syscalls and
zero physical read/write bytes in its repeated-cache phase. This is an
operation-specific observation, not a full daemon CPU or memory benchmark.
Transport still exports paths and writes `destination_table` and `tunnels`.

Reproduce the cache qualification with:

```sh
cargo test --locked -p reticulum-rs-transport --lib \
  profile_repeated_announce_cache_writes -- --ignored --nocapture
```

The deterministic Transport regression also removes an already fingerprinted
file, confirms the fast save does not touch it, then verifies that fresh
Transport instances repair missing and corrupt records and restore the path.
Interface/header changes, write errors, verification scheduling and concurrent
saves are covered independently.

## Complementary RCH work

[RCH issue 276](https://github.com/FreeTAKTeam/Reticulum-Community-Hub/issues/276)
owns repeated RCH connection setup and historical inbox/dispatch queries.
Schema 8 adds partial pending-only indexes; high-frequency lane owners retain
configured connections with purge and joined-shutdown ownership. A controlled
10,000-completed-row fixture reduced empty inbox/dispatch VM steps from
75,018/30,009 to 17/10, unchanged between 32 and 10,000 history rows. These
results are separate from the daemon cache qualification.

## Local validation, 2026-10-15

The RPC, transport, SDK and daemon package test suite passed 2,957 tests with
128 explicit ignores using four test threads. The initial default-concurrency
run passed the changed RPC/transport/daemon targets but ten SDK unit tests hit
the shared process-wide ZMTP connection quota. The complete four-thread rerun
passed without changing production quotas, timeouts or assertions. Focused
refill tests additionally cover inventory inserted between pages after the
invocation boundary was captured.

The complementary RCH workspace passed 697 tests with two existing ignores.
All eight paired durable-broker tests also passed with a disposable RCH source
copy resolving the patched local LXMF 0.14.0 workspace. That qualification leaves
the repository's immutable production dependency pins unchanged. It covers
lost ACK and both restarts, replay, outbound admission/receipt reconciliation,
worker pause/resume/join and actual purge with retained worker connections.

The four-package test command is:

```sh
cargo test --locked -p reticulum-rs-rpc -p reticulum-rs-transport \
  -p lxmf-sdk -p reticulumd --tests --no-fail-fast -- --test-threads=4
```

## Local harness and 10-minute smoke baseline, 2026-10-24

The issue 663 follow-through adds a repeatable, production-shaped harness
(`tools/scripts/resource_soak_663.py`, wrapped by
`cargo xtask resource-soak-663`). The fixture generator writes a
durable-broker `reticulum.db` with bulk propagation inventory, mixed-case peer
history and a deliberately grown WAL, plus a Python-compatible Reticulum
storage root with 100,000 signed cached announces under the interface hash the
daemon's first interface receives. The runner copies the fixture per run,
starts `reticulumd` under `systemd-run --user --scope -p MemoryHigh=768M
-p MemoryMax=1G` pinned to two CPUs, drives it with an RCH-shaped 100 ms ZeroMQ
event poller, and samples `smaps_rollup`, `/proc/<pid>/io`, cgroup v2 memory
and CPU files, WAL size and the daemon `/metrics` JSON every five seconds. The
ingress profile adds a second plain daemon announcing every second over TCP.

Reproduce:

```sh
cargo xtask resource-soak-663 -- fixture --root target/soak-663/fixture \
  --entries 200000 --announces 100000
cargo xtask resource-soak-663 -- run --root target/soak-663/fixture \
  --out target/soak-663/baseline-idle --profile idle --duration-secs 600
cargo xtask resource-soak-663 -- run --root target/soak-663/fixture \
  --out target/soak-663/baseline-ingress --profile ingress --duration-secs 600
```

Fixture: 200,000 propagation entries of 2 KiB (database 1,134 MiB, WAL
551 MiB), 300 peers with 600,000 history rows, 100,000 cached announces. Host:
Linux 7.0, 22 cores with the daemon pinned to two, rustc 1.96.0, release
profile, baseline daemon `main` at `8f872639`, ten minutes per run.

| Measure | Idle, first minute | Idle, last minute | Ingress, first minute | Ingress, last minute |
| --- | ---: | ---: | ---: | ---: |
| process RSS | 82.4 MiB | 84.0 MiB | 487.3 MiB | 547.1 MiB |
| process anonymous | 62.7 MiB | 64.1 MiB | 466.4 MiB | 526.3 MiB |
| process swap | 102.5 MiB | 137.6 MiB | 0 | 0 |
| cgroup memory.current | 195.3 MiB | 560.8 MiB | 495.2 MiB | 557.4 MiB |
| cgroup file cache | 124.8 MiB | 487.9 MiB | 24.8 MiB | 26.9 MiB |
| WAL file | 550.9 MiB | 550.9 MiB | 550.9 MiB | 550.9 MiB |

| Rate or delta over the run | Idle | Ingress |
| --- | ---: | ---: |
| time to `/readyz` (WAL replay plus 100,000 cached announces) | 404 s | 289 s |
| daemon CPU, percent of one core | 5.7 | 13.0 |
| logical reads (`rchar`) | 7.9 MiB/s | 8.0 MiB/s |
| read syscalls per second | 2,031 | 2,691 |
| `memory.events` high / max / oom | 1,906 / 0 / 0 | 0 / 0 / 0 |
| store write ops / read ops | 3 / 47,191 | 1,903 / 48,052 |
| store write-lock wait, total | 0.001 ms | 19,029 ms |
| SDK polls / errors | 5,996 / 0 | 5,993 / 0 |
| SDK poll p95 latency, median of windows / max | 819 us / 6.3 ms | 790 us / 27.9 ms |

Observations the baseline attributes or leaves open:

- The idle profile reproduces file-cache pressure: cache grows by about 360 MiB
  and `memory.events high` fires 1,906 times in ten minutes with three writes.
  The initial inference that this was announce-cache verification was not
  established by these counters. The instrumented 2026-10-26 comparison below
  instead identifies a five-minute propagation-retention pass, while announce
  persistence has zero runs.
- Idle store reads run at about 80 per second. Half are the durable dispatch
  pump in `rpc/daemon/sdk_outbound.rs`, which re-ran `broker_pending_dispatch`
  every 25 ms on an idle daemon; a probe through `handle_rpc` confirmed the SDK
  poll path itself performs no store reads. The remaining idle reads are not
  yet attributed.
- Under announce ingress, anonymous memory grows about 60 MiB in ten minutes
  (466 to 526 MiB) and writers wait 19 s in total across 1,903 writes while the
  refill runs; the daemon logged 410 "dropped path-table persistence trigger"
  warnings. These are the item 3 and item 4 targets.
- A SIGINT during startup rewrites `destination_table` empty, discarding the
  cached routes; the harness now works on a per-run fixture copy because of it.

## Operation-level metrics, 2026-10-24

The daemon metrics snapshot (`GET /metrics`) now carries `storage.refill`
(invocations, skips, pages, candidates examined, rows inserted, capacity stops,
wall and max page time, priority yields), `storage.wal` (checkpoint attempts and
outcomes, emergency reclaims, last WAL size), `memory` (process `smaps_rollup`
and the daemon's own cgroup v2 memory files, or an explicit unavailable
marker) and `providers.announce_persistence` / `providers.transport_tables`
(path-table save costs and in-memory table occupancy). The soak sampler records
all of them, so later runs attribute reads and memory per operation instead of
per process. Counters carry no payloads, identities or SQL values.

## Item 3 before/after smoke, 2026-10-24

Same harness and limits as above on a regenerated fixture (200,000 entries,
database 1,134 MiB, WAL 973 MiB, 100,000 cached announces), ten minutes per
run, daemon pinned to two CPUs. Baseline is `main` at `ac0eb429` (daemon code
identical to `8f872639`); item 3 is `corvo/issue-663-03-refill-wal`. These are
smoke comparisons, not acceptance.

| Measure | Baseline idle | Item 3 idle | Baseline ingress | Item 3 ingress |
| --- | ---: | ---: | ---: | ---: |
| time to `/readyz` | 307.396 s | 261.554 s | 239.733 s | 287.428 s |
| process RSS, first / last minute | 187.0 MiB / 223.0 MiB | 184.4 MiB / 218.5 MiB | 370.4 MiB / 433.9 MiB | 323.3 MiB / 417.0 MiB |
| cgroup anonymous, first / last minute | 167.8 MiB / 203.7 MiB | 165.2 MiB / 199.2 MiB | 350.2 MiB / 413.7 MiB | 303.3 MiB / 396.9 MiB |
| cgroup file cache, first / last minute | 42.5 MiB / 254.4 MiB | 323.1 MiB / 429.9 MiB | 38.2 MiB / 233.1 MiB | 188.6 MiB / 255.2 MiB |
| WAL file, first / last minute (peak) | 0.5 MiB / 212.3 MiB (212.3 MiB) | 0.4 MiB / 96.0 MiB (212.3 MiB) | 0.5 MiB / 212.5 MiB (212.5 MiB) | 0.4 MiB / 96.0 MiB (177.7 MiB) |
| `memory.events` high / max / oom | 0 / 0 / 0 | 276 / 0 / 0 | 205 / 0 / 0 | 464 / 0 / 0 |
| daemon CPU, percent of one core | 6.056 | 5.197 | 11.229 | 11.891 |
| read syscalls per second | 2084.712 | 2116.762 | 2709.508 | 2853.615 |
| store read ops / write ops | 46881 / 3 | 4763 / 12 | 48096 / 1723 | 5909 / 2292 |
| store write-lock wait, total | 0 ms | 0 ms | 6,907 ms | 8,937 ms |
| refill invocations / skipped by gate / pages | n/a | 0 / 0 / 0 | n/a | 537 / 536 / 8 |
| SDK poll p95 latency, median of windows / max | 1236.000 / 1371 | 1265.500 / 1901 | 742.000 / 3940 | 741.500 / 3527 |

What changed and what did not:

- Store reads on an idle daemon fell from about 78 to 8 per second: the durable
  dispatch pump now backs off to 250 ms while its queue is empty. Process read
  syscalls are unchanged at about 2,100 per second, so that churn is not the
  SQLite store; it is attributed to the file-side announce cache work (item 4).
- Both builds checkpoint the 973 MiB fixture WAL during startup. The
  five-minute maintenance pass then writes about 212 MiB of WAL in one
  transaction; the baseline keeps that file for the rest of the run (and, in
  production, grows it on every pass), while item 3's `journal_size_limit`
  truncates it to 96 MiB (`budget/64` for the 6 GiB fixture budget) at the next
  checkpoint. The timer's own PASSIVE/TRUNCATE pass did not need to fire.
- Under ingress, 536 of 537 refill invocations were turned away by the
  read-only capacity gate; the one that ran took eight 128-row pages. Writer
  wait per store write stayed at about 4 ms on both builds (6.9 s over 1,723
  writes against 8.9 s over 2,292), consistent with `synchronous=FULL` commit
  cost rather than refill occupancy. Item 3 therefore does not reduce the
  per-write cost; it removes the extra writes and the whole-inventory rescans.
- Anonymous memory grows the same amount on both builds (about 35 MiB idle,
  about 65 to 95 MiB under ingress, in ten minutes) and the five-minute pass
  still raises file cache by 150 to 300 MiB with `memory.events high` firing
  on the 768 MiB limit. Both remain item 4 targets. The first-minute file-cache
  difference between the builds (42 against 323 MiB idle) was not reproduced
  deliberately and is recorded as run-to-run variance to investigate with the
  item 2 counters, not as an effect of this change.
- The announce persistence counters from item 2 show the cost that item 4 must
  bound: under ingress, 228 path-table saves examined 22.8 million records in
  ten minutes (100,000 per save, 44 s total) and 677 further triggers were
  absorbed while a save was running.

## Announce retention and persistence allocation, 2026-10-26

The follow-through for [#678](https://github.com/FreeTAKTeam/LXMF-rs/issues/678)
reuses the existing Python-format announce cache. Successful path/tunnel saves
collect one bounded batch of obsolete cache files under the same save gate;
idle daemon maintenance continues the directory cursor every 30 seconds without
rewriting route snapshots. Each batch examines at most 512 directory entries.
Packets referenced by the last successful persisted active-route or tunnel
snapshot remain protected, as do files modified within the preceding 24 hours.
The grace period and incremental sweep bound obsolete history by age and scan
progress; this is not a fixed disk-byte budget or cross-process exclusion.
Cleanup errors and progress are visible in `providers.announce_persistence`.
Its reclaimed-byte counter reports logical file bytes, not filesystem block
allocation.
No announce-cache migration, new storage engine, protocol hash change, or route eviction policy
is introduced.

Persistence previously collected owned `Packet` clones for every route before
writing the snapshot. The cache now shares immutable packets through `Arc`;
response-context mutation uses copy-on-write, and existing packet-returning
getters retain their behavior. A deterministic 100,000-route probe with 256-byte
payloads avoids copying 25,600,000 payload bytes and 15,200,000 bytes of inline
packet storage into each snapshot, using 800,000 bytes of packet handles instead.
This attributes avoidable allocation churn, not all retained process memory.
Existing route/tunnel expiry, announce-cache capacity and packet-history rotation
remain the population policies; capacity zero retains its existing unlimited
meaning.

The new comparison also corrects the idle read attribution. In the untouched
`f7822310` idle run, announce persistence reports zero runs throughout. Logical
reads are nearly zero between samples, then increase by about 5.05 GB during
the five-minute propagation-maintenance pass. The fixture has 409,600,000 bytes
of propagation payloads against the daemon's default 256 MiB limit, plus
600,000 peer-history rows with old timestamps. Existing retention therefore
performs a large legitimate cleanup; the run is not a continuously idle,
unchanging database. Its roughly 8 MiB/s whole-run read average must not be
described as sustained empty-poll scanning or announce-file verification.

The retention metadata query now uses a covering ordered index, avoiding
lookups into the large payload table for each candidate. Expired/orphan peer-history
marks are selected outside the writer in rowid keyset pages of at most 512
identities and deleted in 512-row transactions. A captured upper rowid bounds
the pass; rows are rechecked for their identity and current expiry/orphan status
before deletion, candidate memory is released between pages, and maintenance
yields to priority broker writers. Per-peer and global quota
enforcement retains its original SQL in one atomic writer transaction. TTLs,
quota ordering, configured limits and priority weights are retained. Payload
retention still materializes its candidate metadata and quota enforcement can
still be a long writer operation; the entire retention algorithm is not bounded
in memory or writer duration.

A controlled query-only probe using Python SQLite 3.46.1 against the same
200,000-row fixture measured 1,960,681,407 logical read bytes / 477,234 read
syscalls before the covering index, versus 16,687,241 bytes / 4,076 syscalls
afterwards. Both scans returned 200,000 rows. A Rust regression independently
checks the upgraded query plan uses the covering index without a temporary
sort. These numbers describe that metadata query, not total daemon CPU or
memory usage.

The five affected Rust packages (`lxmf-propagation`, `reticulum-rs-rpc`,
`reticulum-rs-transport`, `lxmf-sdk`, `reticulumd`) passed 3,345 tests with
four test threads; 135 tests were ignored by that suite. The final keyset paging
change additionally passed 1,816 RPC and daemon tests (121 ignored), including
its new upper-bound regression. The explicit allocation
probe above passed separately, and all 13 Python harness tests passed. Workspace
Clippy (all targets/features, warnings denied), formatting, module sizes,
boundaries and architecture checks including the bare-metal core gate passed. Tests
cover cleanup-only snapshot preservation, current route/tunnel recovery,
cleanup failure/retry, bounded deletion pages, refreshed/reused row protection,
stage correlation and index migration.

The choice follows the standard library's
[`Arc` shared ownership](https://doc.rust-lang.org/std/sync/struct.Arc.html)
and Tokio's incremental
[`ReadDir`](https://docs.rs/tokio/latest/tokio/fs/struct.ReadDir.html).
It removes full packet copies and reuses the persistence owner without adding
a separate cache authority.

## Exploratory ten-minute comparison, 2026-10-26

Four 600-second local runs used the same untouched synthetic fixture and separate
copies for each daemon: 200k payload rows (1.1 GiB database), 600k old peer marks,
a 973 MiB WAL and 100k signed announces. Each daemon had two pinned CPUs,
768 MiB MemoryHigh and 1 GiB MemoryMax; the host and storage were shared.
Baseline was `f7822310`; the immutable candidate binary was `e4cba817`
(SHA256 `5a15747c9607d1a7356de77f6b1f34f756fe037f253289e8476ccf2680edae41`).
The candidate predates the final expiry-selection memory bound and quota-SQL
simplification. These are exploratory measurements of that candidate, not an
exact-final-head benchmark or a production acceptance result.

| Measurement | Baseline idle | Candidate idle | Baseline ingress | Candidate ingress |
| --- | ---: | ---: | ---: | ---: |
| Time to HTTP readiness (s) | 305.8 | 615.8 | 131.1 | 621.5 |
| CPU (% of one core) | 4.30 | 5.62 | 14.32 | 12.56 |
| Logical reads (MiB/s) | 8.08 | 3.49 | 8.49 | 3.57 |
| Read syscalls/s | 2074.15 | 1226.72 | 2831.13 | 1569.94 |
| RSS first → last minute (MiB) | 95.0 → 74.7 | 234.3 → 281.3 | 265.2 → 314.9 | 375.9 → 390.9 |
| Process swap first → last minute (MiB) | 83.9 → 137.2 | 8.0 → 28.5 | 22.5 → 50.7 | 0.0 → 20.3 |
| Cgroup RAM first → last minute (MiB) | 100.5 → 548.6 | 646.9 → 749.2 | 424.9 → 660.6 | 645.3 → 735.0 |
| Cgroup swap first → last minute (MiB) | 94.4 → 148.2 | 8.0 → 46.7 | 77.4 → 176.1 | 0.0 → 96.3 |
| MemoryHigh events added | 1834 | 301 | 4144 | 1767 |
| Writer wait added (ms) | 0.00 | 0.08 | 19022.74 | 6526.10 |
| SDK polls / errors | 5995 / 0 | 5992 / 0 | 5870 / 1 | 5984 / 0 |
| Worst window poll p95 (µs) | 4452 | 1524 | 898240 | 32969 |

The candidate reduces maintenance reads and ingress writer wait, and these
short runs observed no SDK poll failures. Idle CPU and resident memory are
higher, reclaim and swap still grow, and readiness is slower. The five-minute
retention work still has costs beyond the covering metadata query. The index is
created synchronously on first upgrade and adds about 21 MiB to this database;
no migration timing isolates its contribution to startup latency.

The original harness also let private Unix RPC staging paths exceed Linux's
socket-name limit in both baseline runs and candidate ingress. HTTP and ZeroMQ
continued, but these runs do not establish complete listener health. The harness
now binds a short relative socket from each run root. A separate 30-second
empty-fixture ingress smoke verified both private Unix sockets and no socket
binding panic; it does not replace the large-fixture comparison.

Raw local artifacts are under `target/soak-678-663/{baseline-idle-retry,head-idle,
baseline-ingress,head-ingress}` (each contains `run.json`, `samples.jsonl`,
`client.jsonl`, `summary.json` and `report.md`). The tracked record preserves
these limitations instead of treating a short poll run as durable delivery.

## Acceptance boundary

Local SQL plans, regressions and socket tests do not establish a multi-hour
memory/swap plateau or fresh delivery on the production droplet. Remaining
acceptance uses its production-shaped database/WAL and announce inventory,
two cores, 768 MiB MemoryHigh and 1 GiB MemoryMax; measures CPU, logical reads,
anon/file/cache, swap and reclaim deltas; and verifies durable inbound,
outbound/fanout, restart and lost-ACK recovery. Hardware, public-network and
production deployment evidence are not claimed by this increment.

## Preview.26 follow-up: timer-driven persistence

The [regression package](../goals/preview26-regression-663-657-678/PLAN.md)
replaces the two-second announce debounce with dirty saves no more frequently
than 300 seconds (`LXMD_PATH_TABLE_SAVE_INTERVAL_SECS`, positive seconds).
The delayed 30-second maintenance tick only continues bounded cache collection;
it cannot write a route snapshot. Shutdown still flushes explicitly.
Canonical validated cache records seed fingerprints during restore, removing
the first-save reread; periodic full cache verification is removed under the
single-owner assumption. Missing/invalid records on restart remain classified.
Focused regressions cover burst timing, cleanup without snapshots and seeded
restore. Production acceptance and paired ingress measurements remain pending.

Preview.26 control follow-up: `/readyz` returns 503 until every configured ZMQ
listener has bound, and clears on loop exit. Durable capability reads use a
startup/enable cache. Replay-safe broker RPC commands return a typed timeout
after 2.5 seconds while queued work retains its commit semantics. Eight legal
maximum control request/reply pairs and 32 bounded rejection replies have
reserved admission capacity; power-of-two drop warnings bound log volume.
Larger configured poll batches preclude a fixed 1 MiB reply reservation.
Local readiness/deadline regressions passed; production p99 and pressure
acceptance remain pending.

Preview.26 WAL follow-up runs scheduled PASSIVE/TRUNCATE on a dedicated
maintenance connection outside the traffic writer mutex. Emergency write-path
checkpoint attempts are limited to the shared maintenance interval, so recovery
from reader-pinned physical backpressure can wait until that next pass.
`storage.wal.last_checkpoint` exposes SQLite busy/log/checkpointed frames.
Regressions retain physical budget and custody/ACK guarantees and demonstrate
checkpoint progress while both traffic connection mutexes are held.
Production reader attribution and resource acceptance remain open.

### 2026-10-26 preview.26 paired ingress and control evidence

Daemon images were static `x86_64-unknown-linux-musl`, default daemon features.
The published preview.26 image was `392fc1b9`; PR 1 was `cd7f616d` on
`883e01b1`. Both ran against independent copies of the same 200k-entry,
1.11 GiB database / 973 MiB fixture WAL and 100k signed routes, with
MemoryHigh=768 MiB, MemoryMax=1 GiB, CPUs 2/3, and 600 seconds of ingress.
Local evidence: `target/soak/preview26/{baseline-ingress,pr1-musl-ingress}`;
`source-bindings.json` binds image hashes to commits. The sampler's
`git_revision` identifies the active workspace, so it is not the image identity.
The stopped glibc startup is excluded.

| Sampled ingress metric | Preview.26 | PR 1 |
| --- | ---: | ---: |
| Path persistence runs | 210 | 1 |
| Full announce-cache verifications | 2 | 0 |
| Unchanged records skipped on the candidate's one save | — | 100,000 |
| CPU, percent of one core | 14.330 | 7.831 |
| MemoryHigh event delta after readiness | 2,013 | 205 |
| Poll errors / polls | 0 / 5,990 | 0 / 5,974 |
| Poll window p95 maximum, microseconds | 12,303 | 1,838 |
| Logical read rate, MiB/s | 3.936 | 15.045 |
| Read syscalls/second | 1,665 | 3,857 |
| Aggregate writer wait delta, seconds | 5.776 | 14.468 |
| Readiness startup, seconds | 209.310 | 259.095 |
| Process swap, first/last minute medians, MiB | 0 / 21.2 | 248.1 / 74.2 |

The snapshot amplification is removed, but resource acceptance **does not pass**.
Around the 300-second retention pass the candidate's anonymous memory rose
from about 24 MiB to 236 MiB, file cache from 23 to 521 MiB, and high events
increased by 205; its observed tail then plateaued near 224 MiB anon / 522 MiB
file. Startup swap was substantially higher and startup slower. The runs do not
control host cache residency; these observed startup differences are not an
allocator attribution. Logical reads and aggregate writer wait worsened.
The candidate still pruned the fixture from 200k entries to 125k / 256 MB,
which implicates retention work, while persistence did no full verification.
The 14.468-second counter is aggregate wait across many operations, not a
measurement of one writer hold or a particular retention phase.

PR 4 remains deferred: one retention high-water step does not prove repeated
allocator ratcheting, and cumulative wait does not establish the required
multi-second quota/payload critical section. Per-phase/max-hold attribution or
repeated-cycle allocation evidence is needed before changing those algorithms.
No allocator, quota ordering, payload-prune policy or RCH timeout was changed.

The combined daemon additionally passed a 90-second local control stress with
100k restored routes, 2-second dirty-save and WAL timers, a bounded synthetic
WAL workload, and a reader pinned then released. Propagation retention was
disabled to isolate the save/checkpoint control boundary. There were 8,166
broker negotiations, no errors, p99 1,087 microseconds and maximum 151,151
microseconds, with 41 path saves and 23 checkpoint attempts. Evidence lives in
`target/soak/preview26/control-stress-pinned`. An earlier probe had zero
checkpoints and is excluded from proving checkpoint concurrency.
A real occupied command endpoint kept `/readyz` at 503 while `/healthz` and
`/livez` stayed 200 (`target/soak/preview26/failed-bind`).

The standard four-package suite passed 3,329 tests (135 ignores), with the new
SDK recovery and timeout-category regressions passing separately. A matching
Fernet optional-feature profile passed all 687 daemon tests; the initial
selected-package `--all-features` run enabled AES-128 transport with the other
Fernet implementation still on its default, so that inconsistent mode failed
propagation decryption. Final all-feature Clippy, formatting, boundaries,
module size, architecture (including no_std), and 13 Python harness tests passed.
`cargo xtask release-check` passed its runtime/check lanes through the publish
dry-run, then failed packaged daemon verification against already published
0.15.0 dependencies, which lack both #694 and current APIs. No crates are
published or version-bumped by this daemon-only prerelease workflow.

Preview.26 operator metrics, production reader attribution, fresh delivery,
restart/lost-ACK recovery and multi-hour #663/#657 acceptance remain open.
#678 cache file/inode retention over several GC cycles remains a separate
acceptance lane; these resource/control runs do not close it.

### Operator preview.26 metrics supplied on 2026-10-26

The operator supplied the requested pre-redeployment excerpt in this chat,
identified as preview.26. Preserve the exact counters in
[the operator excerpt](../goals/preview26-regression-663-657-678/evidence/operator-preview26-2026-10-26.json).
This is user-supplied production evidence, not a direct scrape by this agent;
uptime, current broker budget, and a second sample were not supplied.

- Persistence completed 2,177 runs after 8,941 triggers and examined 67,815,223
  records, with 28 full verifications. Cumulative reported path-save wall time
  is 2,538.980 seconds (42.316 minutes), averaging 1.166 seconds per run.
  This supports the production save-amplification diagnosis. The duration is
  accumulated wall time for path saves (including their cleanup), not CPU
  utilization or process uptime; packet-hashlist persistence is outside it.
- 65,110,554 records were skipped by known fingerprint, 845,700 verified
  unchanged and 1,858,969 written, with zero save errors. The exact split sums
  to the examined count. Even with about 96% skipping cache-file work, every
  save still walks/encodes the route snapshot. The most recent save took
  999.827 ms; the last snapshot had 28,677 active routes and 3,045 tunnel paths.
- 4,958 dropped persistence triggers and 1,805 coalesced triggers demonstrate
  scheduling/coalescing pressure. These counters concern redundant save
  notifications; they are not counts of lost network announces.
- Cache collection removed 1,046,838 files / 268,812,387 bytes (256.359 MiB),
  completed two scans and reported zero cleanup errors. This demonstrates
  active #678 collection, but without file/inode population trends it does
  not establish bounded retention or complete that separate acceptance lane.
- The reported WAL is **exactly 100,663,296 bytes (96 MiB)**, with zero
  recorded checkpoint attempts, busy/incomplete outcomes or errors. If the
  retained broker budget is still the plan's 6 GiB, its routine threshold is
  `max(64 MiB, budget/64) = 96 MiB`. In deployed `392fc1b9`, `maintain` returns
  `BelowThreshold` when `size_before <= threshold`; that outcome updates size
  but does not increment `checkpoint_attempts_total`. Thus equality is
  consistent with all these counters remaining zero. These counters also
  omit SQLite's automatic checkpoints. This sample does not establish a
  pinned reader, a stalled maintenance timer, or a currently 546 MiB WAL.

This excerpt supplies the plan's requested production baseline and supports
PR 1's existing direction. At the time it was not read as requiring a threshold
change; the preview.29 collection below shows equality is the permanent steady
state, which that reading missed. PR 3's dedicated
connection and frame diagnostics remain useful for observing eligible
checkpoints; no deployment or operational acceptance is inferred here.

## Preview.29 follow-up: WAL pinned at the maintenance threshold, 2026-10-27

The operator's four-hourly collector (preview.29, LXMF-rs `6782e899`) recorded
`storage.wal.last_wal_bytes` fixed at exactly 100,663,296 bytes (96 MiB) and
`checkpoint_attempts_total` at zero across 13 h 47 min of uptime, and asked
whether the maintenance worker was stalled or the metric stale.

Neither. With the production 6 GiB broker budget, `truncate_limit_bytes` is
`max(64 MiB, 6 GiB / 64) = 96 MiB`, and the same value is applied as SQLite's
`journal_size_limit`. SQLite trims the WAL to exactly that limit when it resets
the log after an automatic checkpoint, so the file settles at 96 MiB. Routine
`maintain` then compared `size_before <= threshold` and returned
`BelowThreshold` every minute: the timer ran and refreshed the gauge, but the
checkpoint/truncate branch was unreachable in steady state. The file-cache
footprint therefore carried a permanent 96 MiB WAL whose content SQLite had
already checkpointed.

Change: `maintain` runs the PASSIVE/TRUNCATE pass once the file *reaches* the
threshold (`>=`), so the trimmed-to-limit state is eligible and the file
returns to near zero on the next quiet pass. A new `storage.wal.runs_total`
counter advances on every pass, including below-threshold ones, so a live timer
is distinguishable from a stalled one when nothing is due. Regressions:
`wal_pinned_at_journal_size_limit_is_still_truncated` and
`below_threshold_runs_are_counted_without_checkpoint_attempts`.

Dropped persistence triggers (241 of 47,081 in the same collection) are
redundant save notifications: the trigger channel has capacity one, and
`announce_worker` counts a send as dropped only when a trigger is already
queued, which the worker turns into the next dirty save. No dirty route state
is lost; `dropped_persistence_trigger_still_reaches_a_dirty_save` pins that
guarantee. The counter is kept for scheduling-pressure visibility.

Not covered here: anonymous-memory growth attribution, polling-error
classification and multi-hour production acceptance remain open per the issue.

## P0 item 1 — destination cache lifetime (2026-10-28)

The approved [P0 plan](../goals/p0-2026-10-28-663-608-609-625/PLAN.md)
is implemented at the existing transport and daemon owners. The ten-second
interface cleanup prunes remote recipient identities after route removal, with
exceptions for local destinations, relay/endpoint links and pending SINGLE
receipts. The announce worker prunes its peer identity cache on the same cadence;
a busy transport retains entries until a later pass. Metrics scrapes remain
nonblocking and expose `single_out_destinations`, `random_blob_count`,
`peer_crypto` (null when busy), and both removal totals under `transport_tables`.
`transport_tables.fingerprint_map_len` reads the existing persistence state
without blocking, including restored fingerprints before the first save. It is
null while persistence owns that state for I/O. The existing
`announce_persistence.last_fingerprint_map_len` remains the last-save gauge;
no second cache or truth path was introduced.

Source correction to the plan: cloning 64 random blobs and pushing one retained
capacity 128, rather than doubling without limit across refreshes. The shared
bounded-blob helper now shrinks capacity for announce and restore paths. A
regression failed at 128 versus 64 before the fix and passed afterwards.
Hashlist persistence encodes directly from current-generation membership under
the existing lock, releases it before disk I/O, and preserves Python's raw format.

Validation: transport library suite 1,068 passed / 13 intentionally ignored;
peer-cache provider regression passed. Paper encoding also recovers persisted recipient keys offline after live cache expiry;
the real URI encode/decrypt regression covers cached and persisted-only keys.
The cleanup regression preserves a live
route, local destination, endpoint link and pending no-path receipt, then releases
the recipient after receipt expiry. The production-shaped ingress before/after
measurement is recorded separately; no multi-hour or production acceptance is
claimed from the software tests.

## P0 item 2 — systemd readiness (2026-10-28)

The daemon now sends `READY=1` through optional `NOTIFY_SOCKET` after every
configured RPC listener binds and existing `RpcDaemon::is_ready()` confirms
ZeroMQ binds. Both filesystem and Linux abstract datagram sockets use the
standard library. Shutdown cancels a pending readiness task and sends
`STOPPING=1`; SIGTERM now follows the same graceful path as SIGINT.

Both daemon build paths share this integration. The direct daemon runbooks use
`Type=notify`, `NotifyAccess=main`, an explicit restoration startup timeout, and
RCH `After`/`Requires` ordering. The combined `lxmd` launcher intentionally stays
`Type=simple`: its child cannot notify under `NotifyAccess=main`.

Socket regressions cover delayed RPC/ZMQ readiness, STOPPING, owner cancellation,
listener disappearance, retry after a full socket queue, and Linux abstract addressing. A release-binary process check passed with and
without ZeroMQ: READY received, `/readyz` returned 200, SIGTERM emitted STOPPING
and exited 0. A real local user systemd `Type=notify` / `NotifyAccess=main`
service reached active/running with `Result=success`; stopping it through
systemd's SIGTERM reached inactive with `ExecMainStatus=0`. Logs are under
`target/issue663/notify-process.json` and `notify-systemd.log`. No RCH production
deployment was performed.

## P0 item 3 — maintenance attribution and paging gate (2026-10-28)

`storage.maintenance.{expiry,quota,payload}` on `/metrics` now reports run/error
totals, last/total/max duration in ns, successful-phase deleted-row totals, last rows
deleted (null on error), last/total/max physical read-byte deltas and available-read
sample totals. The counters surround the existing storage owners and preserve
errors. An unavailable `/proc/self/io` read is null, rather than zero. Read deltas
include all concurrent process work and exclude logical cached reads; durations
include writer wait and existing WAL/admission work.

The SQL predicates, deletion order, custody semantics and writer ownership are
unchanged by instrumentation. On a phase error some earlier expiry pages may have
committed; its last-row count is unknown/null and the total counts only completed
successful phases. Tests exercise each real phase and preserve an original error.
Paging is conditional on a quota/payload duration above 1 s or physical reads
above 200 MiB in the controlled ingress run. The decision and measurements follow.

Payload pruning uses a shared operation owner, so its totals/maxima also include
inline ingestion/import and policy changes. Lifetime maxima retain slow calls
that later fast calls would hide from the last-run gauges; the paging gate uses
these maxima. The ingress profile sends announces, rather than payload traffic,
so the recorded payload operations in that profile are scheduled maintenance.

### Measured paging decision

The 600 s instrumented run at `a182a4ea` on the paired fixture measured expiry
9.92 s / 224,223,232 physical read bytes / 600,000 deleted history rows; quota
1.74 ms / 0 bytes / 0 rows; payload 8.08 s / 397,365,248 bytes / 75,000 rows.
Payload exceeded both gates. It now sorts at most 4,096 oldest candidates per
transaction, commits/releases the writer and yields between windows. Quota stays
atomic because its measured cost did not justify changing concurrent-write
semantics. The [migration note](../migrations/propagation-retention-paging-663.md)
records age ordering across windows and partial-page failure semantics. The
low-level library keeps its caller-owned transaction; only MessagesStore releases
its writer between pages.

A regression first failed with global ordering (removed `id-04097`) and passes
with the approved age-window rule (`id-00001`). Storage tests verify 5,000-row
paging and rollback of a failing second page while the committed first page stays
removed. The original storage error and unknown final count remain observable.

The attribution-only run is not a successful memory fix: anon medians rose from
170.75 to 255.34 MiB, with 476 MemoryHigh events and zero OOM/max events. Table
populations stayed essentially flat. The earlier baseline rose 170.73 to
220.52 MiB with 226 high events; initial file-cache ownership differed, so these
runs do not establish a comparative memory improvement. The plan's conditional
allocator follow-up is activated after this evidence. Counts plus conservative
compiler-layout accounting explain about 77% before the pass, below the 80% gate;
that gate remains open rather than crediting unmeasured allocator/SQLite slack.

### Conditional allocator evaluation

The daemon-only candidate uses mimalloc 0.1.52 (libmimalloc-sys 0.1.49) as the
Rust global allocator, following the conditional PR 4 in the earlier plan. The
measured step is retained anonymous memory with flat table populations, not yet
proof of repeated ratcheting. This is an experiment pending a same-fixture,
same-limit static-musl control/candidate comparison. Native C/SQLite malloc is
not overridden. The dependency adds no core or workspace-local edge; unrelated
Cargo.lock resolution changes were removed and `cargo check --locked` passed.
Audit found no vulnerabilities, with the existing unmaintained serial 0.4.0
warning; cargo deny licenses/bans/sources passed but advisories rejected that
same pre-existing serial advisory (RUSTSEC-2017-0008). No allowlist was weakened.
