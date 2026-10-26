# Preview.26 regression progress

| PR | State | Branch / head | Evidence |
| --- | --- | --- | --- |
| 1. Timer-driven dirty persistence | Implemented; ingress comparison pending | `corvo/preview26-01-persistence` | Delayed 30 s cleanup only; dirty saves every 300 s (configurable); canonical restore fingerprints; focused daemon/transport tests, module-size check and Luna review passed. |
| 2. Truthful readiness and cheap control | Pending | | |
| 3. WAL maintenance connection | Pending | | |
| 4. Conditional allocator/prune paging | Pending ingress evidence | | |

## PR 1 decisions

- Use Tokio `interval_at` to delay the first cleanup; a separate dirty deadline bounds saves. Failed saves retain dirty state. The explicit shutdown flush remains in place.
- Restore seeds only accepted, canonical cache records using their stored interface. Unknown/noncanonical records still verify before rewriting. No periodic reread remains; runtime external file modification is outside the cache ownership contract.
- No streaming encoder, allocator, incremental cache writer or hashlist format change. Reference: [Tokio interval_at](https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html).
