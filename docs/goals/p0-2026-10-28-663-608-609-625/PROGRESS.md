# P0 closure progress (#663, #608, #609, #625)

Started 2026-10-28 from main `45b5f565`. One PR per [PLAN.md](PLAN.md) item.

| Item | Issue | State | Branch / PR / head | Evidence |
| --- | --- | --- | --- | --- |
| 1. Attribute and bound daemon memory | #663 | Implemented; memory acceptance open | [PR #704](https://github.com/FreeTAKTeam/LXMF-rs/pull/704) | 1,069 transport tests; provider/capacity regressions; completed soaks fail memory gates |
| 2. systemd readiness notification | #663 | Locally verified | [PR #705](https://github.com/FreeTAKTeam/LXMF-rs/pull/705) | Datagram tests; release process READY/readyz/STOPPING/SIGTERM; real user systemd notify service active then clean exit |
| 3. Maintenance-pass metrics and quota paging | #663 | Implemented; software gates passed; resource acceptance open | [PR #706](https://github.com/FreeTAKTeam/LXMF-rs/pull/706) | 600 s gate: payload 8.08 s / 379 MiB; quota 1.74 ms / 0 reads; bounded-window and failure regressions |
| 4. IFAC alias fix, CI wiring, evidence refresh | #608 | Not started | | |
| 5. IFAC remaining carrier-family traces | #608 | Not started | | |
| 6. Designated no_std core and seeded checks | #625 | Not started | | |
| 7. Duplicate-filter differentials into CI | #609 | Not started | | |
| 8. Two-relay reverse delivery diagnosis | #609 | Not started | | |
| 9. Shared-instance and restart matrix | #609 | Not started | | |

## Baseline carried forward (2026-10-28)

- Preview.30 operator sample: daemon anon 357 MiB, cgroup RAM 755 MiB at a
  768 MiB high, +1,857 MemoryHigh events in 1 h 44 min, CPU 6.5 %, persistence
  21 runs per 1 h 44 min, WAL 39 MiB with one truncation, poll errors ~0.002 %,
  15 RCH startup retries when started before daemon readiness.
- #608: 14 of 20 carrier families qualified locally; six open rows listed in
  `docs/goals/open-issues-2026-10/TRANSPORT.md:92-97`.
- #609: two-relay reverse delivery 1 pass / 2 timeouts at #629 head; five
  duplicate-filter differentials ignored and absent from Verify.
- #625: only `reticulum-rs-core` built for thumbv7em; no seeded-violation
  test; `lxmf-core` re-enables `rns-core/std` for every dependant.

## #663 execution scope and decisions

Only items 1–3 are in this execution; #608/#609/#625 stay on their existing board.
The truth owners, Python wire/cache format, SQLite writer, TTLs, memory limits,
SDK v2 and typed ZMQ boundary stay as specified in PLAN.md. No allocator or
retention-policy change is justified before measurement. Main-agent implementation
is sequential, with a read-only explorer and POST/correctness/maintainer review.

| Task | Owner / input | Allowed files | Output / evidence | Dependency / parallel safety |
|---|---|---|---|---|
| 1 | transport + announce worker; existing cache lifecycle | destination/path/hash persistence, provider, focused tests, status/evidence | bounded lifetime/capacity; occupancy; 600 s before/after | none; main-agent sequential |
| 2 | daemon loops; `RpcDaemon::is_ready()` | notification helper, startup/shutdown integration, runbooks, tests, status/evidence | READY/STOPPING datagrams and real service ordering | item 1 checkpoint; independent code |
| 3 | `MessagesStore` operations; instrumented soak | existing operation metrics + phase boundaries, tests, status/evidence | phase durations/deletions/read-byte deltas; paging only after >1 s or >200 MiB gate | item 1; measurement before paging |

Forbidden: RCH/other-issue edits, raised memory limits, new announce database,
weakened receipt/TTL/quota semantics, production acceptance from local evidence.
Plan corrections: prepared SINGLE proof recipients and endpoint/backchannel links
must also survive cleanup; incoming links target our local destination, so retain
the identified peer's same-name destination. Blob waste is bounded 2x, not
exponential. Live fingerprint occupancy reads the existing persistence state;
the earlier save-report gauge alone missed restored entries before the first save.

