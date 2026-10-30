# Propagation peer-entry quota is index-driven

The quota phase of propagation maintenance ranked every unhandled peer mark with
`ROW_NUMBER() OVER (PARTITION BY peer ORDER BY updated_at DESC, transient_id DESC)`
on every 300-second pass, even when no peer was over quota. No index matched
that order, so each pass looked up every unhandled row in the table and sorted
the whole unhandled set in a temporary B-tree, holding the shared writer
throughout.

One partial index is added on first open and builds once on existing databases:
`idx_propagation_peer_entries_unhandled_rank (peer, updated_at DESC,
transient_id DESC) WHERE state = 'unhandled'`.

- A read-connection gate checks whether any exact peer key has more unhandled
  marks than the per-peer limit (a covering walk of that index, no sort) and
  counts the table. If neither quota is exceeded the pass returns without taking
  the writer.
- Otherwise one write transaction runs the original ranking, restricted to
  over-quota peers and read in index order (no sort), recounts, and applies the
  unchanged global stages: oldest unhandled marks first, then the four completed
  states, ordered by `updated_at, transient_id, peer`. The global unhandled stage
  is pinned to `idx_propagation_peer_entries_expiry` so the new index cannot
  change its plan.

Limits, `max(1)` effective minimums, TTLs, expiry paging, completed marks and
deletion counts are unchanged. Deletions are still decided and applied in one
atomic transaction; a write that commits after the gate's snapshot is seen by
that transaction or by the next pass, as with any write that lands after a pass.

Cost: the index holds one entry per unhandled mark (about 114 bytes each, 29 MiB
for 256,000 marks; unhandled marks are bounded by peers × per-peer limit after
each pass). Inserting an unhandled mark or moving one out of `unhandled` updates
one more B-tree: about 18% more SQLite VM steps per insert and 16% per state
change in the benchmark below.
