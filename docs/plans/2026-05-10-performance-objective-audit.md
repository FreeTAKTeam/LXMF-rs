# Performance Objective Audit

Status: in progress

This audit maps the performance objective to concrete artifacts in the active
workspace. It is intentionally stricter than “tests pass”: each item needs
direct evidence that the requested behavior, gate, or benchmark exists.

## Objective Checklist

1. Run the Rust/Python benchmark report and identify gaps.

   Evidence:
   - `target/criterion/python-impl-report/report.txt` exists from
     `cargo xtask python-impl-bench-report`.
   - The report includes Rust/Python timing and speedup targets for LXMF
     encode/decode, large LXMF encode/decode, announce create/validate,
     announce validate batch 64, identity sign/verify/encrypt/decrypt, resource
     request-window handling, and daemon inbound delivery accept.
   - Current weakest p50 speedups are identity encrypt at 2.60x, identity
     decrypt at 3.14x, identity sign at 4.35x, identity verify at 4.97x, and
     announce validate at 5.19x. The strongest protocol wins remain LXMF
     decode/encode at roughly 1483x/1081x.

   Status: covered.

2. Add explicit performance targets.

   Evidence:
   - `tools/benchmarks/python_impl.toml` carries minimum and stretch p50 speedup
     targets for Rust/Python comparisons.
   - `xtask/src/main.rs` enforces those targets in the Python implementation
     comparison/report paths.
   - `xtask/src/main.rs` also defines SDK/daemon/transport latency and
     throughput budgets, including `rns_transport_resource_worker_ipc_envelope`.

   Status: covered.

