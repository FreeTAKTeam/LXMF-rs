# Framework correction and evidence: LXMF-rs #657

Investigation date: 7 October 2026. [Issue #657](https://github.com/FreeTAKTeam/LXMF-rs/issues/657)
reports propagation bookkeeping growth, private memory/swap growth and correlated
ZeroMQ response timeouts after deploying RCH preview.17.

Isolated release-mode probes reproduce framework-side queue expansion,
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

- Fanout selection copies only peer ID, last-seen timestamp and static rank.
  Maintenance selection and rotation also use metadata without inventories;
  ranking, tie-breaking, pool composition and backoff claims are preserved.
- Paginated SDK presence listing sorts borrowed peer records and copies only
  the requested page of presence metadata. Contact enrichment reads only those
  rows; neither complete peer inventories nor the entire contacts map is cloned.
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
- All local/remote `peer_sync` and `peer_unpeer` events omit inventory ID arrays and transferred
  payload arrays before legacy/SDK retention, sinks and broadcasts. Counters,
  peer identity, scheduling and failure information remain. Explicit RPC replies
  keep their detailed inventories and existing schema. The event contract is
  documented in `docs/contracts/rpc-contract.md`. Notification construction
  also skips inventories before copying detailed replies, rather than cloning
  large arrays only to discard them at publication. Event-only failure/removal
  paths do not query inventories that the notification would omit.

Correctness regressions cover repeated/concurrent refills, case-variant peers,
completed-state precedence and completion age, a freed queue slot, stale
snapshots, peer-removal cleanup, read operations
without queue writes after import, event summaries, presence pagination and
fanout ordering. Three
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

The same isolated fixtures were rerun against initial correction `d8015ce`
using the same RPC-only release build configuration as the baseline.

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
confirmed propagation-notification inventory/payload amplification.

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
measurements and validation results are recorded below. Hosted CI is tracked
separately in PR #658; local results do not imply hosted acceptance. No live
production profiling or long-running constrained daemon test was run here.

## Validation of the correction

- `cargo test --release --locked --offline -p reticulum-rs-rpc -p lxmf-sdk -p reticulumd --tests`:
  **1,808 passed, zero failed, 119 ignored, across 42 suites**. This includes
  SDK/ZeroMQ behavior, daemon response-writer tests, real restart recovery tests
  and the issue #369 error-handling scanner. Local TCP/Unix socket access was
  enabled after sandbox-only permission failures in the first attempted run.
- Ten ordinary issue-specific regressions are covered by affected-package
  acceptance. Seven Linux memory diagnostics are opt-in and run separately
  in fresh processes; RSS is reported, not asserted as a portable allocator SLA.
- `cargo clippy --release --locked --offline -p reticulum-rs-rpc -p lxmf-sdk -p reticulumd --all-targets --all-features --no-deps -- -D warnings`: passed.
- `cargo fmt --all -- --check`, Git whitespace checks, dependency boundaries,
  module-size and architecture checks: passed. The unchanged installed xtask
  executable was reused for the follow-up architecture check.
- The exact previously failing `rncp_mixed_runtime_compression_matrix_roundtrips_binary_files`
  interoperability test passed locally against pinned Python Reticulum
  `99de23c040d507e3fefca19e87b182302902725d`. This is separate from hosted HIL acceptance.

The default-feature test build reports pre-existing dead-code warnings for two
BLE runtime-status helpers in the unchanged transport crate. Strict Clippy with
all features passes. Ignored live/stress/reference/hardware tests were not
silently converted to passes; issue-specific memory probes are run explicitly
and reported separately. This is affected-package acceptance, not full workspace
or hosted CI acceptance, and no production service was restarted or upgraded.

Corrected raw evidence: `/tmp/lxmf-657-evidence/fixed-results.json` and
`fixed-*.log`; acceptance log: `/tmp/lxmf-657-acceptance.log`.


## Allocator evidence and the follow-up correction

The [additional live evidence](https://github.com/FreeTAKTeam/LXMF-rs/issues/657#issuecomment-6044412133)
classifies approximately 95.9% of sampled swapped heap-page bytes as wholly
inside validated allocator-free chunks. This is a bounded, non-atomic glibc
2.42 observation, not a complete heap census. It supports allocator retention
as a major contributor, without identifying the initiating allocation stacks
or proving a causal link to transport timeouts.

A local replay uses the reported largest received queue (27,542 identifiers),
tiny synthetic payloads, and 384 real postponed-sync responses. At deployed
source `81344ae1`, its retained events raise RSS from about 35 to 1,274 MiB;
one notification serializes to 1,846,984 bytes. Three polls fail with
`SDK_VALIDATION_EVENT_TOO_LARGE` without a cursor advance. With the initial
correction `d8015ce`, RSS reaches about 47 MiB, the notification is 1,543 bytes,
and the three polls succeed. These are short local synthetic runs; no
production event rate or event-size distribution was measured.

A local-only C interposer samples `mallinfo2` immediately before the replay's
memory readings, without trimming or tuning the allocator. After clearing
both retained event logs, while the test thread and seeded database remain
alive:

| glibc 2.43 local allocator counter | Deployed source | Initial correction |
| --- | ---: | ---: |
| Arena bytes | 890.21 MiB | 37.93 MiB |
| Free arena bytes | 865.42 MiB | 13.14 MiB |
| In-use arena bytes | 24.79 MiB | 24.79 MiB |

Thus this controlled workload reproduces allocator-free retention after large
event copies, and reducing the copies reduces that retention. The allocator's
in-use counter is not an exact live Rust-object census. Counters after test
thread teardown are much smaller and must not be substituted for these active
stage readings. Instrumentation is confined to newly launched local test
processes; it is neither linked into the daemon nor injected into production.

The follow-up removes further full-inventory copies in maintenance selection,
rotation and SDK presence listing. An identical 512-peer fixture owns
1,049,600 synthetic cached identifiers throughout each operation. Maintenance
selection rejects these expired peers; rotation has sufficient headroom and
returns no removals; presence requests just one row. Comparing `d8015ce`
with the follow-up source in separate RPC-only release test processes:

| Operation | Additional peak RSS before | Additional peak RSS after |
| --- | ---: | ---: |
| Maintenance selection | 103.63 MiB | 0.20 MiB |
| Rotation with no removals required | 103.98 MiB | 0.20 MiB |
| One-row SDK presence page | 105.24 MiB | 0.57 MiB |

The presence replay's allocator-free arena bytes after the request fall from
104.92 MiB to 0.37 MiB while its original cached inventories remain alive.
These are measured single-run peaks, not a throughput benchmark or a universal
process-memory budget. The data shows the avoided allocation mechanism; it
does not establish a multi-hour production RAM-plus-swap plateau.

The follow-up also makes unpeer notifications summaries before retention and
broadcast. A normal regression seeds 1,024 completed entries, verifies that
the explicit unpeer reply still contains all identifiers, and verifies that
both event streams remain small and the SDK poll advances. This regression
failed before the change. Existing cleanup tests still verify cleared counts,
bytes, persistent marks and peer removal, while expecting summary events.

Additional diagnostics are in
`crates/libs/rns-rpc/src/rpc/daemon/tests/issue_657_allocation_peaks.rs`:

```sh
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_maintenance_selection_allocation -- --ignored --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_peer_rotation_allocation -- --ignored --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_presence_page_allocation -- --ignored --nocapture --test-threads=1
cargo test --release --locked --offline -p reticulum-rs-rpc --lib diagnose_issue_657_live_shaped_event_retention -- --ignored --nocapture --test-threads=1
```

Local raw evidence is under `/tmp/lxmf-657-evidence/`: `allocator-results.json`,
`followup-baseline-results.json`, `followup-fixed-results.json`, and per-probe
logs. The saved baseline/follow-up executable hashes are respectively
`4708da65df422f6213e552eba66957f68f17154af404c8d88615b16b855747ca`
and `738ca2d148318196a9a89a6194d9c73afb722a96ffeb8c08270e3cfc4923b5a8`.
The source correction does not impose allocator tuning, periodic trimming,
service restarts, retention reductions or propagation-history deletion.


The final follow-up source also passes the large-history replay: 384 retained
notifications, 1,543 bytes per event, approximately 46.7 MiB RSS after publishing,
and three successful non-draining polls. After clearing the logs, active-stage
allocator counters are 33.91 MiB arena, 9.11 MiB free and 24.80 MiB in use.
Evidence: `followup-final-live-shape.json`; executable SHA-256
`7902038afb7218001d600bde227160be509b2a82fb8f838f42b0ec1c51a98553`.
Final acceptance and Clippy logs are `followup-acceptance-final.log` and
`followup-clippy.log` in the same local evidence directory.


## Sustained-resource work, 8 October 2026

A new isolated comparison uses 135,893 authenticated signed/encrypted LXMF
payloads (71,751,504 payload bytes), 1,000,000 durable peer associations across
940 histories, 512 activated peers, and real RCH SDK event polling. RCH also
holds 100,000 announces and 25,000 terminal 4 KiB messages. Synthetic announce
ingress and dashboard requests continue during observation; new independent
message delivery and sustained browser traffic are not yet qualified.

The inventory cutover at `540e449a` leaves SQLite as the runtime association
owner. Public legacy PeerRecord codec fields remain import input, are imported
once under an import/peer/store lock order, and release all vector capacity
only after a successful transaction. Failed import retains its input for retry;
concurrent peer clearing cannot resurrect marks. Explicit inventory replies
continue to read the durable IDs. Normal synchronization and maintenance never
repopulate the legacy vectors.

A 600-second same-fixture comparison against diagnostic-only baseline source
measured daemon RSS plus swap first/last-minute medians of 109.0/126.8 MiB before
and 48.1/58.7 MiB after. Owned inventory buffers changed from about 52 MB to zero.
The cutover retained all 135,893 payloads and 1,000,457 associations after
maintenance/refill, completed 3,807 SDK polls with no poll errors, and verified
graceful exit plus empty cgroups for both services. This short result supports
removing redundant ownership; it does not prove a memory plateau, explain the
remaining allocator/process growth, or establish production recovery.

The `daemon_status_ex.resources` diagnostic reports borrowed inventory capacity
and lower-bound retained event heap bytes. Pipeline diagnostics additionally
distinguish dispatch waiting (including blocking-pool scheduling), synchronous
handler execution, response enqueue/residency, and reply delivery. Each stage
reports active/peak items and owned wire-buffer bytes, completed/failed/timed-out/
cancelled outcomes, and total/maximum elapsed microseconds. Work taking at least
250 ms increments a slow-stage counter and can emit a warning, rate-limited
to one warning per stage per five seconds; suppressed warning counts remain
visible. The existing correlated reply timeout warning remains. RAII guards move with work and queued buffers and release on task abort,
failed send, queue disposal, and shutdown.

These counters do not impose admission limits. Wire-buffer bytes exclude decoded
JSON, temporary envelope encoding copies, allocator metadata and ZeroMQ-internal
buffers. Atomic samples are observational rather than a coherent allocation
census. Only the PUSH/PULL pipeline is currently instrumented; the canonical
ROUTER/DEALER path is not. Event/reply byte budgets, RCH durable-history/cache
changes, complete lifecycle fixes and the final constrained three-hour real
delivery/browser workload remain outstanding.

Local raw comparison evidence is under `/tmp/rch-res-diag1/` and
`/tmp/rch-res-fixed1/`, with immutable binary, harness and fixture hashes in the
manifests. Focused evidence under `/tmp/rch-resource-stability/evidence/` includes
`rpc-issue657-independent-verifier.log`, `zmq-stage-unit.log`,
`zmq-stage-pipeline.log`, `zmq-stage-writer.log`, and `zmq-stage-issue369.log`.

### SDK failure correlation checkpoint

The latest production evidence has requested RCH polls failing while the daemon's
actual poll count remains flat and negotiation/import counts increase. Request
ID alone therefore cannot identify which RPC stalled. Failed SDK ZeroMQ calls
now carry call-local session, request, actual method, stage, elapsed milliseconds,
local-send completion and ignored-unrelated-reply count. This context is added
once to the existing error message and a `sdk_zmq_exchange` detail object. A
pre-existing detail with that name is preserved; in that exceptional case local
context is available only in the suffix, and the existing object is not trusted
as local diagnostics. Method/session representations are bounded to 128 ASCII
characters. New context excludes parameters, tokens, endpoints, response bodies
and unrelated peers' session IDs. It does not sanitize pre-existing error text.

The bookkeeping borrows call-owned identifiers and keeps no successful-call
record, registry, event, log or background task. Send completion means the local
ZeroMQ send returned success, not daemon receipt or handler execution. Existing
deadlines, lock ownership, transport reset boundaries and endpoint-specific
correlation behavior are unchanged. The already-mapped SDK error's semantic
fields and existing details/extensions remain intact. The existing ZeroMQ
`map_rpc_error` still drops raw remote retry/actionability/cause/details/extensions;
that is a separate contract gap and was not changed by these diagnostics.

Focused tests cover both endpoint modes' cold connection and held-lock timeouts,
actual negotiation/import/poll method distinction, post-send ignored-reply
timeouts and successful recovery, envelope/RPC decode failures, DEALER mismatch,
mapped error metadata and a colliding detail key. These diagnostics are partial
attribution only: the frozen RCH delivery experiments still use their recorded
SDK dependency and do not establish production recovery or resource stability.


## 8 October follow-up: durable inventories and poll metadata

The resource-stability follow-up removes the resident handled/unhandled ID
arrays after their one-time transactional import. Explicit inventory RPCs read
SQLite; normal peer records retain only metadata. Tests cover failed imports,
retry, completed-state precedence, import/clear races, concurrent maintenance
and completion, and zero resident inventory capacity after successful import.
ZeroMQ diagnostics now account for queued dispatch, handlers, queued responses
and delivery, including bytes, elapsed time, cancellation and outcomes. These
measure work; they do not implement an aggregate byte-admission limit.
The SDK also attaches bounded, call-local failed-exchange context without
retaining a registry of successful calls or changing deadlines.

An independently verified, short populated run used 1,000,000 associations,
135,893 signed/encrypted propagation payloads, 100,000 RCH announces and 25,000
RCH messages. It completed 130 unique authenticated message/receipt chains,
2,716 polls with no errors, a successful maintenance cycle, history-preservation
checks and graceful cleanup. The 427-second observation is attribution evidence,
not the several-hour memory-plus-swap qualification required by issue #657.

A separately bounded heaptrack run completed the same 130 chains and all
history/maintenance gates with 2,695 polls and no errors. Its sampled live heap
peaked at 12,222,450 bytes and was 6,407,554 bytes at the last sample definitely
before shutdown. This does not explain the production footprint. The report
identified 1,380,352 transient string clones in poll response metadata: 512
configured peer strings copied for each of 2,696 metadata constructions. These
allocations consumed no bytes at the global heap peak; they are churn, not
proof of a retained-memory leak.

`response_meta` now constructs only its existing propagation-policy JSON fields
under the authoritative policy lock, releases that lock, and moves that object
into the response. It no longer clones unrelated peer lists, store paths or
sync-error details. The public full-state reader is unchanged. Three actual RPC
regressions preserve default/custom policy, control-list order, nullable and zero
values, immediate policy changes and the unrelated full state.

The original offline-analysis failure (a parser assuming integer milliseconds
rather than the tool's decimal seconds) remains preserved. The corrected retry
passed independent verification of all 48,853 samples, tool/artifact hashes,
clock alignment and bounded analysis-worker cleanup. No production services,
settings or databases were changed. Issue #657 remains open pending sustained
memory-plus-swap, operational propagation and slow-consumer qualification.


Finish validation: affected RPC/SDK/daemon tests with `zmq-pipeline-rpc` passed
1,829 tests across 42 suites, with 119 explicit opt-in tests ignored. Strict
RPC and SDK all-target/all-feature Clippy, workspace formatting, dependency
boundaries, module-size policy and the issue-369 scanner passed. Independent
execution rechecked the three metadata regressions and an existing node-policy
case against the same compiled executable. The broader all-feature daemon
Clippy attempt remains failed on 64 literal-format diagnostics (62 daemon and
two compatibility-test diagnostics); their source predates the metadata change.
This follow-up does not claim a clean full-workspace lint gate or final resource
acceptance.
