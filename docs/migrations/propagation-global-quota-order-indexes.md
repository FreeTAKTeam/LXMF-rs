# Global propagation peer-entry quota order indexes

The shared `lxmf-propagation` schema adds two partial indexes on
`(updated_at, transient_id, peer)`: one for `state = 'unhandled'`, one for the
four completed states (`handled`, `transferred`, `received`, `transfer_limited`).
The daemon's global quota deletes use these indexes to select the oldest excess
marks directly. Equal timestamps keep the existing transient-id then exact-peer
ordering, including case variants. Unhandled marks are still removed before
completed marks; TTLs, limits, refill budgets and transaction boundaries do not
change.

Previously a one-row excess could sort every completed mark, or all unhandled
marks tied on timestamp, while holding the writer. The new selection walks only
its limited candidate prefix. Large excesses still require proportionate index
maintenance and remain in the existing atomic quota transaction. This change
does not bound the entire quota pass or eliminate the count/per-peer gate scan.

Existing databases create the indexes on open in the existing startup schema
transaction before traffic admission. Creation can take time, temporary space,
WAL space and persistent disk space; failures propagate and roll back through
the caller's transaction. Each recognized mark adds one index entry in addition
to existing indexes. Inserts, state transitions and deletes maintain it through
SQLite's ordinary transaction machinery. No payload format or durable state is
rewritten. Older binaries can ignore the additional indexes.

The indexes live in the canonical shared storage schema, with no daemon schema
copy. This chooses an ordinary order-covering index over a new queue/counting
projection: SQLite maintains ordering and recovery atomically, at the explicit
cost of persistent index space and write amplification. Query-plan and statement
counter regressions exercise the daemon's actual delete SQL; existing policy
oracle tests cover retained rows across ties, mixed peer case and both stages.

This is local algorithmic evidence only. Attribution of production temporary
file writes, memory.high, file-cache residency, timeouts and end-to-end delivery
remains open under #663. No production intervention or soak is part of this
migration.
