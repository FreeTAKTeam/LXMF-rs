# RCH resource follow-up: LXMF-rs #655

The [operator report](https://github.com/FreeTAKTeam/LXMF-rs/issues/655)
identifies reticulumd as the source of measured disk traffic and memory/swap
pressure on the preview.13 daemon `67e63710986111fbf671dd3cf823d57615f8596f`.
Readiness remained healthy while SDK polling stopped progressing. Production
heap/object and database-write attribution is unavailable locally.

## Confirmed defects and corrections

1. Path-table flushes rewrote unchanged cached announce files. Compare exact
   encoded bytes before writing, with a bounded read of the encoding length plus
   one. Hash equality does not bind interface references or mutable headers.
   Changed/corrupt files still persist; file errors propagate. Existing small
   path-table and tunnel writes remain unchanged.
2. Propagation ingestion durably stored payloads and kept a second hex-encoded
   copy of every payload in an unbounded process map. Remove that private owner
   and its fallback reads. MessagesStore owns ingestion, aliases, fetch, listing
   and configured pruning. Explicit durable deletion is authoritative; deleted
   content cannot reappear from RAM. No stored rows are deleted by this change
   and no new retention policy is introduced. Router statistics count durable
   rows, including after restart, instead of the removed process map.
3. Destination offers loaded full payloads to return only IDs and sizes. Query
   those columns using the existing covering destination/size/ID index. Preserve
   normalization, ordering and size conversion. No schema migration is needed;
   payload fetch still validates content and exposes storage errors.
4. SDK polling held the event-log lock during identity lookup, JSON sizing and
   response metadata. Resolve identity scope before taking the lock, snapshot at
   most the requested visible shared immutable event references, then release
   it before encoding and metadata. Snapshotting does not clone whole payloads.
   Keep cursor expiry/restart, lifecycle filtering, hidden-event cursor progress,
   gap reporting, overflow policies and event/batch limits.
5. SDK timeout messages identify request ID and stage: transport lock,
   connection, send or correlated response. Preserve the shared deadline, machine
   code, category and failed-transport reset rules. No daemon mutation work is
   cancelled by this change.

## Release-mode local evidence

Linux, GCC, installed locked dependencies, NVMe-backed disposable database/cache
files. Probes are explicit ignored tests; no production databases were modified.

| Probe | Baseline | Fixed |
| --- | --- | --- |
| Repeat 10,000 unchanged cached announces | 40,960,000 kernel-accounted write bytes; 223.22 ms | 0 write bytes; 270.75 ms |
| 1,000 propagation rows, RSS after ingestion | 139,292 KiB | 11,860 KiB |
| Same process RSS after offer listing | 207,216 KiB | 11,860 KiB |
| Offer listing latency | 79.28 ms | 0.66 ms |

The cache counter is `/proc/self/io` kernel accounting, not completed device
throughput; truncation/writeback timing can affect it. Tokio wakeups also affect
aggregate write syscall counts (20,000 baseline, 30,000 fixed), so these are not
cache-file write counts. Exact-byte/sentinel-mtime tests establish that unchanged
files are not rewritten. Bounded reads trade elapsed time for reduced writes.

The memory probe starts near 8.5 MiB RSS, ingests 1,000 unique 65,536-byte inputs
at cost zero and uses existing normalization (65,504 stored bytes each), then
lists their IDs/sizes. Persistent row and byte counts match: 1,000 rows and
65,504,000 bytes. These short measurements have no enforced memory cgroup.

Reproduce with:

```sh
cargo test --release -p reticulum-rs-transport profile_repeated_announce_cache_writes -- --ignored --nocapture
cargo test --release -p reticulum-rs-rpc profile_propagation_payload_retention -- --ignored --nocapture
```

Correctness regressions cover cached-path restore, changed interfaces/headers,
corrupt files and IO errors; durable aliases and reopen; explicit deletion;
metadata-only covering queries and payload errors; identity lookup blocked while
publication proceeds; and timeout-stage messages with transport reset/correlation.
The old cache-retention, publication-lock and timeout-stage regressions fail on
the baseline. Use a short temporary-directory path for Unix socket tests.

## Remaining acceptance

These corrections remove reproduced retention, reads, writes and publication
blocking. They do not prove every cause of #655 is removed. Event retention and
queued ZeroMQ responses still have count limits rather than aggregate byte
admission limits. Readiness does not independently qualify an SDK round trip.
No new eviction/backpressure policy, service-limit increase, history pruning or
worker cancellation is part of this fix.

Re-test the exact updated daemon and RCH pins on the production 2-CPU / 2 GiB
workload: fresh inbound receipts, SDK event progress, per-process disk rates,
anonymous memory, swap and memory.high counts over an overnight interval.
Production SQLite contents, filesystem and reclaim behavior have not been
reproduced by the short probes. Keep #655 open until that observation succeeds.
This source update publishes no new release.

## Local verification

RPC: 762 ordinary tests passed (the explicit profile is ignored by default).
Transport: 900 library tests plus its integration suites passed. SDK: 242 library
tests plus integration suites passed with the ZeroMQ backend enabled. The full
reticulumd package suite passed, including issue #369 diagnostics and the real
two-worker readiness/ZeroMQ-polling test under announce-storage contention.
Denied-warning clippy passed for RPC, SDK, transport and daemon; formatting,
module sizes and dependency boundaries passed. The daemon release build includes
its default `zmq-pipeline-rpc` feature. Its local binary SHA256 is
`6b8dccc14c5c16d347d02fae9bf6f6d8a4450524bff525155888151eadfb3670`.
These are local results; hosted checks must be assessed on the pushed head.
