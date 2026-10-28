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

Added 2026-10-28 after items 1-3 (#704-#706) and the draft allocator
evaluation (#707). The 600 s ingress runs recorded in
`evidence/663-local-resource-comparison.json` show every transport population
flat (100,002 recipients, 100,128 blobs, zero removals) while anon still steps
up after the single maintenance pass, and the step size tracks the allocator:
glibc +85 MiB, static musl +23 MiB, musl + mimalloc +10 MiB. That is allocator
retention of transient maintenance work, not a live-object leak. Item 1's two
gates could not be decided by that evidence: a 10-minute run holds the warm-up
and exactly one cycle, and the 80 % attribution compares a hand estimate of
content bytes against total anon, which also holds allocator slack, SQLite,
stacks and zmq buffers that the estimate cannot name.

Facts that bound the owners without new code: no store sets `PRAGMA
cache_size`, so the three SQLite connections hold at most the 2 MiB default
page cache each; SQLite is therefore not a large anon owner. The remainder
after the content estimate is Rust-heap allocator slack plus runtime, which
needs one measured number, not a profiler.

Work (on the #707 branch; it stays draft until step 3 is recorded):
1. Rust-heap gauge: wrap the daemon allocator in `cap::Cap<MiMalloc>`
   (`cap = "0.1"`, safe `allocated()` API, no `unsafe` in this tree) and
   expose `providers.heap.rust_allocated_bytes` through the existing
   `MetricsProvider` registry in `persistence_metrics.rs`. Two lines in
   `main.rs`, one provider. The soak sampler already stores every `/metrics`
   scrape, so no harness change is needed.
2. Attribution by subtraction, in the report: `anon_bytes` =
   `rust_allocated_bytes` + allocator retention (anon minus Rust allocated minus
   the 6 MiB SQLite bound) + runtime. Compare `rust_allocated_bytes` with the
   132.56 MiB content estimate: if they agree within 10 %, the estimate is
   validated and the rest is a named owner (allocator retention). Replace
   item 1's ≥ 80 % estimate gate with: named owners (Rust allocated + SQLite
   bound) explain ≥ 80 % of anon, measured.
3. 60-minute mimalloc soak, same fixture and limits as the comparison
   (`cargo xtask resource-soak-663 -- run --profile ingress --duration-secs
   3600`): six maintenance passes and twelve path-table saves. Pass: anon
   median of each 10-minute window after the second maintenance pass grows
   ≤ 1 MiB per window, and `rust_allocated_bytes` is flat across passes. The
   slope gate is judged on this run, not on 600 s. Record the run in the
   comparison JSON and `rch-resource-fixes-663.md`.
4. Only if step 3 fails with flat `rust_allocated_bytes`: the retention is
   inside mimalloc; try `MIMALLOC_PURGE_DELAY=0` and `MIMALLOC_ARENA_EAGER_COMMIT=0` by
   environment in the unit file before touching code. If it fails with growing
   `rust_allocated_bytes`, a live owner exists and a heap profile is justified.

Out of scope here: the cgroup file-cache reclaim (memory.high events) is
driven by the fixture's 1.2 GiB database and 1 GiB WAL being read by the prune
passes; it is addressed by production incremental pruning, not by this item.

Tests: provider snapshot test asserting `rust_allocated_bytes` is positive and
below `anon_bytes`; no other code.

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
