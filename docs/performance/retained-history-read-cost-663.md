# Retained-history read work (#663)

The completed b57/b360 four-hour ingress run passed delivery, polling, recovery
and retained-identity checks, but added 40 regular `memory.high` events. Within
that same fixed workload, hourly primary logical reads grew from 0.501 to 0.877,
2.339 and 4.091 MB/s. This is within-run growth evidence; the shorter historical
runs are not a matched regression comparison. The frozen full report is
`ingress.json`, SHA-256
`0ebc7998d7032ddb4fba8ddd35777a33d1db9778c1eb66da2d8c54b09605332f`.

The unconditional one-second announce-history worker repeatedly selected a
boundary with `ORDER BY timestamp DESC,id DESC LIMIT 1 OFFSET max_rows` while
history remained below its configured 100,000-row window. There was no boundary,
cursor or deletion, so the next tick walked the growing index again. Regular
statement counters recorded 14,357 window calls, 113,564,697 fullscan steps and
227,316,035 VM steps. Announce maintenance recorded 13,611,614,152 logical bytes
on its own synchronous thread. This is a measured contributor, not an exclusive
explanation for all daemon reads or reclaim.

Other maintenance contributed roughly 0.283 MB/s without the same hourly growth.
After subtracting that and announce maintenance, a rough unassigned read budget
still grew from 0.209 to 0.548, 0.892 and 1.248 MB/s. These separately sampled
counters are not an exact additive partition. A delivery announce also invokes
`list_retryable_outbound_for_destination`: its existing query can inspect completed
outbound rows before returning no retryables. Durable ownership is checked only
after this SQL. There was no wakeup-specific byte/VM counter in the runtime trace,
so a controlled SQL experiment establishes query scaling without assigning it the
entire residual.

## Controlled SQL evidence

[The evidence record](retained-history-read-cost-663.json) separates original
main, an intermediate guard and the mutation-aware candidate. All inputs use
owned synthetic SQLite databases; no new daemon or service was launched.

Eight original under-window checks walked 1,016 rows with 128 retained records
and 79,992 with 10,000. The upper-bound fast path walks none. Repeated replacement
at the exact limit causes neither window walks nor exact recounts, with recursive
triggers both off and on. A separate empty outbound lookup cost 5,155 VM steps
with 128 completed rows and 400,035 with 10,000; the unchanged query using the
partial index costs 12 in both cases. Case, status, direction, timestamp ties,
index migration and receipt transitions retain their previous results.

The original active cursor also had a correctness gap: recent-row deletion or
timestamp replacement could cause later batches to delete rows now inside the
newest-N window. Three regressions fail on the pre-correction implementation.
The revised tests preserve the current window after those mutations, migration,
restart and another handle opening; repeated mutations behind more than 512
protected front rows still make progress. Rollback covers deletes, revision,
upper bound and cursor state together.

## Large-limit page work remains material

The file-backed probe uses the runtime ID shape (timestamp, 32-character peer
hash, sequence), a mean ID length of 57.89 bytes and 32-character ASCII peers.
SQLite uses 4,096-byte pages and its unchanged `cache_size=-2000` KiB setting.
The timestamp/ID index is about 14.59 MB and the primary-key index about 7.68 MB.
Page/index diagnostics run after measured operations; no forced cache eviction
or pager-cache reduction conditions the result.

Eight repeated one-row overflows under the original descending-boundary algorithm
read 177,673,232 logical bytes and executed 1,600,112 window VM steps. That file
baseline already contains the narrow under-window guard; below/at-limit checks
are therefore already fast in this particular comparison.

Using an exact count before seeking the shorter indexed end reduces window VM
steps to 112. The first candidate nevertheless reads 123,307,024 bytes because
it counts twice per completed one-row sweep. This demonstrates why constant VM
steps do not establish constant page I/O. A guarded transaction-local derivation removes the second count when only the
known bound reset and direct deletes/revision writes occurred. A deliberately
rejected intermediate shortcut checked the bound and revision alone; its nested
trigger regression inferred 21 rows when 20 remained. The final guard also checks
SQLite total changes from before the reset, falls back on extra writes, and marks
an uncertain active cursor for revalidation. These named tests do not establish
support for arbitrary schema/trigger tampering.

Two matched comparisons in ABBA order (baseline, final, final, baseline) read 177,673,232 bytes
in each baseline run and 61,969,424 bytes in each final run across eight one-row
sweeps. Both algorithms count eight times; the first candidate's sixteen counts
are reduced without introducing per-active-batch counts. Final window VM steps
remain 112. Baseline probe wall times were 48.77/62.12 ms; final times were
12.41/11.81 ms. These are controlled process measurements, not daemon latency.
The final full-window sweep still reads about 7.75 MB each with this fixture and
pager cache: exact COUNT page work remains material despite its eight VM steps.
Below/exact-limit cases make zero COUNT/window calls; their eight maintenance
checks still read about 21.5 KB for other transaction work. No constant-I/O or
daemon resource-acceptance claim is made.

The full RPC library passed 912 tests (11 explicitly ignored). After relocating
the enlarged test module to a conventional `tests/mod.rs`, the final executable
passed all 24 retention and three retry-query tests. Strict release Clippy covers
library and test targets with warnings denied. Named trigger fallbacks cover
nested deletion, extra audit/reset writes, and full-batch inserted-row metrics
with recursive triggers off/on; a failure rolls back bookkeeping and deletes.

Reproduce focused regressions with:

```sh
cargo test -p reticulum-rs-rpc --release --lib announce_retention
cargo test -p reticulum-rs-rpc --release --lib retryable_outbound_tests
LXMF_ANNOUNCE_PROBE_DISK=1 cargo test -p reticulum-rs-rpc --release --lib \
  storage::messages::announce_retention::tests::profile_under_at_and_above_large_retention_limit \
  -- --exact --ignored --nocapture
```

Startup/schema behavior is described in
[the migration note](../migrations/announce-retention-bound-and-retry-index-663.md).
These results establish local storage correctness and cost changes. They do not
reuse the b57 runtime qualification as evidence for this candidate, establish
exclusive residual-read attribution, or establish hours-long resource acceptance.
Exact paired daemon/RCH runtime qualification is recorded on the follow-up PR,
separately from this controlled SQL record.
