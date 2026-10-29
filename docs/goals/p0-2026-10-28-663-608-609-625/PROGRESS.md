# P0 closure progress (#663, #608, #609, #625)

Started 2026-10-28 from main `45b5f565`. One PR per [PLAN.md](PLAN.md) item.

| Item | Issue | State | Branch / PR / head | Evidence |
| --- | --- | --- | --- | --- |
| 1. Attribute and bound daemon memory | #663 | Implemented; memory acceptance open | [PR #704](https://github.com/FreeTAKTeam/LXMF-rs/pull/704) | 1,069 transport tests; provider/capacity regressions; completed soaks fail memory gates |
| 2. systemd readiness notification | #663 | Locally verified | [PR #705](https://github.com/FreeTAKTeam/LXMF-rs/pull/705) | Datagram tests; release process READY/readyz/STOPPING/SIGTERM; real user systemd notify service active then clean exit |
| 3. Maintenance-pass metrics and quota paging | #663 | Implemented; software gates passed; resource acceptance open | [PR #706](https://github.com/FreeTAKTeam/LXMF-rs/pull/706) | 600 s gate: payload 8.08 s / 379 MiB; quota 1.74 ms / 0 reads; bounded-window and failure regressions |
| 3b. Index-driven peer-entry expiry | #663 | Implemented; production acceptance open | branch `corvo/663-indexed-maintenance` | Production residency map: 174.76 MiB DB in 29,276 runs from the expiry rowid walk; query-plan and delete-time regressions; RPC 850 passed / 8 ignored |
| 1b. Heap-owner gauge and 60-minute allocator soak | #709 / #663 | Implemented; resident ownership open | [PR #710](https://github.com/FreeTAKTeam/LXMF-rs/pull/710) | Requested-byte gauge; two full-hour runs with 11 actual maintenance passes each; default RSS stability fails, purge trial passes; ≥80% resident ownership unresolved |
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

The initial execution covered items 1–3; #709 item 1b follows below.
#608/#609/#625 stay on their existing board.
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
At this comparison checkpoint the candidate remained a draft because memory
slope and attribution gates were unmet. Stack #708 later merged it by user
request; it remains unqualified as a memory fix. SQLite/native C allocations
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