Item 2 runbook deviation: the existing lxmd-systemd example starts a launcher,
so making that unit `Type=notify`/`NotifyAccess=main` would reject the child's
notification. A direct-daemon notify unit is documented there and in the daemon
operations guide; the combined launcher stays simple. RPC bind completion is
also gated because `is_ready()` alone checks only ZeroMQ, and is initially true
when no ZeroMQ endpoint is configured. SIGTERM uses the graceful SIGINT path.

The measured gate justified payload paging, not quota paging. Quota is left atomic
because its 1.74 ms / zero-read pass is not the driver; this narrows item 3 to the
measured source and avoids a needless concurrent-write semantic change. Payload
weight ordering applies inside each oldest 4,096-row candidate window, with age
ordering across windows as approved and documented in the migration note.

Conditional follow-up from preview26-regression-663-657-678 PR 4 is now active:
the 600 s attribution run retained anonymous memory after transient work with flat
route/recipient populations. No memory acceptance claim is made for items 1–3.

## Conditional allocator evaluation

The 600 s instrumented run shows a retained anonymous-memory step with flat table
populations; it does not by itself prove repeated ratcheting. Following item 1's
conditional pointer, the existing preview.26 PR 4 is reopened as a measured
evaluation. The candidate selects mimalloc only in the reticulumd binary (no
native malloc override), with no tuning options. A static-musl control containing
payload paging and a same-fixture allocator candidate completed the comparison.
The candidate remains a draft evaluation because memory slope and attribution
gates are unmet; no merge/release is proposed as a qualified memory fix. SQLite/native C allocations
remain system-allocated. No production or >=80% ownership claim is inferred.

## Local software validation

Final suites: transport 1,069 passed / 13 ignored, RPC 849 / 8 ignored, daemon
692 passed, propagation 6 passed, and the #369 diagnostics scanner passed.
Workspace all-target/all-feature Clippy, formatting, boundaries, module-size and
architecture checks passed. The live fingerprint accessor initially exceeded the
500-line policy; persistence-state methods now live in a 20-line child module.
Focused path save/restore tests passed after that extraction. Test temporary files
use workspace TMPDIR because the system temporary filesystem exhausted its quota.

The 600 s paged static-musl control completed with flat populations but anon
267.08 -> 289.90 MiB and +280 MemoryHigh events. Payload total duration remains
9.10 s / 386 MiB reads; only candidate materialisation/deletion is bounded per
writer transaction. The full indexed size sum is repeated per page to include
concurrent writes. Memory acceptance and a total-latency improvement remain open.

## Allocator evaluation outcome

Draft [PR #707](https://github.com/FreeTAKTeam/LXMF-rs/pull/707), stacked on #706,
contains the measured conditional experiment; it is not proposed as a qualified fix.

The completed 600 s pair shows control anon 267.08 -> 289.90 MiB versus
mimalloc 201.99 -> 211.87 MiB. After minute one, normalised OLS growth is
34.99 versus 16.45 MiB/600 s: both fail the <=1 MiB gate. High events were
+280 versus +254; max/OOM and swap remained zero; SDK polls were 5,996/5,997
with zero errors. File-cache starting ownership differed, so high/read-byte
changes are not isolated allocator causality. Logical reads rose 5.64 ->
10.04 MiB/s, so this is not a demonstrated overall resource improvement.

The conservative 132.56 MiB table content estimate explains about 66% of the
candidate's first-minute anon, below 80%. Further heap ownership measurement
and repeated cycles remain unresolved within #663; a ten-minute run cannot
establish multi-hour working-set/delivery acceptance. The machine-readable
[evidence](evidence/663-local-resource-comparison.json) includes binary hashes,
source revisions, counts and phase metrics. No production deploy or issue closure.

## Hosted software checkpoint

CI, Verify and independent interoperability checks pass on exact heads
#704 `e8377ccf`, #705 `026aabf9`, and #706 `c49bac7e`; all three are ready for
review and remain unmerged. Draft #707 keeps the allocator experiment and the
failed memory gates explicit; its hosted checks are separate and still running.
All task-owned soak processes stopped. Raw evidence is preserved in
`target/issue663/`, including the abandoned temporary fixture moved there to
avoid leaving the system temporary-filesystem quota exhausted.