3. Optimize hot paths.

   Evidence:
   - LXMF encode/decode paths clear high Rust/Python speedup targets.
   - Resource request-window reuse is budgeted and reported.
   - `rns_transport/resource_prepare_send` measures bounded resource
     preparation work.
   - `rns_transport/resource_worker_ipc_envelope` isolates worker frame plus
     msgpack request/response overhead for representative resource completion
     payloads; the latest budget report shows p50 3504.29 ns and p95/p99
     3521.53 ns.
   - `reticulumd/worker_local_resource_complete` and
     `reticulumd/worker_stdio_resource_complete_round_trip` compare the same
     resource completion workload on the local path and through a reused real
     `reticulumd --worker-stdio` child. The latest budget report shows
     24294.40 ns p50 local completion and 64417.00 ns p50 process round-trip.
   - `reticulumd/worker_local_outbound_encrypt` and
     `reticulumd/worker_stdio_outbound_encrypt_round_trip` compare the same
     outbound crypto workload locally and through a reused child. The latest
     budget report shows 60562.07 ns p50 local encryption and 103178.91 ns p50
     process round-trip, so process mode is measured as an isolation boundary
     rather than assumed to be a latency win.
   - `CachedFernet::encrypt` now uses the cached signing/encryption key
     material directly instead of constructing a temporary `Fernet` wrapper on
     every cached-session encryption. The measured cached Fernet and link
     encrypt paths remain inside the SDK perf budget; an attempted broader
     `DerivedKey` cache was rejected because it did not improve the identity
     encrypt/decrypt gap in Criterion.
   - Fernet verification now uses the HMAC implementation's built-in tag
     verifier instead of materializing a computed tag and comparing it with a
     local iterator in both `rns-core` and `rns-transport`. This keeps the
     verification path simpler and aligned with the crypto crate's constant-time
     comparison primitive.
   - `rns-core` Fernet decrypt now uses the padding-helper decrypt path already
     used by `rns-transport`, avoiding the manual block-copy/decrypt/padding
     scan in the core identity benchmark path. The report-profile Rust/Python
     comparison now shows identity decrypt at 3.14x over Python, up from the
     prior 1.92x gap.
   - `crates/libs/rns-core/benches/parity_hotpaths.rs` now includes focused
     identity encrypt split probes. The latest focused Criterion run measured
     total identity encrypt at about 106.26 us, key schedule at about 58.39 us,
     and Fernet-only payload encryption at about 54.69 us, which points the
     next encrypt pass at both X25519/HKDF cost and Fernet encryption rather
     than a single dominant clone/allocation issue. The SDK perf budget runner
     now includes matching diagnostic budgets for both split probes.
   - The key-schedule diagnostics are now split further and enforced in the SDK
     perf budget gate. The latest budget report measures
     `rns_core_identity_encrypt_key_schedule` at p50 51039.74 ns,
     `rns_core_identity_ephemeral_keypair` at p50 13444.83 ns,
     `rns_core_identity_x25519_exchange` at p50 35756.81 ns, and
     `rns_core_identity_hkdf_sha256` at p50 1909.25 ns. That shows the
     remaining key-schedule floor is curve work rather than HKDF expansion.
   - `x25519-dalek` already resolves with `precomputed-tables`, so there is no
     obvious missing dependency feature for the curve path. The opt-in
     `fernet-aes128` feature now propagates through `lxmf-wire`,
     `reticulum-rs-transport`, `lxmf`, and `reticulum-rs`. A controlled
     Criterion run measured 2 KiB core Fernet encrypt at about 42.67 us with
     AES-128 versus 54.27 us by default, and decrypt at about 16.21 us versus
     19.42 us. This remains opt-in until wire-compatibility and security
     posture are explicitly decided.
   - The same benchmark and SDK budget now split identity decrypt. The latest
     SDK perf budget report shows full identity decrypt at p50 57426.30 ns,
     key schedule at p50 37549.40 ns, and Fernet verify/decrypt at p50
     19657.29 ns. That narrows the decrypt optimization target to
     asymmetric/HKDF work first, then Fernet verification/decrypt throughput.
   - `crates/libs/rns-core/benches/parity_hotpaths.rs` now includes batch-64
     identity encrypt/decrypt probes in both serial and parallel forms. The
     latest SDK perf budget report measured encrypt batch-64 at about 6.79 ms
     serial and 1.38 ms parallel; decrypt batch-64 measured about 3.68 ms
     serial and 0.82 ms parallel. `xtask/src/main.rs` carries SDK perf budgets
     for all four batch probes, so crypto batch scheduling has a regression
     guard before being promoted into runtime behavior.
   - `crates/libs/rns-transport/src/transport/worker_boundary.rs` now defines
     batch worker envelopes for outbound encryption and single-destination
     decrypt jobs, plus matching batch result variants. The real
     `reticulumd --worker-stdio` child handles both batch job kinds, and
     `cargo test -p reticulumd --test worker_stdio_process` covers batch
     encrypt/decrypt through a spawned framed worker process.
   - The stdio worker processes batch crypto items in parallel inside the
     child. `crates/apps/reticulumd/benches/worker_process.rs` now compares
     serial local outbound encryption batch-64 against a reused child-process
     batch round trip. The latest SDK perf budget report shows p50
     3901555.50 ns locally and p50 958669.80 ns through the stdio batch worker,
     with overall `Status: PASS`.
   - `crates/libs/rns-transport/src/transport/crypto_batch_lane.rs` now adds
     bounded outbound and inbound crypto batch lanes. They accept outbound
     encrypt and single-destination decrypt work with non-blocking `try_send`,
     coalesce queued work up to 64 items, submit one batch worker job, and fan
     ordered results or worker errors back through per-packet one-shot replies.
     Transport startup wraps configured outbound and single-destination worker
     backends in these lanes, so live outbound encryption and inbound
     single-destination decrypt now reach the measured batch worker paths
     instead of submitting one worker job per packet. Focused tests cover
     queue-full rejection, queued-job coalescing, batch error fanout, and the
     existing configured outbound/inbound worker paths.
   - Local outbound encryption now uses
     `ratchets::encrypt_for_public_key_into` to write ciphertext directly into
     the packet's fixed `PacketDataBuffer` inside the blocking worker. This
     removes the previous heap `Vec` ciphertext plus copy back into packet
     storage on the local worker path. Unit tests cover the in-place helper and
     full `reticulum-rs-transport` tests cover the outbound encryption path.

   Status: partially covered.

   Remaining gap:
   - The report still shows identity encrypt/decrypt below stretch targets, so
     deeper crypto work remains worthwhile. Runtime outbound encryption and
     inbound single-destination decrypt now have bounded batch coalescing, while
     lower-level asymmetric crypto and Fernet throughput remain open.

