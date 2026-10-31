# Announce history retention and query costs (#717)

This change is based on `e7908ccb`, which includes the propagation quota and
refill corrections from #714 and #715. It does not deploy to the RCH droplet,
change memory limits, or attribute the October 31 production temporary-file
trace to a query that was not observed in that trace.

## Retention contract

`reticulumd --announce-history-max-rows N` retains the newest **100,000** events
by default, ordered by `(timestamp DESC, id DESC)`. In addition, it always retains
each exact peer key's latest event and its latest event with a non-null stamp
cost. These may be the same row or may already be in the recent window. The
converged bound is therefore **N + at most two rows per distinct peer**. `N=0`
keeps only those operational records. There is no age limit. Distinct peer growth
is deliberately not bounded by this policy; identity eviction needs its own
trust/lifecycle policy. Existing case-sensitive stamp lookup semantics remain.

The latest full event retains names, capabilities, radio measurements and other
metadata. A newer event with no stamp cost does not erase a previously known
cost. Identity keys, propagation inventory/marks, tickets, message custody and
receipts are not deleted. The library storage API only prunes when explicitly
called; the daemon schedules it regardless of whether propagation is enabled.

A daemon worker waits one second after each completed batch, then examines at
most **512** old events. Selection, latest-record rechecks, deletion and the
persisted timestamp/ID cursor commit together. Each transaction releases the
writer before another batch starts. Protected records advance the cursor rather
than starving older events. A failed batch rolls back both rows and progress.
Restart resumes from committed progress. A later sweep revisits out-of-order
arrivals and formerly protected records. Raising the configured row limit takes
effect on the next batch, without deleting additional newly protected history.

This is an eventual bound, not admission control. With a fixed peer population,
the deletion rate must exceed ingress for convergence; sustained ingress above
the service rate can grow a backlog. At one second per batch, removing 1.6 million
obsolete events takes at least about 52 minutes, plus transaction time and any
protected rows examined. Do not interpret a short soak as convergence evidence.

Selection uses the existing timestamp/ID index. The start of a sweep walks up to N
index entries to capture its recent-window boundary; resumed batches seek directly
from the persisted cursor. Changing N starts a new sweep. Events arriving during a
sweep are considered again in the next sweep. The end of a sweep counts retained
rows. Large configured N therefore trades longer history for more index reads. Latest-per-peer and non-null
stamp lookups use separate peer/time/ID indexes so absent costs do not scan a
peer's entire history. These indexes build once during the schema transaction
on existing databases; the initial startup can take longer and temporarily use
SQLite sort files. Subsequent starts reuse them.

`list_announces` and its cursor format are unchanged, but older disposable
history disappears. Cursors identify an ordering position, not a guaranteed
existing row: pagination continues below that position over retained rows.
Clients needing a full audit archive must export events before retention. Manual
`clear_announces` still clears all history and identity keys, and now resets the
retention cursor atomically.

Deletion makes database pages reusable; it does **not** immediately shrink the
SQLite file or prove reduced resident memory. Routine retention never runs a
full `VACUUM` and does not change `temp_store`, allocator or pager settings.

## Diagnostics

`/metrics` JSON exposes `storage.announces`:

- `history`, `stamp_cost`, `retention_window`, `retention_candidates`,
  `retention_delete`, `retention_count`: fixed
  statement families with call/error counts, total/max wall nanoseconds, VM
  steps, full-scan steps, sort operations and automatic-index rows. No SQL
  parameters, peer IDs or payloads are retained. Counters include attempted work
  even if a transaction subsequently rolls back.
- `maintenance`: committed rows pruned, duration and the existing logical/physical
  read counters. Those I/O counters are thread/process observations, not exact
  per-statement temporary-file byte attribution.
- `examined_total`: candidates examined in committed batches.
- `retained_at_last_sweep`: exact row count when the most recent full sweep
  finished; zero before the first sweep. It is not a live count during ingestion.

Sort counts do not imply disk spills. Attribute temporary bytes separately with
OS tracing, including filenames and the statement window. The probe prints
`BEGIN/END` family markers to stderr for this purpose. Compare startup migration
and recurring maintenance separately.

## Reproduction and evidence

The assert-based probe uses the repository's bundled SQLite, the real schema and
real storage retention API, 1.7 million synthetic events across 300 peers,
nullable stamp costs, timestamp ties, and 128-byte app data (hex encoded). It
refuses an existing database path. It compares the original queries without the
new indexes to indexed lookups, then drives retention with six new events per
batch, checking the bound and all original peers' latest records and stamp costs.

