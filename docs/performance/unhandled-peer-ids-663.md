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
