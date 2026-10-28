# P0 closure plan: #663, #608, #609, #625 (evaluated at main `45b5f565`, 2026-10-28)

**Intent:** Close the four open P0 issues with the smallest code and evidence
increments that satisfy each issue's own acceptance text, in one PR per
numbered item, ordered by production risk.
**Current Behavior:** Preview.30 (LXMF-rs `eee8b2c6`) runs with the preview.26
fixes (#695-#697, #700). CPU fell to ~6 % and WAL truncation works, but daemon
anonymous memory still ratchets (357 MiB anon, +1,857 MemoryHigh events in
1 h 44 min), RCH needed 15 startup retries when started without waiting for
readiness, and poll errors stay ~0.002 %. #608 has one acceptance row open
(six carrier families without daemon-level IFAC traces, stale evidence). #609
has three rows open (unlocalised two-relay reverse-delivery timeout, ignored
duplicate-filter differentials not in CI, incomplete shared-instance matrix).
#625 has every automated row open: only `reticulum-rs-core` is built for bare
metal, no seeded-violation test, feature unification re-enables `std`.
**Expected Outcome:** Each issue's remaining checkboxes are green with
evidence, or the residual is named as out of scope with a reason.
**Target-Perspective Output:** Operators get a daemon whose memory is attributed
and bounded per table, a `Type=notify` unit that orders RCH after real
readiness, and CI that proves the core boundary. Maintainers get the #608/#609
matrices closed with named tests.
**Truth Owner:** `docs/status/current-roadmap.md`, the parity matrices and
`docs/performance/rch-resource-fixes-663.md` own posture and evidence. This
package owns execution scope. Prior packages (`issue-663-resource-acceptance`,
`preview26-regression-663-657-678`, `open-issues-2026-10` item 4,
`reticulum-reference-parity-605`) stay as history; do not repeat their work.
**Contract Boundary:** Python Reticulum/LXMF behaviour at the frozen pins,
Python path/cache formats, single SQLite writer, custody/ACK semantics,
propagation quotas, SDK v2 and typed ZMQ, existing HTTP health routes, no new
`unwrap`, 500 LOC modules, workspace dependency boundaries.
**Kill Criteria:** No raised memory limits, no retention deletion beyond
reference TTLs, no relaxed retransmission counts or timeouts, no allocator
change before attribution, no production acceptance claimed from local runs.
**Forbidden Moves:** No RCH edits, no SQLite announce cache, no new
governance process for #625, no rewrite of carrier code for #608.
**Evidence Lane:** Focused regressions per PR, `cargo xtask resource-soak-663`
ingress profile for #663, hosted Verify/HIL runs for #608/#609, per-PR
`architecture-checks` for #625, operator read-only follow-ups.

## Ordered task board

| Item | Issue | Size | Depends on |
| --- | --- | --- | --- |
| 1. Attribute and bound daemon memory | #663 | M | - |
| 2. systemd readiness notification | #663 | S | - |
| 3. Maintenance-pass metrics and quota paging | #663 | M | 1 |
| 1b. Heap-owner gauge and 60-minute allocator soak | #663 | S | 1, 3 |
| 4. IFAC: alias fix, CI wiring, evidence refresh | #608 | S | - |
| 5. IFAC: remaining carrier-family traces | #608 | M | 4 |
| 6. Designated no_std core and seeded checks | #625 | M | - |
| 7. Duplicate-filter differentials into CI | #609 | S | - |
| 8. Two-relay reverse delivery diagnosis | #609 | L | 7 |
| 9. Shared-instance and restart matrix | #609 | M | 8 |

Items 1, 4, 6 and 7 touch disjoint files and may run in parallel. Items 2 and
3 follow 1 only for measurement; their code is independent.

### 1. Attribute and bound daemon memory (#663)

Verified growth sources at `45b5f565`:
- `single_out_destinations` (`crates/libs/rns-transport/src/transport/mod.rs:260`)
  is inserted on every accepted remote announce (`transport/announce.rs:185`)
  and removed only by `close_channel.rs:153`. `add_destination` never inserts
  here, so every entry is a remote identity. Nothing prunes it when the path
  expires (`path_table.rs:322 remove_stale`). ~700 B × every destination seen.
- `peer_crypto` (`reticulumd/announce_ingest.rs:16`, map in
  `bridge_outbound.rs`) grows per announcing destination, never pruned.
- `PathEntry.random_blobs` (`path_table.rs:222-241`): `clone_from` then `push`
  doubles capacity on every refresh; up to 1,280 B retained per path.
- Per-save transient churn (`reticulum_path_store.rs`, `packet_cache.rs:190`
  `hashes()` copies 500k hashes; `export_python_entries` clones every blob
  vector) now runs every 300 s; no trim afterwards on static musl.

Work:
1. Extend `TableOccupancy` (`transport/table_occupancy.rs`) with
   `single_out_destinations`, `random_blob_count`, `fingerprint_map_len` and the
   daemon's `peer_crypto` length (`persistence_metrics.rs` provider). Counts
   only. This turns `memory.process.anon_bytes` into an attributable number in
   production without a profiler.
2. Prune `single_out_destinations` and `peer_crypto` when their destination
   leaves the path table: in the 10 s `iface-cleanup` job
   (`transport/jobs.rs:252`), after `remove_stale`, retain only destinations
   still present in `path_table`, the link table, or locally registered
   destinations (`destinations` map). Emit a removed count in the occupancy
   provider. Expose the removed destinations through the existing announce
   event channel only if a daemon consumer needs it (none found; skip).
3. Fix blob capacity: build the new vector with `with_capacity(len + 1)` and
   truncate, or `shrink_to_fit` after `bounded_random_blobs`. One-liner.
4. Replace `PacketCache::hashes()`'s full `Vec` copy with an iterator that
   encodes directly into the hashlist payload (`runtime_management.rs:115`).
5. Re-run the ingress soak 600 s before/after, same fixture and limits.
   Acceptance: anon slope after minute 1 ≤ 1 MiB/10 min at 1 announce/s;
   occupancy counts × per-entry estimate explains ≥ 80 % of `anon_bytes`.
   If anon still ratchets with flat counts, open the allocator item in
   `preview26-regression-663-657-678/PLAN.md` PR 4 (mimalloc, static musl).

Tests: `announce_table_memory_tests.rs` (blob capacity), a new
`tests_parts/destination_map_prune.rs` (expired path removes destination and
peer_crypto entry; local destination and open link survive), provider snapshot
test in `persistence_metrics.rs`.

### 2. systemd readiness notification (#663 startup retries)

Preview.30 recorded 15 RCH restarts because `Type=simple` returns before the
ZMQ sockets are bound. `RpcDaemon::is_ready` (`rns-rpc/src/rpc/daemon/readiness.rs`)
already knows the moment. Add `sd_notify` without a dependency: if
`NOTIFY_SOCKET` is set, send `READY=1` over a `std::os::unix::net::UnixDatagram`
once `is_ready()` first becomes true (poll from the HTTP loop task or a small
`tokio::spawn` in `main.rs` after the ZMQ loops start); send `STOPPING=1` on
shutdown. Update `docs/runbooks/lxmd-systemd.md` to `Type=notify`,
`NotifyAccess=main`, and document `After=lxmd.service` + `Requires=` for the
RCH unit. ~30 lines plus a unit test with a temporary datagram socket.

### 3. Maintenance-pass metrics and quota paging (#663 file-cache churn)

The 300 s propagation pass still runs two unbounded writer transactions:
the quota phase window-sorts every unhandled row
(`prune_propagation_peer_entries_to_policy.rs:23-87`) and the payload prune
loads all `propagation_entries` metadata when over budget
(`lxmf-propagation/src/storage/retention.rs:18-30`).
1. Add `storage.maintenance` to `operation_metrics.rs`: per phase (expiry,
   quota, payload) last/total duration, rows deleted, and `/proc/self/io`
   `read_bytes` delta per pass. Cheap, and it is the attribution the operator
   reports keep asking for.
2. Quota phase: replace the global `ROW_NUMBER()` with a per-peer loop over
   `SELECT peer, COUNT(*) ... GROUP BY peer HAVING COUNT(*) > ?` (indexed by
   `idx_propagation_peer_entries_state`), deleting each peer's excess with
   `ORDER BY updated_at ASC LIMIT excess` in 512-row pages via the existing
   `peer_entry_prune_pages.rs` helper, writer released between pages.
   Semantics unchanged: same ordering keys, same limits.
3. Payload prune: keep the weight ordering but stop at a bounded candidate
   window: load at most 4,096 oldest rows by `received_at` per page
   (`idx_propagation_entries_retention_cover`), sort that window by weight,
   delete, repeat until under budget. Document the approximation in
   `docs/migrations/`: prioritised destinations are still protected inside each
   window; cross-window ordering is by age.
4. Gate: only ship 2 and 3 if the item 1 soak shows `storage.maintenance`
   quota or payload phases above 1 s or `read_bytes` above 200 MiB per pass.

### 1b. Heap-owner gauge and 60-minute allocator soak (#663 memory gates)

Updated for #709 after stack #708 merged (#704-#707). The 600 s comparisons
show flat transport populations and retained anon after one maintenance pass.
They do not establish a repeated-cycle slope, a live-object leak, or exclusive
allocator causality. The 132.56 MiB table-content estimate omits collection
capacity, native allocations, stacks and allocator overhead.

Execution contract (new branch from merged main):
1. Wrap only the daemon's existing `MiMalloc` allocator in
   `cap::Cap::new(mimalloc::MiMalloc, usize::MAX)` (`cap = "0.1"`), with no
   limit changes or unsafe code in this tree. Register
   `providers.heap.rust_allocated_bytes` in the existing `MetricsProvider`
   registry. It measures outstanding requested Rust allocation sizes, not
   resident, reserved or retained pages. The sampler already records it.
2. Keep measured requested bytes, estimated content and configured SQLite
   cache targets separate. SQLite's default `cache_size=-2000` suggests
   2,048,000 bytes per connection, not a hard total-heap bound or measured
   allocation. Do not credit an assumed 6 MiB owner or add the content estimate
   to the gauge. Report anon minus requested Rust bytes as a signed,
   **unclassified** residual; it cannot identify mimalloc retention. The ≥80%
   resident ownership target remains unresolved unless measurements establish
   compatible, named owners.
3. Run 3,600 seconds of ingress after readiness with the same fixture and
   cgroup limits. Record actual completed maintenance passes and path saves;
   maintenance waits 300 seconds after completion, so counts are not fixed.
   Judge adjacent 10-minute anon medians after the second completed pass:
   growth ≤1 MiB per window, with requested Rust bytes flat across passes.
   Extend the comparison JSON, performance report and progress log with source
   revision, binary hash, window statistics and the observed gate outcomes.
4. If anon grows with flat requested bytes, investigate native allocations,
   stacks, residency and allocator retention; the gauge alone cannot choose
   among them. A scoped local trial of `MIMALLOC_PURGE_DELAY=0` and
   `MIMALLOC_ARENA_EAGER_COMMIT=0` can test the allocator hypothesis before
   changing production configuration. Growing requested bytes justify profiling
   live Rust owners once transient work and intentional capacity are excluded.

Out of scope: production deployment/acceptance, new hard memory limits and
cgroup file-cache reclaim. Preserve the existing metrics provider boundary,
allocator choice and fixture. No second metrics framework or native malloc
interposition.

Tests: registered provider snapshot contains a positive numeric gauge; observe
it on the real `/metrics` endpoint. Requested bytes below resident anon may
occur in this fixture but is not a universal invariant (reserved buffers may
be nonresident). Run focused daemon tests and the common software gates.

References: [Cap requested-byte API](https://docs.rs/cap/0.1.2/cap/struct.Cap.html),
[SQLite cache target](https://www.sqlite.org/pragma.html#pragma_cache_size),
[SQLite memory owners](https://www.sqlite.org/malloc.html).

#### Remaining resident-attribution gate (#709)

The follow-up measures resident mapping ownership: resident anonymous bytes
in kernel-named mimalloc mappings. It does not infer live-object ownership or
allocator retention. Existing requested-byte and full-hour evidence stays intact.
The truth owners remain the comparison JSON, performance report, progress log,
roadmap and parity matrix. The new result replaces only their unresolved
resident-attribution status when its evidence passes.

Use a diagnostic static-musl build of the same daemon source with
`CFLAGS_x86_64_unknown_linux_musl='-DPR_SET_VMA=0x53564d41 -DPR_SET_VMA_ANON_NAME=0'`.
This enables the pinned allocator's existing Linux mapping-name hook; the earlier
musl binary compiled it out. Record source, binary, build flags and allocator
source hashes, and witness actual names/syscalls. No native malloc override,
application unsafe, allocator-policy change or production configuration change.

Extend the existing soak's optional diagnostic sampling: stop its owned daemon
through a pidfd, await all threads stopped, capture raw `smaps` and bracketing
`smaps_rollup`, verify all threads remain stopped, and always resume in `finally`.
The denominator is the sum of every VMA's `Anonymous` bytes, reconciled exactly
with both rollups. Credit only private, inode-zero, device-zero VMAs named exactly
`[anon:mimalloc]`; every other resident anonymous byte stays unclassified.
Do not add Cap requests, huge-page counters, SQLite targets or estimated content.

Run another full 3,600-second ingress fixture with the previously tested two
purge settings, same limits and peer identity. Retain every five-second raw
capture, including transient peaks. After the second successful maintenance pass,
every scheduled capture must exist, reconcile and attribute ≥80%; report the
minimum, per-window owners and the complete unclassified remainder. Also require
the existing duration/cadence/stability gates and zero phase/poll errors. A
missing label, failed stop/resume, racy/missing capture or failed reconciliation
invalidates the evidence rather than permitting selective omission.

Main-agent edits are limited to the soak helper/harness/tests and these existing
evidence/status documents. Read-only PRE, correctness, maintainer and verifier
reviews gate the final result. Production, fresh-message/fanout and multi-hour
qualification remain outside this issue's local measurement acceptance.

### 4. IFAC: alias fix, CI wiring, evidence refresh (#608)

Code: `config_parts/interfaceconfig_sections/normalize_auto_aliases.rs:138,145`
trims I2P `ifac_netname`/`ifac_netkey` through `non_empty_string`, so a
whitespace-only value silently yields plaintext while canonical aliases keep
whitespace. Use the canonical resolver there (one line each) plus a config
test next to `tests_parts/bootstrap_ifac_whitespace_credentials.rs`.
CI: `.github/workflows/verify.yml:338-356` runs only `ifac_udp` and the
lxmf-cli `ifac` case; add the ignored `crates/apps/reticulumd/tests/python_tcp_ifac_lifecycle.rs`
and `python_channel_interop_parts/ifac_kiss_pty.rs` as named steps at the
#608 pin.
Docs: add a family-trace section to
`docs/goals/reticulum-reference-parity-605/evidence/608-ifac.md` listing the 14
qualified `bootstrap_*_family.rs` / `configured_auto_family.rs` tests, refresh
its status and boundary paragraphs, fix `reticulum-parity-matrix.md:892-893`
(I2P aliases are accepted), and note that the CLI-argument TCP listener
(`bootstrap_transport_tcp_server_adapter.rs:44`) has no config record and so
no IFAC by design.

### 5. IFAC: remaining carrier-family traces (#608 row 4)

Open rows from `docs/goals/open-issues-2026-10/TRANSPORT.md:92-97`, no
production code change expected because every carrier already goes through
`iface_parts/ifac_wire.rs` and `interfaces/common.rs::apply_runtime_config`:
1. Local Unix and Local TCP shared instance: extend
   `bootstrap_interface_startup_parts/tests/shared_instance_policy.rs` with
   live non-IFAC policy, restart and error composition.
2. RNodeMulti: `configured_rnode_multi_*` cases in `tests/bootstrap_lora_family.rs`
   reusing its PTY fixtures: vport rotation while active, failed and restarted
   configuration.
3. RNode BLE: daemon status composition and active-child rotation over the
   existing fake backend (`rnode_ble_parts/ifac_worker_tests.rs`); needs
   fake-backend injection on the `startup_lora` BLE path.
4. VR-N76 KISS BLE and generic BLE GATT: one fake-backend IFAC test each
   (auth, packet-loop failure/reconnect, restart) following RNode BLE's private
   factory pattern. `reticulum_ble` stays degraded under #650 and is not
   counted.
Close the row when the hosted Verify run on the exact head lists these tests.

### 6. Designated no_std core and seeded checks (#625)

All with existing tooling, no new process:
1. `Cargo.toml [workspace.metadata.boundaries]`: add
   `no_std_core = ["reticulum-rs-core", "rns-embedded-core", "rns-embedded-runtime", "lxmf-embedded-mini"]`
   and `forbidden_core_deps = [tokio, tokio-util, tokio-serial, clap, ureq, rusqlite, reticulum-rs-zeromq, axum, socket2, if-addrs, serde_json]`.
2. `tools/scripts/check-no-std-core.sh`: read `no_std_core` with `jq` (pattern
   from `check-boundaries.sh:106-112`) and run the thumbv7em `cargo check`
   for each (per-crate feature set; `lxmf-embedded-mini` has no `alloc`
   feature). Expect real compile errors on first run; fix them in the crate,
   not by shrinking the list.
3. `tools/scripts/check-boundaries.sh`: apply `forbidden_core_deps` to every
   `no_std_core` crate plus `lxmf-wire` via `check_forbidden_dependency`,
   skipping `optional` deps for `lxmf-wire`; add a transitive
   `cargo tree -p <crate> --no-default-features --features alloc --target thumbv7em-none-eabi -e normal,build`
   assertion (pattern from `check-embedded-propagation.sh`); add a
   `cargo tree -p rns-embedded-mininode -e features` assertion that
   `reticulum-rs-core` feature `std` is absent. That last one fails today:
   fix `crates/libs/lxmf-core/Cargo.toml` to take `rns-core` with
   `default-features = false` and forward `std = [..., "rns-core/std"]`.
4. Seeded violation: let `BOUNDARY_METADATA_FILE` override the metadata source
   at `check-boundaries.sh:161`; add `tools/scripts/test_check_boundaries.py`
   that injects (a) `reticulum-rs-core -> tokio` and (b) an unlisted workspace
   edge and asserts exit 1 with the expected message; run it beside the unit
   tests at `ci.yml:143-145`.
5. Extend `check_source_imports` (`check-boundaries.sh:255`) to the
   `no_std_core` crates for `tokio|zeromq|rusqlite|serde_json|clap|reticulumd`.
6. Move `run_embedded_core_check` into `run_architecture_checks`
   (`xtask/src/main_parts/run_unused_deps.rs:30-34`) so it runs per PR.
7. Docs: "Protocol core boundary" section in `docs/architecture/overview.md`
   next to lines 53-65 naming the `no_std_core` set, the direction
   core → transport/propagation → runtime/sdk → apps, and the metadata key as
   authoritative; fix the `serde_json` wording at line 55; mark
   `rns-embedded-mininode`, `lxmf-propagation`, `rns-transport` as `std-first`
   in `docs/contracts/sdk-v2-feature-matrix.md` (or drop mininode's
   `cfg_attr(no_std)`).
Out of scope: making `lxmf-wire` or `lxmf-propagation` no_std (#646).

### 7. Duplicate-filter differentials into CI (#609 row b)

All five differentials exist and are `#[ignore]`d with no Verify reference:
`transport/tests/duplicate_filter_reference.rs` (21 contexts),
`tests_parts/duplicate_filter_python_matrix.rs`, `plain_group_packet_duplicates.rs`,
`ordinary_proof_duplicate.rs`, `packet_cache_generations.rs`. Add them as named
steps in `.github/workflows/verify.yml` at the #609 pin `99de23c`. Add the
Rust-vs-Python rotation differential to `duplicate_filter_python_matrix.rs`
(Python `Transport.py:832-834` vs `packet_cache.rs:96-103`). Extend
`crates/apps/reticulumd/tests/packet_replay_process_restart.rs` with Data and
LinkRequest replay across restart. Fix the stale note in
`evidence/609-transport-local-shared.md:1187-1190` (rotation landed in #674).
Decide and document outbound remembering timing (`handler_parts/dispatch.rs:4-9`
remembers before dispatch; Python only on `should_transmit`): align to Python
by remembering after the interface accepts the frame, with a regression in
`packet_cache_generations.rs`.

### 8. Two-relay reverse delivery diagnosis (#609 row a)

Last record: 1 pass, 2 timeouts at 54.9 s for B-to-A after upstream relay
restart (`evidence 1016-1031`), two outbound attempts stuck at progress 0.5,
A's inbox empty, B holding a two-hop path. Test:
`crates/apps/lxmf-cli/tests/python_lxmd_remote_relay_parts/shared_instance_daemon.rs:758`
(`#[ignore]`, HIL-only at the wrong pin `ea98db4`).
1. Run it 10× locally at pin `99de23c` with `RUST_LOG=debug` on both relays
   and the raw packet trace enabled; capture per-hop: path-request handling,
   LinkRequest/LRPROOF routing, Resource advertisement/request/proof sequence.
   Progress 0.5 means the Resource was advertised and never fully requested:
   bisect between (i) relay A's restored path to B pointing at the pre-restart
   interface, (ii) link table entry for the reverse link not recreated after
   restart (`path_parts/unknown_owner_local_link_request.rs`,
   `wire_parts/local_link_proof.rs`), (iii) duplicate filter dropping the
   retransmitted ResourceRequest (item 7's trace will show this).
2. Fix the one cause found; add the minimal regression in rns-transport
   (`tests_parts/`), then unignore the e2e and run it 5× in Verify at the
   right pin (`verify.yml:618-626` pattern).
3. Extend `deep_relay_recovery.py` with reverse in-flight Resource retry and
   edge-relay replacement; keep it in the `interfaces` Verify lane.

### 9. Shared-instance and restart matrix (#609 row c)

Smallest set that closes the matrix's named gaps (`reticulum-parity-matrix.md:229-231, 335-345`):
1. Packet-class × transport-mode × child-kind cells for Data, Proof and
   PathRequest on virtual and accepted TCP/Unix children, extending
   `tests_parts/{enabled,disabled}_transport_shared_child_local_destination.rs`.
2. One live Python/Rust socket differential per shared-child cell via
   `tools/scripts/local-interface-python-shared-smoke.sh`.
3. Process-level Python↔Rust restart traces for cached-vs-scheduled announce,
   expired route and blackholed restore in `packet_replay_process_restart.rs`.
4. Live-socket local-client announce timing in
   `tests_parts/pinned_python_local_announce_schedule.rs`.
Carrier reconnect beyond TCP and discovery family recipes stay with #614/#650
and are recorded as not part of #609 closure.

## Common validation

Per PR: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
the package suites touched, `tools/scripts/check-boundaries.sh`,
`cargo run -p xtask -- architecture-checks`, `tools/scripts/check-module-size.sh`,
`cargo test -p reticulumd --test code_quality_issue_369`. Update the roadmap,
the relevant parity matrix rows and `PROGRESS.md` in the same PR. Post each
item's evidence as a comment on its issue; close an issue only when every
acceptance checkbox has a named test or a documented out-of-scope note.

#### Prospective SQLite pager-cache intervention after tagged RSS failure

The first two complete windows of the tagged baseline are 182.3359375 and
184.6640625 MiB: +2.328125 MiB fails the unchanged 1 MiB gate. At their one/two
central total-anon captures, +0.2109375 MiB is named and +2.1171875 MiB remains
unclassified. This is a paired-count decomposition, not caller attribution.
Preserve the entire completed baseline and its failed composite before a new
candidate. No anchor, threshold or old observation changes.

One reversible diagnostic candidate will test whether unused SQLite pager caches
contribute to that step. After the periodic blocking worker's existing
`maintain_propagation_storage()` succeeds and releases its transaction/daemon
locks, call the pinned safe `Connection::release_memory()` under only the
existing writer mutex, with autocommit required. Avoid `with_control_conn()`:
it also performs emergency WAL reclaim and would mix interventions. The pinned
SQLite has `SQLITE_ENABLE_MEMORY_MANAGEMENT`, so one connection's call can evict
the process-wide group's unpinned pager pages. This is not writer-only ownership,
a byte gauge, prepared-statement cache eviction or a resident-memory measurement.
No manual-RPC behavior, retention/quota/paging semantics, connection count,
allocator selection, unsafe code or production unit changes are included.
This scoped diagnostic candidate extends the initial tooling-only boundary;
shipping the cache policy requires the prospective evidence below.

Use the same full-size fixture, peer identity, limits, purge flags, tagged build
method, frozen capture tooling and 3,600 measured seconds. Keep the original
three-phase second-pass anchor and all conjunctive acceptance checks. Separately
record release attempts, completed calls, errors, lock wait and duration. Require
zero errors, one completed release for each successful periodic all-phase pass,
no unfinished release at the end, and maximum release/lock wait <=100 ms. Missing
metrics, invalid captures, failed resume, changed data semantics, new errors,
OOM/swap reject the candidate. The completed baseline has 444 measured
MemoryHigh events (12 after the second-pass anchor); a candidate may not exceed
either count. File-cache ownership still limits causal comparison. This pressure
regression screen is fixed before candidate acquisition, not inferred as zero.

Before acquisition, the local shipping screen is fixed as follows, compared with
the completed tagged baseline: full-run CPU may rise by at most the greater of
25% relative or one percentage point of a core; logical-read rate may rise by
at most 10%; median reported poll-window P95 must stay <=max(2 ms, 1.25 times
baseline) and every reported P95 <=100 ms. For each original phase, the maximum
observed per-pass duration after its first pass must stay <=max(10 ms, 1.25 times
the baseline maximum after the first pass). Report physical reads and the windows
immediately after release; cached I/O differences do not identify causality.
These are local regression screens, not production SLO qualification.

TDD covers the actual cache-release boundary, failure counters and propagation,
autocommit rejection, and file-backed data/journal validity after release/reopen;
source review establishes ordering and the real timer trial proves actual calls. Inspect the
disabled propagation path and verify it produces no release. Run the #369 scanner,
affected RPC/daemon suites, formatting, boundaries, size/architecture and hosted
checks. PRE review cleared the narrow reversible trial, not a production-policy
claim. If the hypothesis or regression screen fails, retain the trial and reject
the policy; do not randomly retune or repeat until a passing run appears.

The exact baseline inputs, analyzer/comparator hashes and numerical bounds are
frozen before candidate acquisition in `evidence/709-cache-release-criteria.json`.
In particular, CPU <=4.971897% of a core, logical reads <=11,748,193.041 B/s,
median poll-window P95 <=2,000 us and every P95 <=100,000 us; original post-first
phase maxima <=10 ms for expiry/quota and <=10.941633 ms for payload. Every sample
must contain valid release counters; cumulative values may not regress. The final
release attempts/completions must equal all three original phase counts, with
zero errors and <=100 ms maximum release/lock wait. Missing or malformed metrics
fail closed. These additional screens do not change the resident/RSS closure gates.