4. Make async non-blocking.

   Evidence:
   - Resource send preparation, resource decrypt, resource completion,
     single-destination decrypt, outbound encrypt, delivery scheduling,
     storage/event-sink paths, and resource-manager work have been moved behind
     bounded lanes or worker queues.
   - Tests cover skipping busy links/destinations for resource completion,
     resource decrypt, send preparation, single-destination decrypt, and process
     pool busy-slot selection.
   - First lock-audit pass over `transport/wire.rs`, `transport/jobs.rs`,
     `transport/links.rs`, and `transport/resource_lane.rs` found the packet
     receive hot paths snapshot handler/link state before awaiting worker,
     decrypt, resource, or send operations. `ResourceManagerLane` intentionally
     serializes resource state through one command worker; callers await lane
     replies outside global transport-handler locks.
   - First RPC/storage audit pass found `MessagesStore` routes outbound writes
     through a bounded `messages-outbound-writer` queue, exposes contention
     counters, and uses separate read/write SQLite connections for file-backed
     stores. Event-sink dispatch and outbound bridge delivery use bounded
     worker queues with non-blocking `try_send` on the RPC request path.
   - Message storage writer admission now uses `try_send` for both scheduled
     and synchronous write APIs before any caller waits for a writer-lane reply.
     `synchronous_writer_lane_returns_would_block_when_queue_is_full` fills a
     stalled one-slot writer queue and verifies receipt-status updates return a
     `WouldBlock` error instead of blocking on queue admission.
   - Storage writer-lane cost is now directly budgeted by
     `rns_rpc/message_store_insert`. The latest SDK perf budget report measures
     the synchronous in-memory writer-lane insert path at p50 11413.54 ns,
     p95/p99 11691.64 ns, and 87615.22 ops/sec.
   - Resource send preparation now uses immediate bounded backpressure for the
     local CPU worker lane. `send_resource_returns_when_prepare_workers_are_saturated`
     holds every resource-prepare permit and verifies `send_resource` returns
     `ConnectionError` within a short timeout instead of awaiting a worker slot.
   - Single-destination decrypt, link-resource decrypt, and resource completion
     now use immediate bounded permit admission too.
     `local_single_destination_decrypt_returns_when_workers_are_saturated`,
     `link_resource_decrypt_returns_when_workers_are_saturated`, and
     `resource_completion_returns_when_workers_are_saturated` hold every
     matching permit and verify those paths return within short timeouts instead
     of awaiting saturated local CPU worker lanes.
   - First daemon-adapter audit pass over inbound control, outbound bridge,
     remote propagation control, interface hot-apply, startup, and BLE adapter
     paths found request-path locks used for short snapshots or manager
     mutations before awaited network, link, storage, or BLE work.
   - `outbound_encryption_saturation_returns_without_waiting` holds every local
     outbound crypto permit and verifies an encrypted send returns immediately
     with `DroppedEncryptFailed` instead of waiting behind a saturated crypto
     lane.
   - Interface TX dispatch now uses immediate bounded queue admission after
     planning matched interfaces. `full_interface_tx_queue_returns_without_waiting`
     fills a one-slot interface TX queue and verifies the next send reports one
     failed interface within a short timeout instead of awaiting enqueue
     capacity.
   - Resource manager lane prepared-send commit and outbound dispatch
     confirmation now use immediate bounded queue admission. If the queue is
     full and the manager lock is busy, commit returns `ConnectionError` and
     confirmation returns without waiting; `resource_lane_commit_prepared_send_fails_when_manager_queue_is_full`
     and `resource_lane_confirm_dispatch_returns_when_manager_queue_is_full`
     cover those saturated-lane paths.
   - Physical interface receive loops now use non-blocking admission into the
     shared transport RX queue. UDP, serial, TCP client, and native BLE reader
     loops use `try_send` for decoded packets, so a full transport RX queue
     drops the packet instead of suspending the interface reader.
   - Interface worker process bridge queues now use non-blocking admission at
     the parent/child boundary. `interface_rx_forwarder_drops_when_transport_channel_is_full`
     fills the parent transport RX queue and verifies child inbound forwarding
     does not stall, while `udp_interface_worker_stdio_drops_outbound_when_tx_channel_is_full`
     fills the child TX queue and verifies child outbound forwarding does not
     stall.
   - Parent-side interface-worker manager dispatch now uses
     `InterfaceManager::send_from_shared`, which plans interface dispatch work
     under the manager mutex and performs bounded queue admission after
     releasing that mutex. The restart-state regression test passes serially
     after the change.
   - `cargo run -p xtask -- security-review-check` now includes an executable
     runtime hygiene scan for production paths: blocking thread sleeps,
     unbounded Tokio runtime channels, synchronous `MutexGuard` bindings that
     remain live across `.await`, and async-mutex send/dispatch calls such as
     `.lock().await.send(...)`. The guard scan intentionally ignores test
     modules and one-line snapshot blocks so it catches high-risk production
     patterns without flagging short state snapshots.

   Status: covered for current single-process async lanes.

   Remaining gap:
   - The static lock scan is conservative and line-oriented. It is useful
     regression coverage for obvious synchronous guard-across-await mistakes,
     but it is not a replacement for periodic manual review of complex async
     control flow.