```sh
cargo build --release -p reticulum-rs-rpc --example announce_history_717
mkdir -p target/issue717
strace -f -yy -s 80 -e trace=pwrite64,write,openat,close \
  -o target/issue717/baseline.trace \
  target/release/examples/announce_history_717 target/issue717/baseline.db baseline \
  > target/issue717/baseline.jsonl 2> target/issue717/baseline.stderr
strace -f -yy -s 80 -e trace=pwrite64,write,openat,close \
  -o target/issue717/candidate.trace \
  target/release/examples/announce_history_717 target/issue717/candidate.db candidate \
  > target/issue717/candidate.jsonl 2> target/issue717/candidate.stderr
```

The original production trace's producing statements remain unconfirmed; the
production snapshot replay below identifies a recurring status-listing spill. The
stamp-cost query is a proven scan/sort problem, but the bounded `LIMIT 1` sort
must not be blamed for the production 70.9 MiB temporary writes without a matching
execution trace. The snapshot replay below supplies statement-family attribution and representative
post-fix checks; it does not identify individual calls in the historical trace.

Design: indexed, paged SQL in the existing storage owner was chosen over a second
current-peer table. It preserves operational records without a second projection
or a full backfill, at the cost of two indexes and an explicit latest-peer
exception to the history bound. SQLite explains why transient sorting can stay
in memory or spill in [Temporary Files Used By SQLite](https://sqlite.org/tempfiles.html).
The counters use rusqlite's safe
[statement status API](https://docs.rs/rusqlite/0.40.2/rusqlite/enum.StatementStatus.html).

### Local results, 2026-10-31

[Machine-readable evidence](announce-history-717-evidence.json) records query
plans, counters, aggregate cgroup/process samples and binary hashes. Raw local
traces and samples are untracked under `target/issue717/`.

| Query on 1.7 million events | Baseline VM steps / scan steps / sorts | Indexed VM steps / scan steps / sorts |
| --- | --- | --- |
| Missing peer stamp | 5,100,010 / 1,699,999 / 1 | 11 / 0 / 0 |
| Present peer stamp | 5,119,444 / 1,699,999 / 1 | 14 / 0 / 0 |
| Deep history page, 100 rows | 2,493 / 0 / 0 | 2,493 / 0 / 0 |

The stamp plan changes from `SCAN announces; USE TEMP B-TREE FOR ORDER BY` to
`SEARCH announces USING COVERING INDEX idx_announces_peer_stamp (peer=?)`.
Pagination SQL was left unchanged because it already used the history index.
Do not compare the trace-instrumented baseline wall times to untraced final
wall times as a latency benchmark.

The final accelerated probe deleted **1,618,972** events while inserting
**18,972**, leaving exactly **100,000**. It verified every original peer's latest
event and stamp cost. There were 3,167 batches, at most 512 candidates per batch,
no retention sorts/automatic indexes, and only six window-boundary queries.
The untraced convergence phase took 48.69 seconds; maintenance used 38.32 seconds
in total, with a maximum measured batch duration of 147.89 ms on this host.
This probe calls batches without the daemon's one-second delay. The actual
scheduled catch-up time is much longer, as described above.

The first candidate's syscall trace recorded **209,803,283** temporary-file
`pwrite64` bytes during index creation, and **zero** in the stamp, history or
retention windows. Recurring retention still writes real WAL/database pages:
that trace recorded about 6.80 GB of WAL writes during catch-up. This is useful
cleanup work, not eliminated I/O. The final change avoids rereading the window
boundary every batch; it uses the same indexed delete statements. Its untraced
maintenance logical-read total was 6.47 GB, versus 38.31 GB in the first traced
candidate. These are different instrumentation conditions; the structural
regression test checks one boundary query across successive resumed batches.

Two 150-second daemon runs used the same synthetic fixture (1.7 million SQL
announces, 100,000 propagation payloads of 512 bytes, 600,000 peer marks, 300
peers, no packet-cache files), the existing `resource_soak_663.py` ingress profile,
`MemoryHigh=768M`, `MemoryMax=1G`, CPUs 0–1 and 30-second propagation maintenance.
Each captured four propagation cycles and 1,500 error-free SDK polls. Baseline
and final runs were separate; other probe/build activity ran on the host and
page cache was not flushed, so these are resource smoke observations, not a
controlled production performance benchmark. Both run manifests name the base
commit because changes were uncommitted at capture; the evidence includes the
actual baseline and final binary hashes.

| First → last sample, unless marked delta | Baseline | Candidate |
| --- | --- | --- |
| cgroup file bytes | 1,716,224 → 46,096,384 | 753,819,648 → 642,809,856 |
| cgroup anonymous bytes | 43,425,792 → 51,183,616 | 43,786,240 → 59,170,816 |
| tracked live Rust allocation bytes | 1,600,926 → 2,184,534 | 1,616,848 → 2,192,897 |
| CPU ticks delta over ~145 sampled seconds | 388 | 558 |
| logical read bytes delta | 312,843,101 | 1,855,398,737 |
| logical write bytes delta | 4,037,808 | 566,882,544 |
| physical read bytes delta | 45,608,960 | 4,657,152 |
| physical write bytes delta | 8,830,976 | 654,753,792 |
| memory.high / max / oom delta | 0 / 0 / 0 | **13 / 0 / 0** |
| memory PSI some/full total delta (microseconds) | 0 / 0 | 292 / 292 |
| DB bytes | 1,237,348,352 → 1,237,393,408 | 1,353,228,288 → 1,353,228,288 |
| WAL bytes | 527,392 → 527,392 | 100,663,296 → 2,768,672 |
| maximum window SDK poll p95 | 807 μs | 1,477 μs |

The candidate performed 142 retention batches and removed 72,704 old events
in that sampled window. Its startup index build and ongoing catch-up add I/O
and file-cache pressure. **The memory-pressure acceptance gate did not pass**:
13 new high events were observed. No OOM or swap growth occurred. This does not
establish RAM savings, a leak, production readiness, or convergence at the real
timer cadence. Fresh message delivery was not driven during these soaks.
Native Link-send and receipt-owner tests passed separately.

Initial validation: 882 RPC library tests passed (9 pre-existing ignored), 31 daemon
library tests, 697 daemon binary tests, the issue-369 scanner, native Link-send
and two receipt-owner tests passed. Formatting, strict Clippy for the affected
packages/all targets/all features, dependency boundaries and module-size checks
passed. No production service, data, configuration or memory limit was changed.


### Production snapshot replay, 2026-10-31

A user-supplied standalone SQLite online backup (captured at 16:01:48 UTC)
contains 1,730,975 announces, 88,132 distinct announcing peers, 274,447 propagation
entries and 221,676 propagation peer marks. Its SHA-256 is
`ede6b7ae3228231e3606a1dbcd94cf69ba6fdd40956ed9267fe3de91032245e7`;
SQLite quick-check passed. The unredacted database and raw local artifacts remain
untracked. Only aggregate results are published. All migrations and retention
ran on copies, with the original snapshot unchanged.

The SQL replay uses bundled SQLite 3.53.2, default `cache_size=-2000` and
`temp_store=0`. Schema initialization includes #714/#715. The baseline removes
only the two new announce indexes and executes the original queries; the
candidate uses those indexes and the handled-ID rewrite. The baseline's local
setup builds then drops the announce indexes, so its migration cost is **not**
a baseline startup measurement. Recurring query windows are marked separately.
`strace -f -ttt -yy -s 0 -e trace=pwrite64` records successful bytes and filenames
without database buffer contents. Timestamped BEGIN/END markers identify the
statement family. Peer IDs and query results are never printed.

One pass over all 232 case-folded propagation peers reproduces a temporary-file
producer in `list_peer_handled_propagation_ids`. The old query reduces state
precedence in a grouped subquery, joins inventory, then sorts the resulting IDs.
`list_peers`, peer status/sync reporting and local unpeer cleanup call this
shared method. Since every completed state wins over unhandled/unknown states,
the same result is a distinct stream of completed IDs with an inventory existence
check. The existing `(LOWER(peer), transient_id, state)` index supplies ID order
and duplicate elimination; no new propagation index is needed.

| Snapshot query pass | Before | After |
| --- | --- | --- |
| Handled-ID results | 196,801 | 196,801, identical ordered results for every peer |
| Handled-ID VM steps | 16,339,344 | 6,147,320 |
| Handled-ID sorts | 232 | 0 |
| Handled-ID temporary writes | 4,154,744 bytes / 2 files | 0 bytes / 0 files |
| Stamp lookup VM steps, same 232 peers | 1,205,030,098 | 2,702 |
| Stamp full-scan steps | 401,585,968 | 0 |
| Stamp sorts / temporary bytes | 232 / 0 | 0 / 0 |

The old handled-ID plan ends with `USE TEMP B-TREE FOR ORDER BY`. The new plan is
`SEARCH p USING COVERING INDEX idx_propagation_peer_entries_folded_lookup
(<expr>=?); SEARCH e EXISTS USING COVERING INDEX
sqlite_autoindex_propagation_entries_1 (transient_id=?)`. A regression test covers
all four completed states, case-variant duplicates, unknown states, orphan marks,
missing peers, ascending order and zero sort/full-scan/automatic-index counters.
`/metrics` exposes this family at `storage.propagation_queries.handled_ids`, with
the same bounded counters as announce queries and no identifier labels.

The otherwise-unused `list_peer_prospective_unhandled_propagation` API spills
2,916,352 bytes when deliberately asked to enumerate the full prospective
inventory for the largest peer. No daemon caller exists in this tree; this
exploratory API was left unchanged rather than expanding the production fix.
The other exercised peer existence, stats, unhandled payload and unhandled-ID
queries produced no temporary writes. Schema/index creation itself wrote
427,337,365 temporary bytes and remains an explicit one-time migration cost.
These observations identify a real recurring producer on production-shaped data;
they do not reconstruct which RPC calls made the original 70.9 MiB trace.

Accelerated retention on a separate working copy inserted six synthetic events
per batch while deleting old history. After 3,186 bounded batches it removed
1,555,382 old events, accepted 19,116 new events, and retained 194,709 rows,
below the documented bound of 276,266 for 88,133 peers. Every original peer's
latest event, latest non-null stamp event and stamp lookup value survived.
Retention selection/deletion used no sorts or automatic indexes. The accelerated
phase including verification took 134.95 seconds, with 71.98 seconds of measured
maintenance and a maximum batch of 509.14 ms. Other local test/probe activity
was present; this is correctness and work-count evidence, not an isolated
latency benchmark. The daemon's one-second batch delay still applies in service.

### Daemon checks with the snapshot

Two separate 150-second runs used fresh snapshot copies, the same memory/CPU
limits as above, 30-second propagation maintenance, a local announcing peer,
100-ms SDK polling and a peer-status request about every five seconds. The two
largest stored propagation peers were replayed through `announce_received` to
exercise their existing handled-ID sets. A synthetic direct message was sent to
the newly generated local peer. Both daemons ran in a separate user/network
namespace with only loopback enabled; namespace isolation was checked against
the host. Production identities were not used for the local test sender.
Two harness setup attempts with incorrect RPC framing were excluded. The final
harness uses the daemon's length-prefixed MessagePack HTTP RPC contract.

Each run completed four propagation maintenance cycles, 29 successful status
polls and 1,500 error-free SDK polls. Both receivers stored the fresh message
and both senders recorded `delivered`. The candidate's live handled-ID metrics
reported 102 calls, zero errors, zero sorts, zero full scans and zero automatic
indexes. It pruned another 72,358 old announce events in 145 scheduled batches
without stopping ingress. The raw namespace-local DBs/logs remain untracked.

| First → last sample, unless marked delta | Snapshot baseline | Snapshot candidate |
| --- | --- | --- |
| Ready after | 0.52 s | 3.77 s |
| cgroup file bytes | 18,042,880 → 34,529,280 | 334,151,680 → 238,854,144 |
| cgroup anonymous bytes | 43,532,288 → 78,528,512 | 41,590,784 → 74,469,376 |
| tracked live Rust allocation bytes | 1,745,122 → 2,217,620 | 1,763,497 → 2,471,651 |
| CPU ticks delta over ~148 sampled seconds | 1,161 | 1,347 |
| logical read / write bytes delta | 6,918,734,465 / 335,780,716 | 12,009,913,378 / 1,031,323,997 |
| physical read / write bytes delta | 1,404,928 / 950,157,312 | 1,290,240 / 2,102,779,904 |
| memory.high / max / oom delta | 0 / 0 / 0 | 0 / 0 / 0 |
| memory PSI some/full delta (μs) | 31,368 / 31,360 | 105,128 / 105,129 |
| maximum sampled cgroup bytes | 191,172,608 | 503,111,680 |
| DB bytes | 1,359,355,904 → 1,359,355,904 | 1,572,937,728 → 1,574,076,416 |
| WAL bytes | 2,513,232 → 17,468,832 | 100,663,296 → 2,809,872 |
| maximum window SDK poll p95 | 1,576 μs | 1,689 μs |

These short runs show the snapshot workload continuing through catch-up; they
are not a cold-cache benchmark or a long-term RAM-savings claim. Page cache was
not flushed, a focused test ran elsewhere on the host, and generated local peer
identities differ. Status output reached three peers in the baseline and four
in the candidate. The exact SQL comparison above uses identical peer bindings
and results. Initial indexes and catch-up increase total I/O even though the
avoidable recurring handled-ID sort disappears. The earlier synthetic run's
13 high events remain relevant evidence that startup pressure depends on cache
and workload conditions. No multi-hour or production deployment qualification
is claimed.

Final validation passed 883 RPC library tests (9 existing ignored), 31 daemon
library tests, 697 daemon binary tests, the issue-369 scanner, native Link-send
and both receipt-owner tests. The focused regression also verifies multiple
completed case variants deduplicate. Formatting, strict affected-package Clippy,
boundary/module checks, document links and JSON validation passed. The original
snapshot SHA-256 was checked again and is unchanged.
