# Preview.26 regression progress

| PR | State | Branch / head | Evidence |
| --- | --- | --- | --- |
| 1. Timer-driven dirty persistence | Implemented; ingress comparison pending | `corvo/preview26-01-persistence` | Delayed 30 s cleanup only; dirty saves every 300 s (configurable); canonical restore fingerprints; focused daemon/transport tests, module-size check and Luna review passed. |
| 2. Truthful readiness and cheap control | Implemented; package checks running | `corvo/preview26-02-control` | Bind-lifetime readiness; atomic durable capability; 2.5 s replay-safe command wait; reserved control/rejection capacity and correlated deadline outcome. |
| 3. WAL maintenance connection | Implemented; combined verification running | `corvo/preview26-03-wal` | Dedicated disk checkpoint connection, shared maintenance cadence, throttled emergency attempts, checkpoint frame gauges and traffic-mutex regression. |
| 4. Conditional allocator/prune paging | Pending ingress evidence | | |

## PR 1 decisions

- Use Tokio `interval_at` to delay the first cleanup; a separate dirty deadline bounds saves. Failed saves retain dirty state. The explicit shutdown flush remains in place.
- Restore seeds only accepted, canonical cache records using their stored interface. Unknown/noncanonical records still verify before rewriting. No periodic reread remains; runtime external file modification is outside the cache ownership contract.
- No streaming encoder, allocator, incremental cache writer or hashlist format change. Reference: [Tokio interval_at](https://docs.rs/tokio/latest/tokio/time/fn.interval_at.html).

## PR 2 decisions

- Keep the legal maximum envelope reply reservation: runtime poll batch configuration has no upper cap of 1 MiB, so a smaller unconditional reserve would be unsound. Budget eight maximum legal request/reply pairs; reservations do not preallocate buffers. This increases the control reservation ceiling, with bounded count/bytes unchanged in principle.
- Apply the deadline to replay-safe RPC commands (resume/fetch/ACK/admit/reconcile). Internal dispatch claims and custody operations still wait for their actual completion so a late claim cannot strand delivery scheduling. Queued deadline-expired operations still execute; retry preserves request identity.
- Classify the typed timeout in the actual handler guard on both ZMQ paths, retaining real elapsed time and available queue pressure. Readiness guards clear on loop exit/cancellation; HTTP-only configurations are ready without ZMQ binds.

## PR 3 decisions

- Scheduled PASSIVE/TRUNCATE runs on a dedicated local connection with zero busy timeout for checkpoint calls. In-memory stores keep their single connection (no WAL).
- Cache the maintenance interval at store open; the daemon and storage share the same parser. Inline emergency TRUNCATE retries at most once per interval, including busy attempts. Recovery after a reader unpins may wait until the next scheduled pass; durability integration tests now explicitly model that pass and retain custody, ACK and budget assertions.
- `storage.wal.last_checkpoint` reports the last executed checkpoint's `busy`, `log_frames`, and `checkpointed_frames`, including emergency attempts. The final TRUNCATE result is zeroed after successful work; a pinned PASSIVE keeps incomplete frame counts.
- Source inspection found no read statement escaping the inspected `with_read_conn` closures. Production pinned-reader identity remains unproven; no speculative reader-lifecycle change was made. [SQLite checkpoint semantics](https://sqlite.org/pragma.html#pragma_wal_checkpoint).

## Validation record (in progress)

- Luna exploration, correctness and maintainability reviews covered each increment. Findings corrected: exact canonical interface parsing for restore, maximum legal admission bytes, and actual handler timeout classification with tuple-encoded responses.
- Initial expanded all-features package tests exposed propagation-decrypt failures associated with the optional Fernet feature combination; the plan's standard feature package suite is being checked separately. This extra test mode is not reported as passing.
- Local boundary and module-size checks passed. All-features Clippy passed before the final small response-shape/test corrections; final gates are running.
- Paired musl baseline/candidate ingress runs use the same 200k-entry, 100k-route fixture, two CPUs and 768 MiB/1 GiB limits. A glibc candidate startup was stopped before sampling and is excluded from comparisons.
