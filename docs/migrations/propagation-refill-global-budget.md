# Propagation refill respects the global peer-entry limit

Quota counts every peer mark against `peer_entry_limit`: unhandled marks and
completed history alike. Refill only checked the per-peer pending capacity.
When the table sat at the global limit, quota deleted the oldest unhandled mark
on every pass. The delete trigger set `rescan_required` because the payload was
still in inventory, and the next refill recreated the same (peer, transient)
pair. The table never converged. On the preview32 reproduction every cycle
deleted and inserted one row, three rows to two and back again.

## Invariant

Refill never takes the peer table above `max(1, peer_entry_limit)`. The quota
pass keeps its existing role and order for every other writer.

## Mechanism

- `propagation_peer_entry_budget` (singleton) mirrors `COUNT(*)` of
  `propagation_peer_entries` through AFTER INSERT and AFTER DELETE triggers.
  It is backfilled once, in the schema transaction that creates the triggers.
  Upserts fire UPDATE and ignored inserts fire nothing, so neither moves it. No
  writer of this table uses REPLACE. Each quota write transaction stores its
  exact count, and a quota pass that sees a mismatch takes the writer to repair it.
- The refill read gate skips the writer when the limit is full and the peer
  already has saved pass state (`skipped_global_capacity_total`).
- Each refill page reads the room inside its IMMEDIATE write transaction,
  limits the page to `min(per-peer remaining, global room, invocation share)`,
  and counts every insert in the page. Writers serialize on SQLite's lock, so
  two connections or processes cannot both spend the same room.
- At the limit the page stops before the next unoffered candidate. The saved
  cursor, `rescan_required` or stale generation keeps the pass unfinished, so
  nothing is marked complete or skipped. A peer seen for the first time records
  an unfinished pass once (`deferred_global_capacity_total`).
- Deferred work resumes on the next refill, which runs on every announce and
  peer sync after quota, expiry or entry deletion frees room. The state is in
  the existing refill-state table, so a restart does not lose it.
- When the room is smaller than the peer's remaining need, an invocation takes
  at most `ceil(room / (other peers with an unfinished or stale pass + 1))`, so
  the first caller after room frees cannot take all of it. With enough room the
  peer fills its whole need, as before.

## Unchanged

Quota and expiry ordering, TTLs, the per-peer limit, completed-mark precedence,
fan-out of newly ingested payloads, acknowledgement writes and returned counts.
New fan-out and acknowledgements can still exceed the limit; quota trims them
in its existing order (newest unhandled kept).

## Policy consequence to review

Completed history counts against the limit and is only removed by its TTL
(30 days by default) or by quota when the table is over the limit. A table
filled by completed history therefore defers refill re-offers until that
history expires. Before this change the same state produced endless re-offer
churn instead. Whether pending re-offers should outrank completed history is
a separate policy decision.
