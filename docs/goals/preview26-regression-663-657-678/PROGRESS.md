# Preview.26 regression progress

| PR | State | Branch / head | Evidence |
| --- | --- | --- | --- |
| 1. Timer-driven dirty persistence | Implemented; resource acceptance open | [#695](https://github.com/FreeTAKTeam/LXMF-rs/pull/695), `cd7f616d` | Paired static-musl ingress: 210→1 saves, 2→0 verifications, CPU 14.33→7.83%, sampled high events 2,013→205. Retention memory/read/wait regressions remain. |
| 2. Truthful readiness and cheap control | Implemented; production acceptance open | [#696](https://github.com/FreeTAKTeam/LXMF-rs/pull/696), `105a113f` | Actual failed bind: ready503/health200/live200. 8,166 negotiations: p99 1.087 ms, 0 errors during 41 saves and 23 checkpoints. SDK retry/reconcile and older-client timeout category regressions pass. |
| 3. WAL maintenance connection | Implemented; production reader attribution open | [#697](https://github.com/FreeTAKTeam/LXMF-rs/pull/697) | Dedicated checkpoint connection, traffic-mutex and throttle regressions; 14 durability integration tests passed. Frame gauges cover normal/emergency attempts. |
| 4. Conditional allocator/prune paging | Deferred: conditional attribution not established | No PR | One retention step with a flat observed tail does not demonstrate repeated ratcheting; cumulative wait does not prove a single multi-second critical section. |

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

## Earlier validation record

- Luna exploration, correctness and maintainability reviews covered each increment. Findings corrected: exact canonical interface parsing for restore, maximum legal admission bytes, and actual handler timeout classification with tuple-encoded responses.
- Initial expanded all-features package tests exposed propagation-decrypt failures associated with the optional Fernet feature combination; the plan's standard feature package suite is being checked separately. This extra test mode is not reported as passing.
- Local boundary and module-size checks passed. All-features Clippy passed before the final small response-shape/test corrections; final gates are running.
- Paired musl baseline/candidate ingress runs use the same 200k-entry, 100k-route fixture, two CPUs and 768 MiB/1 GiB limits. A glibc candidate startup was stopped before sampling and is excluded from comparisons.

## Final local evidence

- See the [paired ingress and control record](../../performance/rch-resource-fixes-663.md#2026-10-26-preview26-paired-ingress-and-control-evidence) for exact improvements, regressions and limits.
- Four-package suite: 3,329 passed / 0 failed / 135 ignored, plus SDK timeout recovery and RPC timeout metadata regressions. Consistent optional Fernet profile: 687 daemon tests passed. Final Clippy, fmt, architecture/no_std, boundaries, module size and 13 harness tests passed; issue #369 scanner is included in the package suite.
- `release-check` is not green: the registry package dry-run resolves published 0.15.0 libraries without post-release APIs (#694 and this increment). Earlier runtime lanes passed. This does not prevent an explicitly labeled local static daemon test artifact, and no registry release is attempted.
- POST plan review: aligned implementation; correctness review found and fixed SDK recovery/category compatibility. Maintainer review found no remaining actionable source issue. Production acceptance remains unproven.
- Operator preview.26 metrics received on 2026-10-26 and preserved in [the exact excerpt](evidence/operator-preview26-2026-10-26.json). Persistence amplification is supported; the WAL is 96 MiB with zero explicit attempts, which is consistent with threshold equality under a 6 GiB budget. No deployment, issue closure or production plateau claimed.
- Daemon-only prerelease `reticulumd-test-0.15.0-rch663-678.2` is published; exact source, hashes and fresh-download verification are recorded below and with its assets. Hosted draft-PR checks remain a separate lane.

## Published daemon-only test artifact

[reticulumd-test-0.15.0-rch663-678.2](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/reticulumd-test-0.15.0-rch663-678.2)
is published as a prerelease from immutable source `84103b90f25fef3f93f63ecc6318dc1baed51a32`.
Fresh public downloads passed SHA256SUMS, executable mode, static linking,
source/tag binding and `--help`; a separate 60-second constrained ingress/poll
smoke of the downloaded binary completed with zero poll errors. The local
verification record is `target/release-records/reticulumd-test-0.15.0-rch663-678.2/verify-public/public-verification-result.json`.
The archive SHA-256 is `56c785fa6fe1030e9019f27315908d4e4d98d060b1b4589a656db2508fd5fea3`;
the daemon SHA-256 is `48ee4e199f11faa4a9eaa21af54f3e093be194166f51872b8c37060a91bb811a`.
No crates, OCI images, deployment or hosted provenance were produced. The
current review branch's later documentation commit records this verification;
it does not replace the immutable prerelease source. Hosted PR checks were
still in progress at this handoff, with no failed check observed.

## Operator baseline received

The [operator excerpt](evidence/operator-preview26-2026-10-26.json) records
2,177 saves, 67.8 million records examined and 42.3 minutes of accumulated
path-save wall time, with zero save errors. Cache collection removed about
1.05 million files / 256.4 MiB with zero cleanup errors. These are cumulative
counts; no uptime or population trend was supplied.

The WAL is exactly 96 MiB, so with the plan's 6 GiB retained budget the deployed
`<= threshold` check explains zero recorded explicit checkpoint attempts.
This does not confirm a blocked reader or the earlier 546 MiB WAL observation.
The [performance record](../../performance/rch-resource-fixes-663.md#operator-preview26-metrics-supplied-on-2026-10-26)
details the verified metric semantics and limits. The production baseline
request is fulfilled; post-deployment and resource/retention acceptance remain open.
