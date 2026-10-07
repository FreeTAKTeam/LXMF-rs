# Framework correction and evidence: LXMF-rs #657

Investigation date: 7 October 2026. [Issue #657](https://github.com/FreeTAKTeam/LXMF-rs/issues/657)
reports propagation bookkeeping growth, private memory/swap growth and correlated
ZeroMQ response timeouts after deploying RCH preview.17.

Five isolated release-mode probes reproduce framework-side queue expansion,
stale-snapshot replay, payload-loading overhead, queue-copy overhead and event
retention. RCH is absent from these probes. They identify concrete allocation
and bookkeeping mechanisms; they do not attribute the entire reported 1.41 GiB
production RAM-plus-swap footprint or reproduce its transport timeouts.

The baseline framework main is `05375df512aca66d76c4e2e58f4c610b29064a7e`.
Its Git tree is identical to deployed daemon source
`81344ae1eccc79612fe933efe990c8da55809254` (`git diff --stat` is empty).
The correction uses a clean framework worktree based on that commit. Local
framework source and tests are changed; production services, configuration and
databases were not touched. No release has been published for this correction.

## Implemented correction

- Fanout selection copies only peer ID, last-seen timestamp and static rank,
  preserving the existing selection and ordering rules.
- Replenishment subtracts existing logical pending marks from the configured
  per-peer capacity inside the insertion SQL statement. Concurrent callers
  cannot add separate full batches; completed marks are not reopened.
- Legacy queue snapshots import once per live peer record. Import
  reads the live record, checks payload existence in SQL and preserves completed
  state. Import tracking is discarded with removed peer records. Maintenance
  imports before pruning. Later reporting refreshes use
  persisted state and cannot recreate pruned marks from cached snapshots.
- Queue refresh reads IDs and completed/pending classification in a single
  SQLite query. It neither loads payload hex nor performs per-ID queries or
  quadratic deduplication.
- All local/remote `peer_sync` events omit inventory ID arrays and transferred
  payload arrays before legacy/SDK retention, sinks and broadcasts. Counters,
  peer identity, scheduling and failure information remain. Explicit RPC replies
  keep their detailed inventories and existing schema. The event contract is
  documented in `docs/contracts/rpc-contract.md`.

Correctness regressions cover repeated/concurrent refills, case-variant peers,
completed-state precedence and completion age, a freed queue slot, stale
snapshots, peer-removal cleanup, read operations
without queue writes after import, event summaries and fanout ordering. Three
new regressions were first run against the baseline and failed as expected.
Existing restart fixtures now explicitly model the initial import phase;
subsequent runtime snapshots cannot regain authority over durable state.

## Baseline reproductions

Each result below comes from a separate test process. Units in the raw logs are
KiB from `/proc/self/status`; RSS and high-water readings are approximate kernel
accounting, not live heap measurements. There was no enforced 2 GiB cgroup and
no swap usage in these short probes.

| Probe | Unchanged framework result |
| --- | --- |
| Repeated replenishment of one peer, 2,050 stored messages | Pending marks grow **1,024 -> 2,048 -> 2,050** across three calls. Maintenance removes 1,026, leaving 1,024. The next replenishment grows the queue to 2,048 again. |
| Replay a snapshot cloned before maintenance | Maintenance reduces pending marks from 2 to 1. Replaying the stale snapshot recreates the deleted mark, returning to 2. |
| Refresh IDs for 256 persisted 64 KiB payloads | The implementation loads **32 MiB of payload hex** while rebuilding 256 IDs. Process high-water RSS rises from 11.7 to 47.6 MiB; RSS after the call is 13.7 MiB. Elapsed time: 63.49 ms. |
| Select 512 fanout peers holding 2,050 IDs each | One selection clones **1,049,600 identifier strings**. RSS rises from 113.3 to 218.1 MiB. Elapsed time: 79.10 ms. This fixture seeds runtime queue snapshots, not a million-row SQLite database. |
| Repeated postponed synchronization for a peer with 1,024 received IDs | At 1,024 retained SDK events, RSS rises from 10.1 to 137.5 MiB. Those events contain 71,962,624 serialized bytes, repeating the same identifiers. At 1,100 publications the log still holds 1,024 events and RSS is 137.7 MiB. |

These measurements describe the unchanged baseline. Queue-bound and stale
snapshot probes are now ordinary desired-behavior regression tests. Memory
qualification remains opt-in so it can run in a fresh process; its structural
assertions now check the corrected behavior.

## Corrected fresh-process measurements

The same isolated fixtures were rerun against the corrected source using the
same RPC-only release build configuration as the baseline.

| Probe | Corrected result |
| --- | --- |
| Repeated replenishment, 2,050 stored messages | Pending counts remain **1,024 -> 1,024 -> 1,024**. Maintenance removes no excess marks; another refill remains at 1,024. |
| Stale snapshot after maintenance | Pending count remains **1**, with no deleted mark recreated. |
| Refresh 256 IDs backed by 64 KiB payloads | No payload hex is selected. High-water RSS remains **11.6 MiB** before/after refresh, versus 47.6 MiB at baseline. Elapsed time: **0.21 ms**, versus 63.49 ms. |
| Select 512 fanout peers holding 2,050 IDs each | Additional RSS is **0.31 MiB**, versus 104.9 MiB at baseline. Elapsed time: **0.11 ms**, versus 79.10 ms. The fixture still owns its original million-ID cache. |
| 1,024 retained postponed peer-sync events | Serialized event data is **1,577,984 bytes (1.50 MiB)**, versus 71,962,624 bytes (68.63 MiB): **97.8% less**. RSS is **21.1 MiB**, versus 137.5 MiB. At 1,100 publications the count remains 1,024 and RSS is 21.2 MiB. |

These are short synthetic measurements, not production heap attribution or a
sustained memory/swap plateau qualification. Small RSS/timing differences are
subject to allocator, page-fault and scheduling variation. The final evidence
JSON records the measured test executable's SHA-256.

## Baseline event-retention mechanism

Even postponed/backoff synchronization constructs `messages.handled_ids` and
`messages.unhandled_ids` from the entire stored peer history and publishes them
in a new `peer_sync` event. This occurs without any successful payload transfer.
The SDK event log stores up to 1,024 events and checks count, not aggregate
retained bytes. An Arc shares each event reference between readers; distinct
events still own distinct JSON data and repeated identifier strings.

Sources: [postponed synchronization event](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_messages_parts/rpcdaemon_sections/enriched_peer_status_row.rs#L109),
[event-log insertion](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/events.rs#L132).
Successful sync events additionally include selected propagation payloads.

The event probe deliberately uses only 1,024 completed associations and tiny
16-byte payloads. Event retention alone adds about 127 MiB RSS before reaching
the count cap. Production reports a much larger completed queue for one peer
(27,542 identifiers); actual event-size distribution and retention still need
measurement before extrapolating this probe to production.

After explicitly clearing both SDK and legacy queues, the probe still reports
89.5 MiB RSS. Cleared logical owners and persistent RSS must be distinguished;
allocator behavior or other retained allocations require heap attribution.
The bounded probe shows substantial retention until the event cap, not an
unbounded leak under its fixed workload.

## Baseline queue-expansion mechanism

`queue_existing_propagation_for_peer` passes the configured per-peer value to
`mark_recent_propagation_unhandled_for_peer`. The latter inserts up to that many
previously absent associations. It does not subtract existing pending marks.
Repeated calls therefore add additional full batches until the available
message history is exhausted.

Sources: [queue replenishment](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/init_parts/rpcdaemon_sections/default_ticket_expiry_secs.rs#L70),
[insertion SQL](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/storage/messages_parts/messagesstore_sections/list_messages.rs#L307).

Maintenance does prune the synthetic backlog correctly in isolation. However,
`restore_peer_record_queue_marks` can reinsert marks from a snapshot obtained
before pruning. Both `list_peers` and `peer_sync` clone records and call this
restoration function during normal operation. The stale-snapshot probe
demonstrates that replay mechanism; a production concurrency trace was not taken.

Sources: [restoration writes](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_messages_parts/rpcdaemon_sections/outbound_message_for_query.rs#L197),
[list_peers cloning and restoration](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/dispatch_legacy_messages_parts/rpcdaemon_sections/handle_rpc_legacy_message_catalog.rs#L215).

The current limit scopes matter:

- The per-peer pruning cap covers **unhandled** rows. A received/completed queue
  larger than 1,024 is not, by itself, a violation of this implementation's cap.
- The global cap counts all associations, deleting old unhandled rows first,
  then completed rows if necessary. Enforcement runs during maintenance, not
  transactionally at every insertion. A snapshot above one million does not
  alone prove maintenance is broken.
- The automatic worker waits for maintenance to finish, then schedules the
  next run after another configured interval. A long run lengthens the effective
  period between completions. It logs successes with pruning and errors, but
  does not expose the requested full start/end/backlog telemetry here.
- Historical peer rows are not equivalent to active propagation peers.
  `max_propagation_peers=512` does not imply that only 512 historical identifiers
  may exist in SQLite.
- The 256 MB message-storage policy accounts for payload bytes. It is not a
  process-memory cap or a cap on all peer-association/index database space.

Sources: [pruning policy](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/storage/messages_parts/messagesstore_sections/prune_propagation_peer_entries_to_policy.rs),
[automatic maintenance worker](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/apps/reticulumd/src/bin/reticulumd/bootstrap_parts/module_core.rs#L680).

## Baseline copies and database work

`propagation_fanout_peer_ids` clones complete active PeerRecords, including both
identifier vectors, under the peers mutex. It sorts and truncates those clones,
then returns only peer IDs. Every newly ingested propagation message can invoke
this path. Selecting lightweight peer metadata would preserve its ordering and
selection behavior without copying the entire queue inventory.

`refresh_peer_queue_snapshot_from_storage` loads full unhandled payload records
to obtain IDs, issues per-ID completed checks, and loads completed payloads just
to check their existence. Deduplication uses repeated linear scans. This refresh
operates across all stored runtime peer records after policy pruning, including
historical records. Normal queue restoration performs similar payload reads.

Sources: [fanout selection](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/init_parts/rpcdaemon_sections/default_ticket_expiry_secs.rs#L34),
[snapshot refresh](https://github.com/FreeTAKTeam/LXMF-rs/blob/05375df512aca66d76c4e2e58f4c610b29064a7e/crates/libs/rns-rpc/src/rpc/daemon/init_parts/rpcdaemon_sections/default_ticket_expiry_secs.rs#L198).

These costs can amplify allocation churn and lock/SQLite contention. Neither
probe measures their production invocation rate or establishes a causal link
to a particular ZeroMQ timeout. The response writer's one-second deadline
remains unchanged. Maintenance itself already runs on a joined blocking worker;
moving it off the async reactor alone is not a new fix for this report.

## Remaining production qualification

The existing per-peer pending and global association retention policies still
run during maintenance for ordinary ingress. Replenishment now respects the
per-peer limit immediately. Completed retention is separate from the pending
cap. This change does not introduce a new global byte budget for arbitrary
SDK events, reply queues or in-flight replies; event compaction removes the
confirmed peer-sync inventory/payload amplification.

The current probes do not establish bounded production memory under one million
associations, realistic traffic and the server's CPU/RAM/swap limits. A sustained
daemon/SDK run, heap attribution and maintenance completion telemetry remain
necessary before declaring the full #657 production symptom resolved. The
one-second ZeroMQ reply deadline is unchanged, and no transport-timeout fix is
claimed solely from these memory probes.

## Reproduction and saved evidence

Diagnostic source:
`crates/libs/rns-rpc/src/rpc/daemon/tests/issue_657_investigation.rs`.
Run each probe separately so memory measurements start in a fresh process:

```sh
cargo test --release --locked --offline -p reticulum-rs-rpc --lib issue_657_replenishment_respects_existing_pending_capacity -- --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib issue_657_stale_snapshot_cannot_recreate_pruned_marks -- --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_refresh_payload_allocation -- --ignored --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_fanout_copies_queue_inventory -- --ignored --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_postponed_sync_event_retention -- --ignored --nocapture --test-threads=1
```

Baseline local evidence is saved in `/tmp/lxmf-657-evidence/results.json` and
one log per probe. Installed locked dependencies and GCC were used. Corrected
measurements and validation results are recorded below. Hosted CI, live
production profiling and a long-running constrained daemon test have not run.

## Validation of the correction

- `cargo test --release --locked --offline -p reticulum-rs-rpc -p lxmf-sdk -p reticulumd --tests`:
  **1,806 passed, zero failed, 115 ignored, across 42 suites**. This includes
  SDK/ZeroMQ behavior, daemon response-writer tests, real restart recovery tests
  and the issue #369 error-handling scanner. Local TCP/Unix socket access was
  enabled after sandbox-only permission failures in the first attempted run.
- RPC-only `issue_657_` regression run: **eight passed**, with the three memory
  qualification probes explicitly ignored in that default invocation. All five
  fresh-process logical/memory probes were subsequently executed and passed.
- `cargo clippy --release --locked --offline -p reticulum-rs-rpc -p lxmf-sdk -p reticulumd --all-targets --all-features --no-deps -- -D warnings`: passed.
- `cargo fmt --all -- --check`, Git whitespace checks, dependency boundaries and
  module-size checks: passed. `cargo run --locked --offline -p xtask -- architecture-checks` also passed.

The default-feature test build reports pre-existing dead-code warnings for two
BLE runtime-status helpers in the unchanged transport crate. Strict Clippy with
all features passes. Ignored live/stress/reference/hardware tests were not
silently converted to passes; only the three issue-specific memory probes were
run explicitly here. This is affected-package acceptance, not full workspace
or hosted CI acceptance, and no production service was restarted or upgraded.

Corrected raw evidence: `/tmp/lxmf-657-evidence/fixed-results.json` and
`fixed-*.log`; acceptance log: `/tmp/lxmf-657-acceptance.log`.