5. Add worker pools.

   Evidence:
   - Local bounded workers exist for crypto/resource hot paths.
   - `WorkerStdioPool` owns multiple child processes, selects idle children
     before waiting on a busy slot, and replaces timed-out children.
   - `reticulumd --worker-stdio` handles announce validation, outbound encrypt,
     single-destination decrypt, resource completion, outbound encrypt batches,
     and single-destination decrypt batches.
   - `worker_stdio_process.rs` proves one child can serve multiple framed jobs
     before EOF and can process batch crypto jobs over the same worker protocol.

   Status: covered for current crypto/resource process workers.

   Remaining gap:
   - Storage is still a lane/queue model, not a separate process-worker
     transport.

6. Add perf CI.

   Evidence:
   - `.github/workflows/performance.yml` runs `cargo xtask ci --stage
     sdk-perf-budget-check`, `cargo xtask ci --stage python-impl-perf-gate`,
     and manual report generation.
   - `.github/workflows/ci.yml` now runs `cargo xtask ci --stage
     security-review-check` in the regular security job, so the async runtime
     hygiene scans for blocking sleeps, unbounded channels, and synchronous
     mutex guards across `.await` run on ordinary PRs.
   - `.github/workflows/ci.yml` now checks the optional `fernet-aes128` feature
     graph for `reticulum-rs-transport`, `lxmf-wire`, and `reticulum-rs`, so
     the measured opt-in Fernet throughput path cannot drift out of downstream
     crate compatibility unnoticed.
   - `target/criterion/bench-budget-report.txt` currently reports `Status:
     PASS` and includes storage writer-lane insertion plus local-vs-process
     resource completion and outbound encryption budgets for
     `rns_rpc_message_store_insert`,
     `reticulumd_worker_local_resource_complete` and
     `reticulumd_worker_stdio_resource_complete_round_trip`,
     `reticulumd_worker_local_outbound_encrypt`, and
     `reticulumd_worker_stdio_outbound_encrypt_round_trip`, the 64-item
     outbound encrypt batch budgets, and the 64-item single-destination decrypt
     batch budgets, plus the router/control child-process round-trip budget for
     `reticulumd_control_router_stdio_status_round_trip`.
   - The latest SDK perf budget report measures outbound encryption at p50
     60562.07 ns in-process and p50 103178.91 ns through a reused
     `reticulumd --worker-stdio` child. This makes the crypto process-boundary
     overhead visible before promoting process mode as a default.
   - The focused worker-process Criterion run for decrypt batching measures
     `reticulumd/worker_local_inbound_decrypt_batch_64` at about
     4.31 ms p50 and
     `reticulumd/worker_stdio_inbound_decrypt_batch_64_round_trip`
     at about 1.01 ms p50 for 64 items, matching the outbound batch evidence
     with an inbound decrypt budget gate.

   Status: covered.

