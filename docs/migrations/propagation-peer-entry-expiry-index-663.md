# Propagation peer-entry expiry is index-driven

Production residency maps for #663 (2026-10-29) showed 174.76 MiB of
`reticulum.db` resident in 29,276 short runs, refilled within about a minute of
eviction. The source was the 300-second maintenance expiry phase: it walked the
whole `propagation_peer_entries` table by rowid and probed the
`propagation_entries` primary key once per row for orphans, then counted the
table twice for quota. On a full table with nothing to delete that re-read
roughly the same pages every pass.

One index is added on first open and builds once on existing databases:
`idx_propagation_peer_entries_expiry (state, updated_at)`. Expiry now selects
each state's marks older than its cutoff through it in 512-row keyset pages
ordered by `(updated_at, rowid)`; an idle pass reads a few index pages. The
per-row orphan join is removed: a mark whose entry is gone now lives until its
own TTL (30 days for completed marks by default) or the quota, both served by
the same index, instead of the next pass. This keeps the documented contract
that completed marks survive a haves purge so reintroduced payloads are not
re-queued to peers that declared them; earlier builds only honoured it until the
following pass. Entry deletion still removes the entry's unhandled marks. Quota
counts the table once per pass. TTLs, per-peer and global limits, ordering and
the writer-release pattern are unchanged. The peer table may therefore hold up
to 30 days of completed marks per peer regardless of entry lifetime, bounded by
`peer_entry_limit` (1,000,000 rows by default).

`storage.maintenance` phases also report `logical_read_bytes_total` and
`last_logical_read_bytes` from the maintenance thread's `rchar`, because the
physical counter reads zero on a warm page cache and hid this scan.
