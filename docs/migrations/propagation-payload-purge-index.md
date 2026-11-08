# Propagation payload purge index

Payload quota eviction and recipient `haves` both delete pending peer marks by
`transient_id`. Previously SQLite selected the `(state, updated_at)` expiry
index, reading every unhandled mark for each removed payload. A 4,096-payload
retention page could repeat that scan thousands of times while holding the
daemon writer transaction.

The canonical shared propagation schema now adds
`idx_propagation_peer_entries_unhandled_payload`, a partial index on
`transient_id WHERE state = 'unhandled'`. Both callers use the same deletion SQL.
SQLite searches the matching payload's pending marks directly. Work still
scales with the number of matching peers and maintaining their indexes/triggers;
this does not bound the entire retention pass or remove its size-metadata scan.

The selection and transaction boundaries are unchanged: all exact-key pending
marks for the removed payload are deleted, including peer-case variants.
Completed and unknown-state marks, other payloads, destination matching,
retention weights and priority ordering retain their existing semantics.
Recipient haves for a missing payload or a different destination remove nothing.

Existing databases build the index on open. The daemon runs this inside its
existing startup schema transaction; failures propagate through that transaction.
Building it requires a one-time history scan and may consume temporary space,
WAL space and startup time. Every pending mark adds an index entry, with ongoing
disk/write cost on insertion, deletion and transitions into or out of
`unhandled`. Completed marks add no entries. Older binaries can ignore the
additional index. No message, payload or durable broker format changes.

An ordinary partial index keeps SQLite responsible for transactional consistency
and recovery. A new in-memory cache or authoritative projection would introduce
invalidation/recovery work without improving the exact-key lookup needed here.

Local regression evidence uses the production deletion statement with three
matching pending marks and mixed completed, unknown and unrelated rows. The
negative control drops only the new index. SQLite statement counters were:

| Unrelated pending marks | Previous VM steps | Indexed VM steps |
| ---: | ---: | ---: |
| 1,000 | 5,177 | 181 |
| 100,000 | 500,177 | 181 |

Both variants delete exactly three marks and perform zero sorts. The indexed
work stays constant as unrelated history grows. These counters exclude daemon
triggers, initialization, filesystem I/O and whole-daemon resource use. The
separate daemon migration regression verifies its real trigger-maintained budget
and completed history after recipient purge, retention and database reopening.
Shared-storage tests cover rollback, repeated schema initialization, state
transitions and exact survivors. Cost and migration regressions fail before the
index is added.

Reproduce from this revision:

```bash
cargo test --locked -p lxmf-propagation --all-features payload_purge -- --nocapture
cargo test --locked -p reticulum-rs-rpc --test propagation_payload_purge
```

This fixes a measured source of read amplification under #663. It does not
attribute production temporary-file writes or demonstrate sustained cgroup
MemoryHigh, swap, WAL or delivery acceptance. No production limits, cache policy,
retention policy or polling intervals change.
