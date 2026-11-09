# Announce retention bookkeeping and retryable-message index (#663)

Startup installs private SQLite bookkeeping for announce-history maintenance and
an index for the existing outbound retry query. Message/announce records, wire
formats, configured retention limits and delivery ownership do not change.

`announce_retention_count` stores a conservative row-count upper bound and a
monotonic window revision. A `BEFORE INSERT` trigger increases the bound only
when the new ID is absent. Existing-ID replacements do not grow it, with either
setting of `recursive_triggers`. Deletions deliberately leave the bound high;
ignored inserts may also overestimate it. Neither case permits skipping needed
pruning. Startup performs an exact count and updates only the bound, preserving
the revision when another handle opens the database.

The upper bound avoids repeated history walks when every row is necessarily
inside the configured recent window. `clear_announces` resets it transactionally.
Completed sweeps reconcile it with an exact count. A fresh-window sweep can
reuse its transaction-local starting count minus its own deletes when the stored
bound is unchanged, the revision increases by exactly the number deleted, and
SQLite's total-change delta is exactly one bound reset plus two changes per
deleted row (announce and revision). Checked arithmetic and any mismatch force
an exact completion count. The change counter is captured before the bound reset.
Active batches have no reset and do not acquire a starting count just for this
optimization. An unknown change leaves the next cursor revision unknown. A bound is never presented
as an exact current row count: `row_count_upper_bound` and
`under_window_skips_total` are separate metrics. `retained_at_last_sweep` remains
the last completed exact measurement and may be stale while checks are skipped.

The cursor gains a nullable `window_revision`. A legacy/unknown revision requires
current-window validation; it is not assumed to match a new revision of zero.
Deletes, timestamp/ID updates and existing-ID replacements invalidate a captured
window. The replacement trigger also covers SQLite's suppressed internal delete
triggers when `recursive_triggers` is off. Ordinary new IDs need no invalidation:
insertion cannot move an old row into the newest-N set.

On invalidation, maintenance clips the previous descending progress against the
current recent-window boundary. It preserves the cursor's strict comparison on
equal keys, so repeated changes cannot repeatedly restart a protected prefix and
starve older rows. Own bounded deletions and the post-deletion revision are
committed with the next cursor. Revalidation, metadata and deletes roll back
together on failure. Latest-per-peer and latest non-null stamp protections remain
in the existing delete statement; a batch still examines at most 512 candidates.

`idx_messages_retryable_destination` indexes `LOWER(destination), timestamp, id`
only for rows matching the existing outbound/retryable status predicate. The
query itself is unchanged, including SQL whitespace/case behavior and timestamp/ID
ordering. Terminal rows therefore do not add work to an empty retry lookup.
Receipt transitions update index membership in the same SQLite transaction.
The caller retains its durable-operation ownership checks and returns all eligible
results; no arbitrary result cap or retry batching is introduced.

These invariants require the supported built-in bookkeeping schema and triggers.
The named extra-trigger regressions exercise conservative fallback cases; they do
not promise correctness under arbitrary user-installed metadata triggers that
mutate history during later cursor or bound writes.

Both changes install through the existing startup schema transaction. Creating
the index reads existing messages once and requires its database/index space;
startup reconciliation reads announce cardinality once. No VACUUM, history purge,
cache eviction, durability setting or WAL-budget change is part of this migration.
The controlled evidence and remaining count-page cost are recorded in
[the performance report](../performance/retained-history-read-cost-663.md).
