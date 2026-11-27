# Peer identity cache index (#776)

`reticulumd` keeps announced peer identities in memory (`peer_crypto`). Lookups by
destination hash used to scan every cached peer while holding the cache lock. The
cache now also indexes the four destination hashes each identity owns:
`lxmf.delivery`, `lxmf.propagation`, `lxmf.propagation.control` and
`r3akt.emergency`. Lookups are O(1) and the lock is never held across O(n) work.

## What does not change

- No database schema, config key, wire format, RPC/SDK surface or metric name.
- The cache is still memory-only and is rebuilt from announces after a restart,
  exactly as before. There is nothing to back up or convert.
- Resolution order is unchanged: task peer identity, propagation-node identity,
  cached announces, outbound propagation identities, then transport lookup.
- `prune_peers` removes the same entries and `peer_crypto_removed_total` counts the
  same way. The index is pruned in the same call, under the same lock.

## Cost profile

- Repeat announces of a known identity do no extra hashing. An identity seen for
  the first time costs four destination-hash derivations at insert.
- Extra memory is roughly 0.4–0.8 KiB per distinct identity. At the ~9,100-peer
  production size reported in #776 that is a few MiB on top of the existing ~3 MiB.
  It shows up in the #709 `rust_allocated_bytes` heap gauge.

## Rollout

1. Deploy the new `reticulumd` binary the normal way (stop, replace, start). No
   offline step and no ordering against other nodes; mixed versions interoperate.
2. After start, `peer_crypto` in `/metrics` (transport tables provider) climbs back
   as announces arrive, as it did before this change.
3. Watch for a few hours:
   - `peer_crypto` and `peer_crypto_removed_total` follow their previous pattern.
   - The heap gauge settles a few MiB higher than before at the same peer count.
   - Delivery logs still show `resolved from cached peer identity` for known peers,
     and no new `peer identity cache lock poisoned` errors appear.

## Rollback

Reinstall the previous binary and restart. No state was written, so rollback needs
no cleanup.
