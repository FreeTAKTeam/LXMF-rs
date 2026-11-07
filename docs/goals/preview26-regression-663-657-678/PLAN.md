# Preview.26 resource and control regression (#663 / #657 / #678)

**Intent:** Remove announce-triggered snapshot amplification, report truthful control readiness, and keep WAL maintenance off the broker writer.
**Current Behavior:** Preview.26 failed resource and polling acceptance; the approved investigation and ordered increments are reproduced below.
**Expected Outcome:** Dirty route state saves at most once per configured interval, configured ZMQ listeners gate readiness, broker control work has bounded waits, and checkpoint maintenance releases the writer.
**Target-Perspective Output:** Production-shaped ingress counters and latency evidence under the existing cgroup limits, plus a daemon-only prerelease for operator testing.
**Truth Owner:** `docs/status/current-roadmap.md`, the parity matrices, and `docs/performance/rch-resource-fixes-663.md` own posture and evidence; this package owns execution scope.
**Contract Boundary:** Preserve SQLite custody/ACK semantics, propagation quotas, Python path/cache formats, SDK v2 and typed ZMQ, and existing HTTP health/liveness behavior.
**Cutover:** Replace the 2 s persistence debounce with a dirty timer; remove periodic file reverification; gate readiness on configured listener binds; cache durable capability; use a dedicated checkpoint connection.
**Displaced Path:** Per-announce snapshot saves, unconditional maintenance saves, SQL capability probes and writer-held maintenance checkpoints.
**Kill Criteria:** Do not raise memory limits, weaken custody or quotas, alter RCH timeouts, add an allocator before ingress evidence, or claim production acceptance from local smoke checks.
**Forbidden Moves:** No RCH edits, SQLite announce-cache redesign, protocol changes or unrelated cleanup.
**Evidence Lane:** Focused regressions, repository validation, paired 600 s ingress runs, and operator metrics. PR 4 is conditional on measured residual pressure.

## Execution board

| Task | Owner | Input / allowed files | Forbidden files | Output / evidence | Dependency | Parallel safe |
| --- | --- | --- | --- | --- | --- | --- |
| 1. Persistence | Main; Luna exploration/review | Approved PR 1 transport/daemon files, tests, status docs | RCH; unrelated modules | Dirty timer, seeded fingerprints; tests and ingress comparison | Goal package | Read-only agents only |
| 2. Readiness/control | Main; Luna exploration/review | Approved PR 2 RPC/daemon files, callers, tests, status docs | RCH; wire contracts | Bind-gated readiness, cheap capability, bounded admission/waits; regressions | 1 | Read-only agents only |
| 3. WAL | Main; Luna exploration/review | Approved PR 3 store/WAL files, tests, metrics, status docs | RCH; custody semantics | Dedicated connection, rate limit, frame diagnostics; tests | 2 | Read-only agents only |
| 4. Conditional follow-up | Main; Luna review | Allocator/prune surfaces only if evidence warrants | Speculative changes | Residual ingress measurement justifies change or deferral | 1 measurements | No |

## Approved implementation brief

### Source plan: preview.26 regression on #663 / #657 / #678

## Context

