# Preview.26 regression progress

| PR | State | Branch / head | Evidence |
| --- | --- | --- | --- |
| 1. Timer-driven dirty persistence | Implemented; ingress comparison pending | `corvo/preview26-01-persistence` | Delayed 30 s cleanup only; dirty saves every 300 s (configurable); canonical restore fingerprints; focused daemon/transport tests, module-size check and Luna review passed. |
| 2. Truthful readiness and cheap control | Implemented; package checks running | `corvo/preview26-02-control` | Bind-lifetime readiness; atomic durable capability; 2.5 s replay-safe command wait; reserved control/rejection capacity and correlated deadline outcome. |
| 3. WAL maintenance connection | Pending | | |
| 4. Conditional allocator/prune paging | Pending ingress evidence | | |

## PR 1 decisions

- Use Tokio `interval_at` to delay the first cleanup; a separate dirty deadline bounds saves. Failed saves retain dirty state. The explicit shutdown flush remains in place.
- Restore seeds only accepted, canonical cache records using their stored interface. Unknown/noncanonical records still verify before rewriting. No periodic reread remains; runtime external file modification is outside the cache ownership contract.
- No streaming encoder, allocator, incremental cache writer or hashlist format change. Reference: [Tokio interval_at](https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html).

## PR 2 decisions

- Keep the legal maximum envelope reply reservation: runtime poll batch configuration has no upper cap of 1 MiB, so a smaller unconditional reserve would be unsound. Budget eight maximum legal request/reply pairs; reservations do not preallocate buffers. This increases the control reservation ceiling, with bounded count/bytes unchanged in principle.
- Apply the deadline to replay-safe RPC commands (resume/fetch/ACK/admit/reconcile). Internal dispatch claims and custody operations still wait for their actual completion so a late claim cannot strand delivery scheduling. Queued deadline-expired operations still execute; retry preserves request identity.
- Classify the typed timeout in the actual handler guard on both ZMQ paths, retaining real elapsed time and available queue pressure. Readiness guards clear on loop exit/cancellation; HTTP-only configurations are ready without ZMQ binds.