7. Add optional multi-process mode later, after measurement.

   Evidence:
   - `crates/libs/rns-rpc/src/rpc/control_boundary.rs` defines the first
     router/control process boundary: a versioned, bounded, msgpack-framed
     control envelope for existing `RpcRequest`/`RpcResponse` values, health,
     and shutdown. Its reusable router serve loop reads framed control requests,
     dispatches them through a caller-provided RPC handler, writes framed
     responses, and exits on shutdown or EOF. Tests cover RPC request round
     trips, wrong-version rejection, oversized frame-length rejection before
     payload allocation, incomplete frame rejection, async stream transport,
     request/response exchange, real `RpcDaemon::handle_rpc` dispatch through
     the control stream, and request-stream rejection of unexpected responses.
   - `rns_rpc/control_boundary_envelope` measures the router/control control
     envelope frame plus msgpack encode/decode overhead for representative
     `daemon_status_ex` request/response traffic. The SDK perf budget now
     enforces `rns_rpc_control_boundary_envelope`; the latest report shows p50
     12582.94 ns, p95/p99 12616.24 ns, throughput 79472.70 ops/sec, and overall
     `Status: PASS`.
   - Hidden `reticulumd --control-router-stdio` now exposes the same
     router/control boundary as a real spawned-process runtime. It bootstraps
     the daemon, serves framed control requests through `RpcDaemon::handle_rpc`,
     keeps stdout reserved for control frames, preserves request ids on internal
     errors, and exits on a shutdown envelope. The integration test
     `reticulumd_control_router_stdio_serves_rpc_until_shutdown` verifies a
     spawned daemon child serves `daemon_status_ex` and shuts down cleanly.
   - `ControlRouterStdioProcess` provides the parent-side client primitive for
     that boundary: it spawns a framed control-router child, submits RPC
     requests, rejects mismatched response sequences, and sends explicit
     shutdown. The unit test
     `control_router_stdio_process_round_trips_rpc_response` verifies the
     request/response/shutdown path against a framed mock child.
   - `ControlRouterStdioProcess::request_with_timeout` bounds parent-side
     waits for a child response. On timeout it kills the child, closes the
     pipes, and returns `RequestTimedOut`; the unit test
     `control_router_stdio_process_times_out_stalled_child` verifies a stalled
     child does not block the caller indefinitely.
   - `ControlRouterStdioPool` provides a fixed-size parent-side pool for
     framed control-router children. It selects idle children before waiting on
     a busy slot, exposes worker-count/idle/busy/timeout/replacement snapshots,
     and replaces a child after request timeout. The tests
     `control_router_stdio_pool_serves_idle_child_while_peer_is_stalled` and
     `control_router_stdio_pool_replaces_timed_out_child` cover idle-slot
     routing, timeout accounting, replacement accounting, and a successful
     request through the replacement child.
   - `daemon_status_ex.control_router_processes` now reports router/control
     process health with the same enabled/count/timeout/idle/busy/timeouts/
     replacement fields used by the worker process status. RPC tests cover both
     default disabled values and populated runtime values, and reticulumd tests
     cover the default conversion from staged runtime config to status.
   - Hidden `--control-router-process-count`,
     `--control-router-process-timeout-ms`, and
     `--control-router-process-command` options now configure a retained
     parent-side control-router pool at daemon startup. Bootstrap tests verify
     the pool is spawned, status is published, and child daemon args include the
     parent database path.
   - External `/rpc` handling now routes eligible read-only control requests
     through the configured control-router pool. The initial allowlist is
     `status`; richer parent-owned snapshots such as `daemon_status_ex` and
     mutating methods such as `send_message` and `sdk_send_v2` remain on the
     in-process path to avoid parent/child state divergence. Unit tests cover
     the routed status request, the mutating method guard, and the failure mode
     where a stalled routed child times out without blocking an unrelated
     in-process RPC.
   - `reticulumd/control_router_stdio_status_round_trip` now measures the real
     spawned child-process router/control path for `daemon_status_ex`, reusing a
     `reticulumd --control-router-stdio` child across iterations. The SDK perf
     budget enforces `reticulumd_control_router_stdio_status_round_trip`; the
     latest report shows p50 53907.16 ns, p95/p99 56715.63 ns, throughput
     18550.41 ops/sec, and overall `Status: PASS`.
   - `reticulumd/control_router_http_status_routed_round_trip` now measures the
     production-facing routed path: a real parent daemon accepts HTTP `/rpc`
     over loopback TCP, routes allowed `status` traffic through the
     control-router child pool, and returns the framed RPC response. The SDK
     perf budget enforces
     `reticulumd_control_router_http_status_routed_round_trip`; the latest
     report shows p50 177613.42 ns, p95/p99 187550.00 ns, throughput 5630.21
     ops/sec, and overall `Status: PASS`.
   - Hidden `--worker-process-count`, `--worker-process-timeout-ms`, and
     `--worker-process-command` options configure the process pool. The default
     still spawns the current daemon executable, while the command override
     lets operators point the pool at a separately built framed worker binary.
   - Hidden `--worker-process-tcp` now lets the same framed worker pool connect
     to an externally managed worker supervisor over TCP instead of only
     spawning daemon-owned child processes. Unix-socket endpoint plumbing also
     exists behind `--worker-process-unix-socket` on Unix targets, while tests
     use an in-memory non-child stream to verify the shared framed-worker pool
     path without relying on local socket permissions.
   - Daemon status exposes `worker_processes`.
   - Transport routes announce validation, outbound encrypt, single-destination
     decrypt, and resource completion through the configured worker backend
     with local fallback.
   - Process-worker timeout/replacement, idle-child selection, real child
     stdio jobs, and multi-job child reuse have test coverage. The timeout
     replacement test now verifies that the replacement child can serve a
     subsequent framed worker response after the stalled child is killed. A
     separate pool test verifies that a busy child slot does not stop selection
     of an idle child when capacity remains.
   - `worker_process_pool_serves_idle_child_while_peer_child_is_stalled` keeps
     one child process blocked inside a framed request while a second child
     process in the same pool serves another framed request promptly.
   - `worker_process_backend_serves_idle_child_while_peer_child_is_stalled`
     proves the same request-level isolation through `WorkerStdioPoolBackend`,
     the `WorkerBackend` abstraction used by transport hot paths.
   - `worker_process_backend_replaces_timed_out_child_and_serves_next_request`
     proves `WorkerStdioPoolBackend` maps a stalled child timeout into a backend
     error, replaces the child, and serves the next backend request through the
     replacement process. The same test now runs the periodic status publisher
     on a short interval and waits for `daemon_status_ex.worker_processes` to
     show the timeout/replacement counters without a manual refresh call.
   - `WorkerStdioPool::snapshot` exposes lightweight runtime health for the
     managed process pool: worker count, idle/busy slots, request timeouts, and
     child replacements. Pool and backend tests assert those counters during
     in-flight stalls and after replacement.
   - `daemon_status_ex.worker_processes` now carries the same health fields
     (`idle_workers`, `busy_workers`, `request_timeouts`,
     `child_replacements`) in addition to enabled/count/timeout metadata.
     Bootstrap seeds those values from the backend snapshot and runs a periodic
     publisher so status keeps reflecting the live in-daemon process pool.
   - `stalled_worker_process_submit_does_not_block_daemon_status_rpc` keeps a
     child process stalled in a worker submit and verifies `daemon_status_ex`
     still returns promptly through the real `RpcDaemon::handle_rpc` path.
   - `stalled_worker_process_submit_does_not_block_event_sink_dispatch` keeps a
     child process stalled in a worker submit and verifies a real RPC event-sink
     bridge still receives published events promptly.
   - `stalled_worker_process_submit_does_not_block_outbound_delivery` keeps a
     child process stalled in a worker submit and verifies bridge-backed
     `send_message_v2` still starts outbound delivery promptly.
   - `packet_receive_continues_while_announce_worker_is_stalled` keeps announce
     worker validation stalled and verifies the interface packet receive loop
     still drains and broadcasts a subsequent unrelated packet promptly.
   - `worker_process_restart_does_not_corrupt_daemon_message_state` keeps a
     worker child stalled, records outbound message and receipt state through
     real RPC calls, lets the pool time out and replace the child, verifies the
     replacement serves the next worker request, and then verifies SDK
     configuration revision/CAS, announce-derived route/discovery state,
     message content, and delivered receipt status remain intact.
   - `crates/apps/reticulumd/benches/worker_process.rs` benchmarks local
     resource completion against real stdio process round-trip for the same
     workload, and `xtask` enforces both in `sdk-perf-budget-check`.
   - `crates/libs/rns-transport/src/transport/interface_boundary.rs` now defines
     the process-safe interface event envelope for future interface worker
     processes. Tests cover inbound/outbound packet event round trips, wrong
     protocol-version rejection, oversized event rejection, async stream
     round-trip, oversized frame-length rejection before payload allocation,
     serve-until-shutdown handling, cancellation stop reporting, transport TX
     channel forwarding to a worker stream, and worker inbound packet forwarding
     back to the transport RX channel.
   - `rns_transport/interface_worker_ipc_envelope` measures the interface
     worker frame plus msgpack encode/decode overhead for representative
     inbound/outbound packet events. The current SDK perf budget report shows
     p50 1173.75 ns, p95/p99 1189.07 ns, throughput 851969.45 ops/sec, and
     overall `Status: PASS`.
   - Hidden `reticulumd --interface-worker-stdio` consumes framed interface
     envelopes in a real child process until shutdown/EOF. The integration test
     `reticulumd_interface_worker_stdio_accepts_framed_events_until_shutdown`
     spawns the daemon child, writes a framed interface event plus shutdown, and
     verifies clean process exit.
   - `InterfaceWorkerStdioProcess` is the daemon-side client primitive for the
     same boundary. Its unit test spawns an executable worker, sends a framed
     interface event, reads an echoed framed event from stdout, sends shutdown,
     and verifies the child observed the event and exited cleanly.
   - `InterfaceWorkerStdioProcess::run_channel_bridge` now multiplexes the
     existing transport `InterfaceTxReceiver` and `InterfaceRxSender` channels
     over the child process stdin/stdout boundary and returns sent/received
     counts plus a stop reason. The unit test
     `interface_worker_channel_bridge_forwards_both_directions` proves one
     outbound transport packet reaches the child while one inbound child packet
     is delivered back into the transport RX channel.
   - `InterfaceWorkerStdioProcess::run_channel_bridge_until_cancelled` adds a
     supervisor cancellation path over the same bridge. The unit test
     `interface_worker_channel_bridge_cancellation_shuts_down_child` proves a
     cancelled bridge sends a shutdown frame to the child and returns a
     `Cancelled` stop reason instead of waiting for more interface traffic.
   - `spawn_interface_worker_bridge` now registers a process-backed interface
     as a normal `InterfaceManager` channel and runs the cancellable stdio
     bridge behind it. The unit test
     `interface_worker_bridge_registers_manager_channel` verifies transport
     sends through `InterfaceManager::send` reach the child while inbound child
     packets arrive through the manager receiver.
   - The same registered bridge now supervises the child process internally:
     child EOF, child shutdown, or transient bridge errors restart the child
     after a configurable short backoff while explicit cancellation and
     transport channel close still stop the bridge. The unit test
     `interface_worker_bridge_restarts_child_after_early_exit` proves the
     bridge recovers when the first child exits before interface traffic
     arrives.
   - Hidden daemon options `--interface-worker-process-count`,
     `--interface-worker-process-command`,
     `--interface-worker-process-shutdown-ms`, and
     `--interface-worker-process-restart-backoff-ms` now let bootstrap register
     process-backed interface-worker channels, configure supervisor timing, and
     keep their cancellation/task handles alive in `BootstrapContext`. The test
     `bootstrap_registers_configured_interface_worker_process` verifies a
     configured child command appears in `list_interfaces` as an
     `interface_worker_process` with runtime interface metadata.
   - `daemon_status_ex.interface_worker_processes` now reports whether
     process-backed interface workers are enabled, configured count, shutdown
     timeout, configured restart backoff, live workers, stopped workers,
     aggregate child restarts, and aggregate child errors. RPC tests cover the
     cached status schema and bootstrap tests verify enabled interface-worker
     process status after startup.
   - Bootstrap now runs a periodic interface-worker status publisher backed by
     the live bridge handles, so `daemon_status_ex.interface_worker_processes`
     reports the supervised bridge as live while the bridge restarts an exited
     child. The test
     `interface_worker_process_status_publisher_reports_restarted_child_live`
     starts an immediately exiting child and verifies the real daemon status RPC
     keeps reporting one live interface worker with at least one child restart
     and zero child errors rather than a permanently stopped bridge.
   - `interface_worker_restart_preserves_configured_interface_state` now keeps a
     configured UDP interface on the process-backed path, forces the first child
     process to exit, waits for the supervisor restart, and verifies
     `daemon_status_ex` plus `list_interfaces` still report the original
     configured interface, runtime interface address, startup status, and
     process manager metadata.
   - Hidden UDP interface-worker child mode now exists behind
     `reticulumd --interface-worker-stdio --interface-worker-udp-bind ...`.
     The child mode runs a UDP interface loop behind the framed stdio boundary,
     accepts framed outbound interface events into the UDP tx channel, emits UDP
     rx channel packets as framed inbound events, and uses the parent-assigned
     interface address. Tests cover CLI parsing and stdio TX/RX channel
     bridging for this UDP child path.
   - Configured UDP interfaces now use the process-backed UDP worker path when
     interface worker processes are enabled. The bootstrap test
     `bootstrap_uses_interface_worker_process_for_configured_udp` verifies a
     normal `type = "udp"` config record is exposed through `list_interfaces`
     with `startup_status=spawned_process` and
     `managed_by=interface_worker_process`.
   - Hidden serial interface-worker child mode now exists behind
     `reticulumd --interface-worker-stdio --interface-worker-serial-device ...`.
     The child mode reconstructs `SerialInterface` options from hidden CLI
     flags and runs the serial interface loop behind the framed stdio boundary.
   Configured serial interfaces now choose the process-backed path when
   interface worker processes are enabled. Tests cover serial child CLI
   parsing and `bootstrap_uses_interface_worker_process_for_configured_serial`.
   - Hidden TCP client interface-worker child mode now exists behind
     `reticulumd --interface-worker-stdio --interface-worker-tcp-connect ...`.
     The child mode runs `TcpClient` behind the framed stdio boundary, and
     configured `tcp_client` interfaces now choose the process-backed path when
     interface worker processes are enabled. Tests cover TCP child CLI parsing
     and `bootstrap_uses_interface_worker_process_for_configured_tcp_client`.
   - Hidden BLE GATT interface-worker child mode now exists behind
     `reticulumd --interface-worker-stdio --interface-worker-ble-*` flags. The
     child mode reconstructs BLE GATT config from hidden CLI flags, runs the
     existing BLE interface in a child-local `InterfaceManager`, and translates
     parent-assigned interface addresses to the child-local BLE channel in both
     directions. Tests cover BLE child CLI parsing, address translation, and
     `bootstrap_uses_interface_worker_process_for_configured_ble`.
   - `InterfaceManager::register_remote_iface_alias` now lets the parent
     manager register child/process-owned interface addresses that share a
     parent host bridge tx channel. Accepted TCP clients are dynamic
     child-local interfaces, so listener process ownership needs those child
     addresses to remain routable from the parent. The transport test
     `remote_iface_alias_routes_direct_tx_through_host_bridge` proves direct tx
     to a child-owned alias is delivered through the host bridge channel.
   - Hidden TCP server interface-worker child mode now exists behind
     `reticulumd --interface-worker-stdio --interface-worker-tcp-listen ...`.
     Normal configured `tcp_server` records now choose this process-backed
     listener path when interface worker processes are enabled. Tests cover TCP
     server child CLI parsing and
     `bootstrap_uses_interface_worker_process_for_configured_tcp_server`.

   Status: partially covered.

   Remaining gaps:
   - UDP, serial, TCP client, TCP server/listener, and BLE GATT have
     process-backed configured-interface paths. Router/control now has a framed
     boundary and hidden stdio child runtime, but a full parent/supervisor
     router-control process split, independently managed workers, and broader
     router-process restart-state-corruption tests are still future work.
   - Stalled worker isolation is proven at the pool level, for packet receive,
     RPC status, event-sink dispatch, outbound delivery, and message/receipt
     plus SDK and announce/discovery state across worker replacement.
     Configured-interface state is now covered across interface-worker child
     restart. Independently managed worker evidence is not complete.

## Current Conclusion

The single-process async worker-lane performance engine is substantially
implemented and measured. Optional process-backed crypto/resource workers are
implemented for the main hot jobs and have strong protocol, fallback, timeout,
and child-process reuse coverage. The multi-process plan now carries an
explicit placement decision: local bounded lanes remain the default for
storage, resource completion, and outbound encryption because current process
round trips are slower, while process mode remains operator-selectable for
fault isolation and CPU partitioning.

The overall objective is not complete yet. The remaining work is measurement
and production-scale isolation: audit remaining hot-path lock boundaries,
deepen identity encrypt/decrypt optimization, and extend independently managed
multi-process coverage beyond the current supervised child-pool paths only if
the measurements show it is justified.
