# Peer-status unhandled-ID query cost (#663)

The November 2026 source inspection found a remaining temporary sort in
`MessagesStore::list_peer_unhandled_propagation_ids`, called by enriched peer
status and sync reporting. The previous derived table calculated four separate
state sums for each case-folded peer/ID group, joined retained inventory, then
sorted the result by ID. SQLite's native query plan reported `USE TEMP B-TREE
FOR ORDER BY`, despite an existing `(LOWER(peer), transient_id, state)` covering
index. This is a concrete local source of temporary-file activity; the production
`etilqs_` traces do not identify the responsible statement and cannot establish
that this query caused all observed memory pressure or polling failures.

The replacement aggregates directly in covering-index order and uses
`HAVING MAX(state IN (...completed states...)) = 0` plus a retained-inventory
existence lookup. It has no derived table, temporary result sort, new index or
schema migration. This is a localized query rewrite using the existing store;
no new abstraction or cache policy is needed. The API still returns all matching
IDs, so its output allocation and index traversal scale with that peer's retained
marks. It does not read payload bodies. The separate limited-ID API is not
called by current production code and is unchanged.

Both statements preserve exact ASCII case-folding, ID ordering/deduplication,
completed-state precedence, unknown-state fallback to unhandled and orphan
exclusion. The 49 paired-state combinations include all four completed states,
unknown/empty states and multiple case variants. A file-backed regression
checks baseline agreement and persistence over two reopenings. The native
12,000-ID cost regression demonstrates the old sort, checks fewer VM steps,
and requires no sort, materialization, automatic index or full-table scan for
the replacement. Existing peer-sync and queue-state tests exercise the callers.

`storage.propagation_queries.unhandled_ids` adds fixed aggregate counters for
calls, errors, wall time/max, VM steps, full-scan steps, sorts and automatic-index
rows. These use the same instrumentation as handled IDs and retain no peer IDs,
parameters or payloads. This supplies attribution on real workloads without
asserting the statement dominates them.

The opt-in Linux workload is reproducible with:

```sh
CARGO_BUILD_JOBS=2 cargo test -p reticulum-rs-rpc --lib \
  storage::messages::unhandled_propagation::soak::sustained_unhandled_id_query_workload \
  -- --exact --ignored --nocapture
```

It creates 100,000 synthetic payload rows and peer marks (10,240 hex bytes per
payload, 1,334,317,056-byte database), then runs baseline/candidate/candidate/
baseline phases of 120 reads each, offered every 500 ms. It samples per-process
CPU, logical/physical I/O, RSS range and swap; reads of `/proc` are included in
I/O counters. Fixture generation and its explicit initial checkpoint precede
the measured query phases. No cache eviction or temp-store change is used.
Afterward, 50,000 completed case variants are committed, the public API excludes
them, and reopening preserves exact baseline agreement.

This is synthetic query evidence, not a full-daemon acceptance run. The retained
payload bytes are zeros encoded as hex; the metadata layout and size exercise
SQLite storage, without representative mesh content, production concurrency,
fragmentation or the reported 521 MiB WAL. The environment also runs other builds,
so deterministic query-plan/VM-step/sort deltas are stronger evidence than wall
clock deltas. End-to-end delivery, acknowledgement and recovery protocols are
unchanged by this read-only query; production MemoryHigh, sustained WAL behavior,
swap and fresh delivery still require their own constrained acceptance evidence.

## Recorded local result (2026-11-07)

The four-phase run completed in 257.29 seconds, including fixture setup and
completed-mark/reopen checks. Each phase made 120 calls returning the same
100,000 ordered IDs. The production Rust source matches commit `6858828f`;
[recorded counters and binary hash](unhandled-peer-ids-663-evidence.json) identify
the measured debug test build, including its two subsequent test-only changes.

| Per call (two phases of each statement) | Baseline | Candidate |
| --- | ---: | ---: |
| SQLite VM steps | 6,500,036 | 4,700,038 |
| SQLite sorts | 1 | 0 |
| Process logical reads | 25.34 MB | 18.54 MB |
| Process logical writes | 6.80 MB | 0 |
| Process CPU time | 304–310 ms | 251–253 ms |
| Mean query wall time per phase | 320–377 ms | 251–256 ms |

The candidate reduced deterministic VM work by 27.7% and removed the result sort.
Each baseline phase generated about 816 MB of logical writes and each candidate
phase generated zero. Physical reads were zero for both statements. Linux
`write_bytes` was about 816 MB per baseline phase, but roughly 815 MB was also
reported as cancelled writes; these counters must not be represented as net
physical disk traffic or added together. These are per-process counters, not a
trace attributing production `etilqs_` files.

RSS ranged from 28,708 to 33,444 KiB across the four phases, and swap stayed zero.
Candidate RSS stayed at 33,436 KiB after baseline allocations: this process reuse
is not evidence of a lower heap footprint or cgroup file-cache ownership. Both
candidate phases were stable over their sampled interval, but four minutes do
not prove a long-term production plateau. The complete RPC library suite passed
891 tests with 10 ignored (including this opt-in workload); formatting, module
size/include ratchet and dependency-boundary checks also passed locally.