[PR #707](https://github.com/FreeTAKTeam/LXMF-rs/pull/707), originally draft and
stacked on #706, contains the measured conditional experiment. It subsequently
merged with stack #708; merge does not establish a qualified memory fix.

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

## Earlier hosted software checkpoint (before stack #708 merge)

CI, Verify and independent interoperability checks pass on exact heads
#704 `e8377ccf`, #705 `026aabf9`, and #706 `c49bac7e`; all three are ready for
review and remain unmerged. Draft #707 keeps the allocator experiment and the
failed memory gates explicit; its hosted checks are separate and still running.
All task-owned soak processes stopped. Raw evidence is preserved in
`target/issue663/`, including the abandoned temporary fixture moved there to
avoid leaving the system temporary-filesystem quota exhausted.

## 2026-10-28 evaluation of the memory gates

The three 600 s runs in `evidence/663-local-resource-comparison.json` show flat
transport populations and an anon step after the single maintenance pass whose
size tracks the allocator (glibc +85 MiB, musl +23 MiB, mimalloc +10 MiB). Flat counts do not identify the growing owner or distinguish allocator retention
from native/runtime growth. Item 1's slope and 80 % gates used one cycle and a
content estimate, so repeated-cycle and ownership acceptance remain unresolved.
Item 1b adds requested Rust bytes and judges the slope on a 60-minute run.
Stack #708 subsequently merged #704-#707; acceptance remains separate.

## Stack #708 merge preparation

The user requested merging all four PRs on 2026-10-28, including the allocator
evaluation. #663's failed memory gates and production qualification remain open.
#707's HIL suites passed all 23 case/profile pairs, but its summary failed twice:
Swatinem/rust-cache restored a partial cache and cleaned downloaded result files
from `target/hil/runs`. Cache restoration now runs before downloading/validating
evidence. The workflow-order regression failed before the reorder and passes
afterwards; no test, evidence or branch-protection requirement was removed.

## #709 execution contract and task board

Stack #708 is merged at `9d03af87398f082abcf892f50e4f1527c22ccef4`.
The earlier allocator-retention conclusion and SQLite bound above are superseded
by the corrected item 1b contract: Cap measures requested Rust bytes; SQLite's
cache target is neither measured ownership nor a total native-heap bound. A
signed anon-minus-requested residual stays unclassified. No production memory
acceptance or issue #663 closure follows from the earlier runs.

Goal/intent: distinguish requested Rust heap growth from resident anon during
repeated fixture maintenance. Plan: `PLAN.md` item 1b. Truth owners: this progress
log, comparison evidence, performance report, roadmap and parity matrix.
Boundary/cutover: the daemon binary replaces direct MiMalloc with unlimited Cap;
the existing provider registry owns publication, with no parallel counter.
Acceptance: live endpoint, 3,600 s trace, completed-pass/window statistics and
qualified ownership outcome. Kill criteria: invalid provenance, early exit or
missing metrics invalidates the run. Forbidden moves: unsafe/native interposition,
new memory limits, assumed SQLite ownership or production acceptance claims.

Main agent owns sequential edits to daemon allocator/provider/registration,
dependency lock and the existing evidence/status documents; other code and the
fixture are excluded. Input is merged stack #708 plus item 1b. Each row depends
on the preceding implementation/evidence; read-only review is parallel safe.
Output is a tested gauge and reviewable measured report, not a new runtime path.

Ordered work on `corvo/709-heap-owner-gauge`:

| Task | State | Evidence / gate |
| --- | --- | --- |
| PRE ownership/API review | complete | Independent review confirmed safe unlimited Cap API and corrected attribution/cycle-count assumptions. |
| Gauge and focused regression | complete | Checkpoint `2473e7ef`; red missing-provider regression then green; unconditional bootstrap registration. |
| Immutable binary and 3,600 s ingress | complete | Static-musl SHA-256 `d721b500afb530a8532d554bbf85cb68c72fcdcd2e2b28a20cb9e32c51fe4b05`; same fixture/limits, raw `target/issue709/heap-3600`. |
| Conditional purge-settings trial | complete | 3,603.656 s; four complete windows, RSS changes 0.000/+0.463/−0.266 MiB and requested median range 0.193 MiB pass. |
| Evidence and software gates | complete | Both full-run JSON results, raw bundle, 31 member hashes and exact fresh-extraction replay verified; software gates below. |
| POST reviewer / maintainer / verifier | complete | Independent final review verified all 31 member hashes, three exact analysis replays, provenance, arithmetic, disclosures and links; no remaining actionable findings. |

The focused daemon suite passes with 693 binary tests, 31 library tests and the
#369 diagnostics scanner. The initial long workspace TMPDIR exceeded Unix
socket path limits in three tests; the same suite passes with `/tmp/l709`.
No-default-features check, formatting, boundaries and module-size checks pass.
`cargo audit` finds no vulnerability and warns about pre-existing unmaintained
`serial` 0.4.0; `cargo deny check` fails that existing RUSTSEC-2017-0008 policy
finding while bans, licenses and sources pass. No allowlist was weakened.
Independent implementation correctness/maintainer review found no issues;
real bootstrap and both completed full-hour runs are verified below. PR: #710.

Workspace all-target/all-feature Clippy, architecture checks and a locked
Rust 1.88.0 daemon build check pass. Real `/metrics` after bootstrap reports the
new heap provider: first-minute median 152,058,133 requested bytes versus
206,485,504 anon bytes. This is a magnitude comparison, not resident attribution.
The independent analysis review found two missing safeguards: phase invocation
counts can include failures and sparse samples cannot prove a window median.
The analysis now requires zero phase errors, first/last samples within 10 seconds
of window edges and every internal gap ≤10 seconds; sparse-window/boundary and
signed-residual self-checks pass; combined evidence review is recorded below.

A separate real static-musl startup without transport or ZeroMQ publishes heap
219,915 requested bytes (11,882,496 anon bytes in that observation) and exits 0
on SIGINT. The hour run's second all-phase completion was first sampled at
625.656 seconds, with zero phase errors. Early SDK event-log growth approaches
its 1,024-entry bound; duplicate-filter packet hashes also grow under ingress,
so the selected flat recipient/path counts do not imply every owner is flat.
All 28 hosted checks at `2473e7ef` completed: 27 successful and one skipped.

The verifier also reproduced a missing-entire-window case that could falsely
compare nonadjacent windows. Coverage now requires every expected complete
600-second window in sequence, with dense-pass and missing-window self-checks.
The exact measured code/binary and raw/analysis hashes are recorded for the
completed default and purge evidence. No source code changed after the passing
hosted checkpoint.

Earlier checkpoint at 1,837 sampled seconds: two complete post-second-pass windows have valid
coverage and zero phase errors. Anon medians 218.516 →232.029 MiB grow
13.514 MiB, failing the ≤1 MiB/window gate; requested medians 147.446 →147.511
MiB differ by only 0.064 MiB. The control was still running at that checkpoint,
activating
the conditional local allocator-hypothesis trial after the control ends:
`MIMALLOC_PURGE_DELAY=0 MIMALLOC_ARENA_EAGER_COMMIT=0`, same immutable binary,
fixture and limits, another 3,600 measured seconds. No production unit or runtime
code changes are justified by these preliminary measurements. Flat requested
bytes do not identify mimalloc as the owner of the RSS difference.

Default control completed with 720 samples over 3,604.214 measured seconds and
max gap 5.010 seconds. Eleven successful all-phase passes and eleven path saves
were observed. Full-window anon changes +13.514/−4.930/+2.430 MiB fail the stated
gate; requested medians span only 0.193 MiB and pass flatness. The partial final
578.558 seconds are reported separately. 35,974 recorded polls and all phases
have zero errors, while the existing reject-profile/parser warnings remain.
Code/binary identity and raw/method/environment SHA-256 values are in the extended
comparison JSON. The ≥80% resident-owner target remains unresolved; neither the
signed residual nor SQLite settings are measured owners. The scoped purge trial
started after control cleanup and is complete, as recorded below.

The first purge attempt was intentionally stopped and preserved as
`target/issue709/purge-unmatched-peer`: its fresh peer admitted a second
destination only at 671.324 seconds, versus 125.116 seconds in the control.
Ingress was therefore unmatched through the second-pass anchor. It is not used
for a matched stability/allocator comparison. The replacement reuses only the
control peer's test identity in a fresh peer directory, keeping its database new;
public destination hashes and actual ingress must match before interpreting the
purge result. No private identity file enters the review bundle. The original
fixture and completed control are unchanged.

Independent review reproduced all default analysis fields and verified raw,
method and binary hashes. Stale pending statements were corrected. A compressed
review bundle preserves raw JSONL/logs, selected environment values, frozen
method/harness snapshots and a manifest so fresh checkouts can recompute results;
local target paths identify acquisition locations rather than published artifacts.

Replacement peer public hashes match the default. Two destinations were naturally
learned at 245.557 sampled seconds; subsequent admitted hashes increase at about
two/s, matching control steady ingress before the second maintenance pass. A
later warmup probe was incorrectly JSON-encoded for the framed-MessagePack HTTP
RPC endpoint and rejected with HTTP 400: no request_path RPC was dispatched, and
the route had already been learned. The single additional HTTP error is recorded
separately from SDK poll/maintenance errors; no retry or state-changing warmup
request was sent. This setup difference is included in the final evidence.

Replacement `purge-3600` completed 3,603.656 measured seconds with 719 samples,
maximum gap 5.075 s and eleven successful invocations per maintenance phase and
path saves. Four complete windows have 120/120/120/119 samples with valid cadence;
anon medians 184.979/184.979/185.441/185.176 MiB pass the ≤1 MiB growth gate.
Requested medians 147.439/147.503/147.632/147.632 MiB pass the 0.193 MiB range.
The 582.235 s tail is reported separately. All 35,973 SDK polls and all phases/saves
have zero errors; one rejected HTTP probe remains explicitly recorded. Steady
hash rates after the second pass are 2.000/s in both runs. The trial has no
MemoryHigh/max/OOM/swap delta, but zero physical-read bytes versus the earlier
control means cached I/O/pressure conditions are not isolated. Both settings and
both daemon processes change together. No production default is changed and the
≥80% resident-ownership gate remains unresolved; #709/#663 are not closed by
this local evidence.

The committed `evidence/709-heap-soak-evidence.tar.gz` is 1,087,840 bytes,
SHA-256 `272d6e3f23267b8e4bc1450296c7da8577a4dd1357b76fae36f06e41c56fa301`.
All 31 per-member hashes and exact analysis replay for both full runs and the
discarded setup pass from a fresh extraction using only bundled files. Raw
traces/logs, selected environment values, method/harness snapshots and the
standalone bootstrap observation are recoverable; private identities, databases
and binaries are excluded. The comparison JSON maps original acquisition paths
to portable members and records source/binary/client hashes and gate outcomes.
