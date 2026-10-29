# Propagation peer-entry expiry is index-driven

Production residency maps for #663 (2026-10-29) showed 174.76 MiB of
`reticulum.db` resident in 29,276 short runs, refilled within about a minute of
eviction. The source was the 300-second maintenance expiry phase: it walked the
whole `propagation_peer_entries` table by rowid and probed the
`propagation_entries` primary key once per row for orphans, then counted the
table twice for quota. On a full table with nothing to delete that re-read
roughly the same pages every pass.

Two indexes are added on first open and build once on existing databases:
`idx_propagation_peer_entries_expiry (state, updated_at)` and
`idx_propagation_peer_entries_transient (transient_id)`. Expiry now selects each
state's marks older than its cutoff through the first index in 512-row keyset
pages ordered by `(updated_at, rowid)`; an idle pass reads a few index pages.
Deleting a propagation entry removes its peer marks in every state through the
second index, matching Python, where per-peer handled/unhandled accounting lives
inside the entry and leaves with it. Earlier builds kept completed marks until
the next orphan sweep; that sweep now runs once per process start for rows left
by those builds. Quota counts the table once per pass. TTLs, per-peer and global
limits, ordering and the writer-release pattern are unchanged.

`storage.maintenance` phases also report `logical_read_bytes_total` and
`last_logical_read_bytes` from the maintenance thread's `rchar`, because the
physical counter reads zero on a warm page cache and hid this scan.