Release `reticulumd-test-0.15.0-rch663-678.1` (= PR #694 = this branch's two commits, merged as
`392fc1b9`) was deployed as RCH preview.26 on 2026-10-26 and **failed acceptance within 24 minutes**:
daemon cgroup RAM pinned at MemoryHigh (768 MiB), +8,910 memory.high events, memory PSI ~60 %,
~40 % of one core while idle, anon +87 MiB, file cache ~512 MiB, durable broker unhealthy, 59 poll
errors, and two `sdk_broker_negotiate_v1` timeouts (3 s) right after `/readyz` returned 200.
Disk headroom (#678) improved; cache GC itself is not implicated.

Code reading identified four causes. The first is dominant and explains why the local "idle"
soak looked fine (zero announce ingress) while production, which receives ~1 announce/s
(~80k cache files/day), does not.

**R1. Save-per-announce amplification** — `announce_worker.rs:61` triggers a path-table save for
every inbound announce, debounced to 2 s (`announce_persistence.rs:8`). With ~100k routes each
save (`reticulum_path_store.rs:106-241`): exports all 100k entries and clones every
`random_blobs` Vec (`path_table.rs:394`), builds a full `rmpv::Value` tree and rewrites
`destination_table` + `tunnels`, computes SHA-256 for all 100k packets
(`reticulum_announce_cache.rs:40`), rewrites `packet_hashlist.raw` (two ~16 MB buffers,
`runtime_management.rs:115`), and every 300 s re-opens and re-reads all 100k cache files
(`ANNOUNCE_CACHE_VERIFY_INTERVAL`, ~400 MiB of 4 KiB pages). The maintenance tick's first
`interval` tick fires at t≈0, so this full save+verify also runs during RCH's startup negotiate.
Allocator: no global allocator, no trim; the per-save churn ratchets anon memory.

**R2. Readiness and control-lane starvation** — `/readyz` is a constant 200
(`rns-rpc/src/rpc/http_parts/module_prelude.rs:71`); the ZMQ loops are spawned concurrently
(`main.rs:216-237`) and a bind failure only logs. `sdk_broker_negotiate_v1` has one blocking
dependency: `durable_broker_enabled()` runs a `sqlite_master` query under the single shared
read-connection mutex with a 5 s busy timeout (`broker.rs:7`, `sdk_broker.rs:92`), longer than
RCH's 3 s budget. Control lane admits 8 requests but reserves 16 MiB per reply from a 96 MiB
budget (`admission.rs:6-9,63`), so only ~5 control requests fit; overflow is dropped with a
debug log unless one of 2 rejection slots is free (`router.rs:64-81`).

**R3. Writer-mutex hogs** — WAL maintenance (60 s) holds the single writer mutex for the whole
PASSIVE checkpoint of a 550 MiB WAL (`storage/broker/wal_maintenance.rs`, via
`with_control_conn`). `sdk_broker_resume_v1` → `broker_command` waits on the FIFO writer
queue with no deadline (`broker.rs:10-15`), behind announce ingest (two writer round trips per
announce, `announce_ingest.rs:72-105`). Production WAL is stuck at 546 MB → truncation never
succeeds.

**R4. Unbounded propagation maintenance phases** (secondary, already partly bounded by #694):
quota phase and payload prune run as single unbounded writer transactions
(`prune_propagation_peer_entries_to_policy.rs:23-87`, `lxmf-propagation/src/storage/retention.rs`).

## Step 0 — put this plan in the repo for Codex

Create `docs/goals/preview26-regression-663-657-678/PLAN.md` following the house goal-package
format used by `docs/goals/issue-663-resource-acceptance/PLAN.md` (header block: Intent,
Current Behavior, Expected Outcome, Target-Perspective Output, Truth Owner, Contract Boundary,
Cutover; then numbered items with files, tests and evidence), carrying the content below, plus
an empty `PROGRESS.md` table with one row per PR. Link it from `docs/status/current-roadmap.md`
next to the existing 663 package. Commit on this branch (`corvo/fix-678-663`) as a docs-only
commit so Codex can pick it up; no code in that commit.

## Approach: four small PRs on top of `origin/main`, in this order

Branch base: `origin/main` (`883e01b1`), not this branch (its commits are already merged).

### PR 1 — timer-driven, dirty-only path persistence (fixes R1; biggest win)

Files: `crates/apps/reticulumd/src/bin/reticulumd/announce_persistence.rs`,
`announce_worker.rs`, `crates/libs/rns-transport/src/transport/reticulum_path_store.rs`,
`reticulum_announce_cache.rs`, `persistence_metrics.rs`.

1. Replace the 2 s debounce with a dirty flag + minimum save interval. Keep the mpsc trigger
   (so metrics `triggers_total`/`dropped_triggers_total` stay), but the worker only flushes
   when `dirty && now - last_save >= RETICULUM_PATH_TABLE_SAVE_MIN_INTERVAL` (300 s; env
   `LXMD_PATH_TABLE_SAVE_INTERVAL_SECS` like the WAL timer). Shutdown flush unchanged
   (`main.rs:168,281`). Python Reticulum persists tables on a multi-hour timer and on exit, so
   losing ≤5 min of new routes on a crash is reference-compatible.
2. The 30 s maintenance tick runs **only** `clean_reticulum_announce_cache_batch`; on
   `Ok(None)` it does nothing (today it runs a full flush, `announce_persistence.rs:78-81`).
   Skip the immediate first tick (`interval_at(now + interval)`), as WAL maintenance does.
3. Drop the 300 s full re-verification (`ANNOUNCE_CACHE_VERIFY_INTERVAL`): set
   `verify_existing` only when `cache_state.files` is empty (first save after restart).
   Nothing else writes these files; restart recovery already validates them.
4. Seed `cache_state.files` during `restore_reticulum_path_table_report` (it already reads
   every referenced file): compute `cached_announce_fingerprint(iface, packet)` per restored
   entry and store it, so the first save after restart is `SkippedKnownBytes` for all 100k
   records instead of a 100k-file re-read during RCH startup. If this touches more than the
   restore loop + one helper, defer it and note it.
5. Keep per-save cleanup batch; with per-announce saves gone, GC rate is the 30 s tick
   (512 entries ≈ 17 files/s ≫ ~1 file/s ingress). Leave batch/grace constants as they are.

Tests: `announce_cache_maintenance.rs` (tick does not flush; burst of triggers → one save per
interval); `reticulum_path_store/persistence_tests.rs` (second save with populated fingerprints
performs zero file opens; restore seeds fingerprints). Update `PROGRESS.md`,
`rch-resource-fixes-663.md`, roadmap/parity rows in the same PR.

Skipped on purpose: streaming msgpack encoding of the snapshot, incremental per-announce cache
file writes, changing the hashlist format. One snapshot per 5 min makes those unnecessary.

### PR 2 — truthful readiness and cheap control path (fixes R2)

Files: `main.rs`, `rns-rpc/src/rpc/http_parts/module_prelude.rs`,
`rns-rpc/src/storage/messages_parts/messagesstore_sections/broker.rs`,
`sdk_domain_snapshot_key.rs` (store open), `reticulumd/zmq_rpc_loop/admission.rs`, `router.rs`.

1. `/readyz` → 503 until every configured ZMQ loop has bound. One `Arc<AtomicBool>` per loop
   (or an `AtomicUsize` of bound loops vs expected) on `RpcDaemon`, set right after bind in
   `run_zmq_router_loop_until` / `run_zmq_rpc_loop_until`; a bind error leaves it unset and
   logs at error. `/healthz` and `/livez` unchanged.
2. Cache the durable-broker flag: `AtomicBool` on `MessagesStore`, set at open
   (`restore_profile`) and in `enable_durable_broker`; `durable_broker_enabled()` reads the
   atomic. Removes the read-connection mutex + SQLite from negotiate, resume, fetch, status
   and `broker_owns_message`.
3. Control lane reserves a control-sized reply (e.g. 1 MiB) instead of
   `ZMQ_RPC_MAX_ENVELOPE_BYTES`, so all 8 control slots are usable; bump rejection slots so
   overflow answers `SDK_TRANSPORT_ZMQ_BUSY` instead of silently dropping; log drops at warn,
   rate-limited. Verify in `zmq.rs` that control replies are actually bounded by that size
   (poll events page bound) before picking the number.
4. `broker_command`: `recv_timeout` (2.5 s, under the client budget) returning a typed
   busy/timeout error; the queued job still executes (resume/fetch/ack are replay-safe, see
   `lxmf-sdk .../recovery.rs:50-60`). Record the timeout in `zmq_metrics` failure context.

Tests: readiness 503-before-bind unit test on the HTTP route with a stub daemon; admission
test that 8 control leases fit; `broker_command` timeout test with a parked writer.

### PR 3 — keep the writer mutex free during WAL work (fixes R3)

Files: `rns-rpc/src/storage/broker/wal_maintenance.rs`,
`messagesstore_sections/wal_maintenance.rs`, `sdk_domain_snapshot_key.rs:105-135`.

1. Open a third, maintenance-only connection at store open and run PASSIVE/TRUNCATE on it
   **outside** the writer mutex (SQLite serialises checkpoints with writers itself; PASSIVE
   never blocks a writer, TRUNCATE with `busy_timeout=0` just reports busy).
2. Rate-limit the inline `reclaim_wal_recorded` TRUNCATE to once per maintenance interval,
   so a WAL above budget/8 cannot make every write do a checkpoint.
3. Find the pinned reader that keeps production's WAL at 546 MB: add `wal_checkpoint`
   result (`busy`, `log`, `checkpointed` frames) to `storage.wal` metrics if not already there,
   and check whether any read-connection statement stays open (long `rows()` iteration held
   across RPC handling).

### PR 4 (conditional, after PR 1 measurements) — allocator and propagation prune paging

- If anon memory still ratchets on the ingress soak after PR 1: release binaries are static
  musl (`.github/workflows/release.yml:111-127`), so `MALLOC_ARENA_MAX` does not apply; add
  `mimalloc` as `#[global_allocator]` in `reticulumd/main.rs` (one dependency, two lines) and
  re-measure. Do not do this before PR 1 numbers exist.
- Page the quota phase and payload prune like `peer_entry_prune_pages.rs` (512-row keyset
  pages, writer released between pages). Only if `storage` metrics on the ingress soak still
  show multi-second writer waits during the 300 s pass.

## Verification

1. Per PR: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features
   --no-deps -- -D warnings`, focused `cargo test -p reticulumd -p reticulum-rs-rpc
   -p reticulum-rs-transport`, `tools/scripts/check-module-size.sh`,
   `cargo test -p reticulumd --test code_quality_issue_369`.
2. Before/after soak with the production-shaped fixture, **ingress** profile (the one that
   matches production), 10 min each, same cgroup limits:
   `cargo xtask resource-soak-663 -- run --out target/soak/pr1-ingress --profile ingress
   --duration-secs 600`. Acceptance for PR 1: `providers.announce_persistence.runs_total` ≤ 3
   per 10 min, CPU well under the 12–14 % baseline, file cache no longer stepping by ~400 MiB
   every 5 min, anon growth flat after the first minute, zero poll errors. For PR 2: negotiate
   p99 < 100 ms while a save and a WAL checkpoint run; `/readyz` returns 503 until ZMQ bound.
3. Ask the operator for one `/metrics` scrape from preview.26 before the next deploy:
   `providers.announce_persistence.{runs_total,triggers_total}` and `storage.wal` confirm R1
   and the stuck WAL on real data (no restart needed).
4. Publish a daemon-only prerelease after PR 1+2 for the RCH production test; keep #678
   cache-retention acceptance (file/inode counts over several GC cycles) reported separately
   from #663/#657 resource acceptance, as the issues request.

## Not in scope

RCH-side `request_timeout` (3 s) and its startup ordering; the "sending on a closed channel"
shutdown message (originates in RCH, not this repo); a SQLite-backed announce cache (#678
proposal) — the bounded file GC already holds disk flat.
