# Performance Engine Roadmap

Status: in progress

This plan turns the Python implementation benchmark report into concrete
performance work. The direction is a single-process async daemon with isolated
worker lanes first, then optional multi-process scaling after the hot paths are
measured and stable.

## Baseline

Generated with:

```bash
cargo run -p xtask -- python-impl-bench-report
```

Report artifact:

- `target/criterion/python-impl-report/report.txt`

Completion audit:

- `docs/plans/2026-05-10-performance-objective-audit.md`

Current p50 speedups over Python from the report profile:

| Workload | Rust speedup |
| --- | ---: |
| LXMF message decode | 1482.91x |
| LXMF message encode | 1081.38x |
| LXMF large message decode | 1023.13x |
| LXMF large message encode | 560.41x |
| Reticulum announce create | 6.66x |
| Reticulum announce validate | 5.19x |
| Reticulum announce validate batch 64 | 5.21x |
| Reticulum identity sign | 4.35x |
| Reticulum identity verify | 4.97x |
| Reticulum identity encrypt | 2.60x |
| Reticulum identity decrypt | 3.14x |
| Reticulum resource request window | 18.95x |
| Daemon inbound delivery accept | 10.87x |

## Performance Targets

The enforceable minimum p50 speedups live in
`tools/benchmarks/python_impl.toml` as `min_p50_speedup`. They are deliberately
below the current report medians so noisy machines do not make the gate flaky.

The same file also carries `stretch_p50_speedup` values. Stretch values are the
optimization targets for “surpass Python by miles” work and do not fail CI by
themselves.

## Priority Lanes

0. Router/control boundary

   First slice landed:
   `crates/libs/rns-rpc/src/rpc/control_boundary.rs` defines a process-safe
   control-plane envelope around the existing public `RpcRequest` and
   `RpcResponse` contracts, plus health and shutdown messages. It is versioned,
   msgpack encoded, framed with a 4-byte length prefix, capped at 4 MiB, and
   tested for wrong protocol versions, oversized frame-length rejection before
   allocation, incomplete payloads, async stream round trips, and full
   request/response exchange. This gives a future router/control process split
   a stable IPC primitive without renaming public RPC methods. The boundary now
   includes `serve_control_router`, which reads framed requests, dispatches them
   through a caller-provided RPC handler such as `RpcDaemon::handle_rpc`, writes
   framed responses, and stops on shutdown or EOF.
   `rns_rpc/control_boundary_envelope` now measures the control frame plus
   msgpack encode/decode overhead for representative status request/response
   traffic and is enforced in the SDK perf budget gate. `reticulumd` also has a
   hidden `--control-router-stdio` child-process entrypoint that bootstraps the
   daemon, serves real `RpcDaemon::handle_rpc` requests over the same framed
   control stream, preserves request ids on internal errors, and keeps stdout
   reserved for protocol frames. The integration test
   `reticulumd_control_router_stdio_serves_rpc_until_shutdown` proves a spawned
   daemon child serves `daemon_status_ex` and exits cleanly after a shutdown
   envelope. The parent-side `ControlRouterStdioProcess` primitive can spawn a
   framed control-router child, submit a request, require the matching response
   sequence, and shut the child down explicitly; its unit test covers the
   request/response/shutdown lifecycle against a framed mock process. The same
   primitive has bounded request support: a stalled child is killed and reported
   as `RequestTimedOut` instead of leaving the caller blocked indefinitely.
   `control_router_stdio_process_times_out_stalled_child` covers that isolation
   behavior. `ControlRouterStdioPool` layers a fixed-size pool over those
   child processes, prefers an idle child when another slot is busy, exposes
   idle/busy/timeout/replacement snapshots, and replaces a child after a
   request timeout. Tests cover idle-child routing while a peer slot is busy and
   timeout replacement followed by a successful request through the replacement.
   `daemon_status_ex.control_router_processes` now exposes the same enabled,
   worker-count, timeout, idle/busy, request-timeout, and child-replacement
   health shape for router/control pools that worker process pools already use.
   Hidden `--control-router-process-count`,
   `--control-router-process-timeout-ms`, and
   `--control-router-process-command` options now let daemon startup own that
   pool, pass child daemon paths such as `--db`, and publish live health. The
   external `/rpc` path can route eligible read-only control requests (`status`
   first) through the pool while richer parent-owned status snapshots such as
   `daemon_status_ex` and mutating methods continue on the in-process daemon
   path. The routed path has regression coverage for
   stalled children timing out without blocking unrelated local RPC handling.
   `reticulumd/control_router_stdio_status_round_trip` now measures a reused
   real `reticulumd --control-router-stdio` child serving `daemon_status_ex`;
   the latest SDK perf budget report measured p50 52177.08 ns, p95/p99
   54849.05 ns, throughput 19165.50 ops/sec, and passed. The budget also covers
   `reticulumd/control_router_http_status_routed_round_trip`, which starts a
   real parent daemon, routes external HTTP `/rpc` `status` through the child
   pool, and currently measures p50 182017.97 ns, p95/p99 188971.88 ns,
   throughput 5493.96 ops/sec.

1. Crypto and identity workers

   Reticulum identity encrypt is the weakest Rust win today, with decrypt now
   above 3x after the core Fernet decrypt fast path landed. Move encryption,
   decryption, announce validation, and signature verification onto bounded CPU
   worker lanes so async I/O tasks never wait on CPU-heavy work.

   First slice landed: inbound announce signature validation now runs through
   a bounded blocking worker lane before the transport handler lock is
   reacquired for rate limiting and route-table updates.

   Second slice landed: local single-destination data delivery now clones the
   destination and event sender, drops the transport handler lock, and runs
   encrypted payload decryption through a bounded blocking worker lane.

   Third slice landed: public outbound packet sends now collect encryption
   material under the transport handler lock, drop that lock for bounded worker
   encryption, and reacquire it only for route lookup and dispatch.

   Fourth slice landed: encrypted resource control packets now resolve the link,
   drop the transport handler lock, decrypt through a bounded resource worker
   lane, and reacquire the handler only for resource-manager updates.

   Fifth slice landed: RPC event-sink publishing now goes through a bounded
   daemon worker queue, so a slow external sink cannot block inbound delivery,
   SDK sends, or unrelated event publishing. The lane is covered by
   `rns_rpc/event_sink_dispatch` in the SDK perf budget gate.

   Sixth slice landed: bridge-backed outbound sends now schedule delivery on a
   bounded daemon worker queue after the message is stored and marked
   `sending`, so a slow transport bridge cannot block subsequent
   `send_message_v2` calls. The lane is covered by
   `rns_rpc/send_message_v2_bridge_schedule` in the SDK perf budget gate.

   Seventh slice landed: propagation message-store byte-limit pruning now runs
   on the message-store writer lane and inbound delivery schedules it after
   accepting the message instead of waiting for the full scan/delete result.
   This keeps the inbound path from doing retention cleanup inline.

   Eighth slice landed: receipt-status updates and message metadata field
   updates now run through the message-store writer lane, preserving ordering
   with inserts, receipt resolution, and scheduled pruning instead of grabbing
   the SQLite write lock directly on caller threads.

   Ninth slice landed: announce persistence, inbound/outbound ticket upserts,
   ticket expiry pruning, and ticket-delivery markers now run through the
   message-store writer lane, keeping announce ingestion and ticket workflows
   on the same bounded storage path as message writes.

   Storage budget slice landed: the SDK perf budget gate now includes
   `rns_rpc/message_store_insert`, a focused synchronous writer-lane round trip
   against the in-memory message store. The latest report measures
   `rns_rpc_message_store_insert` at 11.40 us p50, making storage lane cost
   explicit instead of only inferring it from daemon inbound delivery.

   Tenth slice landed: outbound resource preparation now runs through a
   bounded blocking worker before the prepared sender is committed to transport
   state. Hashing, link encryption, part chunking, and advertisement
   construction no longer run while holding the transport handler lock, and
   large concurrent resource sends cannot flood the runtime blocking pool. The
   `rns_transport/resource_prepare_send` benchmark is included in the SDK perf
   budget gate. Process-worker resource completion also has a separate
   `rns_transport/resource_worker_ipc_envelope` benchmark that isolates
   worker-frame and msgpack envelope overhead for a representative
   `ResourceComplete` request and `ResourceCompleted` response; the first short
   local run measured about 3.69 us p50.

   Process-worker measurement slice landed: reticulumd now has a Criterion
   benchmark comparing the same resource completion job locally and through a
   reused real `reticulumd --worker-stdio` child process. The SDK perf budget
   gate includes both `reticulumd_worker_local_resource_complete` and
   `reticulumd_worker_stdio_resource_complete_round_trip`; the latest report
   measured about 24.26 us p50 for local completion and 88.33 us p50 for the
   process round-trip.
   The process-worker comparison now also covers outbound packet encryption:
   `reticulumd_worker_local_outbound_encrypt` and
   `reticulumd_worker_stdio_outbound_encrypt_round_trip` use the same
   `WorkerJobKind::OutboundEncrypt` shape. The current SDK perf budget report
   measures about 60.49 us p50 locally and 102.95 us p50 through the reused
   stdio child, making the crypto process-boundary overhead explicit before
   any production routing decision.

   Eleventh slice landed: inbound resource completion now detaches the completed
   receive state from `ResourceManager`, drops transport state locks, and runs
   reassembly completion, resource decrypt, optional decompression, hash/proof
   generation, and proof packet construction through a bounded blocking worker.
   A transport-level async test exercises the worker path and verifies the
   completion event payload.

   Twelfth slice landed: resource retry polling now snapshots retry requests
   and outbound advertisement retries under the transport handler lock, then
   drops that lock before awaiting link locks or packet dispatch. Resource
   retry work no longer holds the global transport handler across send awaits.

   Thirteenth slice landed: link maintenance now snapshots input/output link
   maps under the transport handler lock, then scans watchdogs, channel
   timeouts, stale links, and repeated link requests without holding the global
   handler. Link removals still reacquire the handler briefly, and repeated
   link requests dispatch through the unlocked packet-send helper.

   Fourteenth slice landed: outbound packet dispatch now snapshots routing,
   interface-manager, packet-cache, and link-note state under a short transport
   handler lock, then sends through the interface manager without holding the
   global handler. Link-maintenance direct watchdog and channel-timeout sends
   use the same unlocked dispatch helper.

   Fifteenth slice landed: resource-proof handling, resource-manager response
   packets, resource-completion proof packets, link RTT packets, and forwarded
   link-request proofs now collect state updates under the transport handler
   lock and dispatch packets after dropping it through the unlocked helpers.

   Sixteenth slice landed: public link-send fanout helpers and destination
   announce sends now reuse the unlocked packet-send helper after building
   packets from link or destination state, avoiding a second transport-handler
   lock around interface dispatch.

   Seventeenth slice landed: retransmitted announce batches now drain under a
   short handler lock and dispatch each message through the unlocked helper,
   and `Transport::outbound` no longer holds the handler while broadcasting
   route misses.

   Eighteenth slice landed: inbound data handling now computes link proof,
   resource response, keepalive propagation, and next-hop forwarding messages
   while holding transport state, then drops the handler and dispatches through
   unlocked helpers. Link-request intermediate forwarding follows the same
   pattern after updating the link table.

   Nineteenth slice landed: public direct/broadcast helpers, link-bound channel
   and resource sends, path-request responses and recursive broadcasts, explicit
   path requests, and local link-request proof dispatch now use unlocked message
   dispatch after transport state updates.

   Twentieth slice landed: the Rust/Python comparison suite now includes daemon
   inbound delivery accept, comparing Rust `rns_rpc_accept_inbound` against
   Python `LXMRouter.lxmf_delivery` with a 2x minimum and 5x stretch p50
   speedup target.

   Twenty-first slice landed: inbound daemon accept no longer clones the full
   `MessageRecord` just to store and publish it; `store_inbound_record` now
   borrows the record and keeps ownership with the caller for follow-up command
   correlation.

   Twenty-second slice landed: daemon inbound delivery benchmarking now measures
   steady-state daemon throughput instead of fresh-daemon fixture setup. The
   fast Rust/Python comparison reported `rns_rpc_accept_inbound` at 20.97us p50
   versus Python `LXMRouter.lxmf_delivery` at 238.46us p50, an 11.37x Rust
   advantage that clears the 5x stretch target.

   Twenty-third slice landed: transport duplicate filtering, virtual unicast
   cleanup, packet-cache cleanup, and queued announce release now snapshot the
   required shared handles before awaiting. The packet receiver and maintenance
   loops no longer use the previous production chains that held the global
   transport handler while awaiting packet-cache or interface-manager work.

   Twenty-fourth slice landed: proof handling now uses unlocked snapshots for
   receipt validation and link-request-proof forwarding. Link and destination
   validation locks are acquired after the global transport handler is dropped,
   and the test inbound receipt path uses the same unlocked validator.

   Twenty-fifth slice landed: fresh validated announces and held announce
   releases now use unlocked announce processing. Announce rate-limit checks and
   held-queue draining happen under short handler locks, while identity drift
   checks, virtual unicast registration, and announce event emission run after
   snapshotting the required handles.

   Twenty-sixth slice landed: normal link-data handling now snapshots inbound
   and outbound link candidates, keepalive routing, and next-hop forwarding
   decisions before dropping the transport handler. Link packet handling and
   proof/keepalive response dispatch run outside the global handler lock.

   Twenty-seventh slice landed: active link-resource data handling no longer
   awaits the link mutex while holding the global transport handler. The resource
   manager update still runs in the same critical section once both guards are
   available, but global handler lock acquisition no longer wraps the awaited
   per-link lock.

   Twenty-eighth slice landed: fixed-destination path-request handling now
   snapshots path-request state and local destination handles under short
   transport handler locks, then builds local path responses after dropping the
   global handler. The packet receive loop dispatches fixed destinations through
   this unlocked helper before normal duplicate filtering and packet handling.

   Twenty-ninth slice landed: link-request handling now uses an unlocked entry
   point. Local link requests lock the destination outside the global transport
   handler, build proof material independently, and reacquire the handler only
   to check and insert the inbound link before dispatching the proof packet.

   Thirtieth slice landed: resource-link lookup now reuses the unlocked link
   lookup helper and the defensive `handle_data` resource fallback delegates
   into the same unlocked resource path. The older candidate-scan branch that
   awaited link locks while holding the global transport handler is no longer
   present in the resource receive path.

   Thirty-first slice landed: Reticulum path-table persistence now snapshots
   interface path metadata under the interface-manager lock before acquiring the
   transport handler. Saving the path table no longer awaits the interface
   manager while the global transport handler is held.

   Thirty-second slice landed: resource response packet scratch storage is no
   longer global transport-handler state. Resource proof and link-resource
   handling now use local response vectors inside their short resource-manager
   critical sections, reducing shared mutable state in the receive path.

   Thirty-third slice landed: `ResourceManager` now lives behind its own
   mutex-protected handle instead of being directly embedded as mutable
   transport-handler state. Resource send commit/confirm, retry polling,
   link-close cleanup, proof handling, and completion finalization can clone the
   resource-manager handle and release the global transport handler before
   mutating resource state.

   Thirty-fourth slice landed: a bounded `ResourceManagerLane` now serializes
   resource-manager commands over an internal worker queue. Link-close cleanup
   and periodic retry/outgoing polling run through this lane, so timer-driven
   resource maintenance has bounded backpressure and no longer directly locks
   the raw resource-manager state from the transport maintenance task.

   Thirty-fifth slice landed: outbound resource send commit and dispatch
   confirmation now go through `ResourceManagerLane`. Public resource sends no
   longer lock the raw resource-manager mutex directly for pending/outgoing
   state transitions, which keeps the lane as the serialization boundary for
   outbound resource lifecycle updates.

   Thirty-sixth slice landed: inbound resource proof handling, link-resource
   packet handling, and resource completion finalization now go through
   `ResourceManagerLane`. The raw resource manager is no longer stored directly
   on `TransportHandler`; production resource state transitions are serialized
   behind the bounded lane, with only tests using a test-only manager handle.

   Thirty-seventh slice landed: `ResourceManagerLane` no longer awaits per-link
   mutex acquisition inside the single resource-manager worker. Link-resource
   packet handling snapshots the link packet-encryption context before queuing
   the resource-manager command, so one busy link cannot stall unrelated
   resource-manager commands behind the lane worker.

   Thirty-eighth slice landed: interface dispatch now plans matching interface
   sends while holding the interface-manager lock, then performs bounded tx
   queue enqueue waits after dropping that lock. Slow or full interface queues
   no longer block unrelated interface-manager operations, queued-announce
   release uses the same split, and a transport test verifies neither the
   transport handler nor interface manager stays locked while a full tx queue
   waits.

   Thirty-ninth slice landed: maintenance cleanup now snapshots live interface
   modes under the interface-manager lock, drops that lock, then prunes stale
   path and tunnel state under the transport handler lock. Interface cleanup no
   longer holds the interface-manager guard while waiting for transport state,
   and a regression test exercises that lock-order boundary directly.

   Fortieth slice landed: public link fanout helpers now snapshot in-link and
   out-link handles under a short transport-handler lock, then build link data
   and channel packets after dropping the global handler. A blocked per-link
   mutex can no longer pin the transport handler during fanout, and a regression
   test covers the blocked-link case.

   Forty-first slice landed: outbound public-key encryption context setup now
   snapshots the outbound destination handle under a short transport-handler
   lock, waits for the destination mutex outside the global handler, then
   briefly reacquires transport state only for ratchet lookup. A blocked
   destination can no longer pin the transport handler before encryption worker
   dispatch, and a regression test covers that lock boundary.

   Forty-second slice landed: tunnel synthesis now resolves interface full
   hashes and transport identity in separate short critical sections instead of
   holding the transport handler while waiting for the interface manager. Tunnel
   synthesize packet handling is synchronous because it only mutates in-memory
   path state, and a regression test verifies a blocked interface-manager lane
   cannot pin the global transport handler.

   Forty-third slice landed: the packet receive loop now drops the shared
   interface receiver mutex immediately after `recv()` returns, before fixed
   destination handling, duplicate filtering, announce validation, link work, or
   local delivery can wait on transport state. A regression test holds the
   transport handler during packet processing and verifies the interface receive
   lane is no longer pinned behind that slow path.

   Forty-fourth slice landed: resource retry maintenance now builds retry
   request packets only for links whose mutexes are immediately available, and
   skips busy links until the next retry tick instead of blocking unrelated
   resource advertisements. A focused regression test verifies a busy link does
   not prevent independent advertisement packets from being returned for
   dispatch.

   Forty-fifth slice landed: outbound-link proof activation now skips busy
   outbound link mutexes while still processing ready links, so an unrelated
   blocked link cannot delay RTT generation for a proof that can be handled
   immediately. A regression test holds one outbound link locked and verifies a
   ready link still emits its RTT response.

   Forty-sixth slice landed: link data proof generation now skips busy outbound
   link mutexes while still processing ready outbound links, preserving the
   directly addressed in-link path but preventing an unrelated blocked outbound
   candidate from delaying proof generation. A regression test holds one
   outbound link locked and verifies a ready link still emits its proof packet.

   Forty-seventh slice landed: fallback outbound-link destination lookup now
   skips busy nonmatching outbound candidates instead of awaiting every
   candidate while resolving by link id. A regression test holds one nonmatching
   candidate locked and verifies a later ready candidate is still found.

   Forty-eighth slice landed: link maintenance scheduling now skips busy link
   mutexes while scanning for the next channel-retry or watchdog deadline, so
   one blocked link cannot delay the scheduler from observing ready-link retry
   work. A regression test holds one link locked and verifies a ready link's
   retry deadline is still selected.

   Forty-ninth slice landed: the link-check sweep now skips busy input and
   output link mutexes instead of awaiting every link during timeout, watchdog,
   retry, and cleanup work. A regression test holds one output link locked and
   verifies an unrelated closed output link is still removed during the same
   sweep.

   Fiftieth slice landed: public link fanout helpers now skip busy input and
   output link mutexes while collecting link data and channel packets, and
   outbound dispatch accounting skips busy link candidates instead of awaiting
   them after interface enqueue. A regression test holds an output link locked
   and verifies public fanout returns immediately instead of waiting on it.

   Fifty-first slice landed: outbound link-id lookup now skips busy output-link
   candidates instead of awaiting every candidate from resource-send, channel,
   `find_out_link`, and `TransportChannel` lookup paths. A regression test
   holds one nonmatching output link locked and verifies a later ready link can
   still be found immediately.

   Fifty-second slice landed: link-bound dispatch now skips a busy link mutex
   while resolving the bound ingress interface instead of waiting after resource
   preparation. A regression test holds the target link locked and verifies
   dispatch returns immediately with a no-route outcome.

   Fifty-third slice landed: outbound link reuse now skips the status check
   when the existing destination-scoped output link mutex is busy, returning
   the existing link handle instead of waiting before link-request creation.
   A regression test holds the existing output link locked and verifies
   `Transport::link` returns the same handle immediately.

   Fifty-fourth slice landed: duplicate filtering for link-request proofs now
   skips the inbound link status wait when that link mutex is busy, allowing the
   packet receive loop to continue instead of stalling behind one busy inbound
   link. A regression test holds the inbound link locked and verifies unlocked
   duplicate filtering returns immediately.

   Fifty-fifth slice landed: direct inbound link-data handling now skips a busy
   inbound link mutex instead of awaiting it after the global transport handler
   is dropped. A regression test holds the addressed inbound link locked and
   verifies `handle_data` returns immediately.

   Fifty-sixth slice landed: link-request proof forwarding now skips validation
   when the learned destination mutex is busy instead of awaiting it during
   proof handling. A regression test holds the destination locked and verifies
   proof handling returns immediately without forwarding the proof.

   Fifty-seventh slice landed: receipt proof validation for link proofs now
   skips a busy link mutex instead of awaiting it during proof handling. A
   regression test holds the addressed inbound link locked and verifies
   unlocked receipt validation returns immediately with no receipt hash.

   Fifty-eighth slice landed: receipt proof validation for single output and
   input destinations now skips busy destination mutexes instead of awaiting
   them during proof handling. Regression tests hold each destination kind
   locked and verify unlocked receipt validation returns immediately with no
   receipt hash.

   Fifty-ninth slice landed: unlocked announce processing now skips an existing
   destination when its mutex is busy instead of waiting in the receive path
   before identity-drift checks. A regression test holds the existing
   destination locked and verifies validated announce handling returns
   immediately.

   Sixtieth slice landed: the resource manager lane now skips link-resource
   packets when their link context mutex is busy instead of waiting before
   queueing manager work. A regression test holds the link locked and verifies
   `handle_link_packet` returns immediately with no resource side effects.

   Sixty-first slice landed: outbound single-destination encryption setup now
   skips a busy destination mutex instead of waiting to copy identity material.
   A regression test holds the destination locked and verifies outbound send
   returns immediately with `DroppedEncryptFailed`.

   Sixty-second slice landed: local path-request response generation now skips
   a busy input destination mutex instead of awaiting it while handling fixed
   path-request traffic. A regression test holds the destination locked and
   verifies `handle_path_request_unlocked` returns immediately without a
   response.

   Sixty-third slice landed: local link-request proof generation now skips a
   busy input destination mutex instead of awaiting it while handling link
   request traffic. A regression test holds the destination locked and verifies
   `handle_link_request_unlocked` returns immediately without creating an input
   link.

   Sixty-fourth slice landed: resource send preparation workers now skip busy
   link mutexes instead of blocking a worker thread on `blocking_lock`. A
   regression test holds an active link locked and verifies `send_resource`
   returns immediately with no committed outbound resource state.

   Sixty-fifth slice landed: single-destination inbound decrypt workers now
   skip busy destination mutexes instead of blocking a worker thread on
   `blocking_lock`. A regression test holds the destination locked and verifies
   local single-destination handling returns immediately without emitting
   received data.

   Sixty-sixth slice landed: link-resource decrypt workers now skip busy link
   mutexes instead of blocking a worker thread before decrypting resource
   control packets. A regression test holds the link locked and verifies
   resource handling returns immediately without creating receiver state.

   Sixty-seventh slice landed: link-resource completion workers now skip busy
   link mutexes instead of blocking while building completion proofs. A
   regression test holds the link locked and verifies completion returns
   immediately with a connection error.

   Sixty-eighth slice landed: direct inbound delivery persistence can now queue
   message inserts onto the storage writer lane without waiting for the writer
   reply. Regression tests verify scheduled inserts flush through the writer
   lane and queued raw inbound acceptance remains visible after a later
   synchronous receive flush.

   Sixty-ninth slice landed: bridge-backed outbound delivery now runs through
   a small worker pool instead of a single delivery worker, so one slow bridge
   delivery does not block later delivery execution. A regression test blocks
   the first bridge call and verifies the second delivery still starts.

   Seventieth slice landed: SDK event-sink dispatch now runs through a small
   worker pool instead of a single sink worker, so one slow webhook/MQTT-style
   sink does not serialize unrelated sink publishes. A regression test blocks
   one sink and verifies a second sink still publishes before the first is
   released.

   Seventy-first slice landed: the message-store writer lane now uses a
   bounded queue, and scheduled hot-path writes use non-blocking enqueue. This
   keeps direct inbound delivery persistence and scheduled retention pruning
   from parking caller threads behind a saturated storage writer; a regression
   test fills a stalled writer queue and verifies the scheduled write returns
   `WouldBlock`.

   Seventy-second slice landed: inbound resource packet handling now uses
   non-blocking enqueue into the resource-manager lane. If that bounded lane is
   saturated, packet handling skips the resource packet instead of awaiting
   queue capacity in the receive path; a regression test fills a stalled
   resource-manager lane and verifies handling returns immediately with no
   side effects.

   Seventy-third slice landed: the SDK perf budget gate now runs the
   `reticulum-rs-core` parity hotpath benchmarks and enforces latency and
   throughput budgets for announce validation plus identity sign, verify,
   encrypt, and decrypt. This puts the weakest Rust-vs-Python crypto gaps under
   the same regression gate as daemon delivery, resource preparation, bridge
   scheduling, and event-sink dispatch.

   Seventy-fourth slice landed: `.github/workflows/performance.yml` now runs
   `cargo xtask ci --stage sdk-perf-budget-check` as a dedicated PR job and
   uploads the SDK budget report artifacts. Latency/throughput budgets are no
   longer only a local advisory gate.

   Seventy-fifth slice landed: LXMF payload and `Message::to_wire` encoding now
   serialize from borrowed title/content/stamp slices and borrowed field values
   instead of cloning into intermediate payload structures before MessagePack
   encode. Compatibility tests compare precomputed-signature packing against
   `WireMessage::pack` and verify the signer branch. The fast Rust/Python gate
   now reports `LXMF message encode` at 531.30x and `LXMF large message encode`
   at 287.55x over Python, so the advisory stretch targets were lifted to 500x
   and 300x respectively.

   Seventy-sixth slice landed: LXMF payload decode now tries typed MessagePack
   tuples for the common binary-field payload shape before falling back to
   generic `rmpv::Value` parsing for compatibility cases such as string title
   or content fields. Focused Criterion runs show about a 30% decode-path
   improvement, and the fast Rust/Python gate now reports `LXMF message decode`
   at 1434.24x and `LXMF large message decode` at 1024.34x over Python. The
   advisory decode stretch targets were lifted to 1500x and 1000x respectively.

   Seventy-seventh slice landed: resource request handling now decodes the
   hot request-window payload through a borrowed `ResourceRequestRef` instead
   of allocating an owned requested-hash vector before dispatching sender
   responses. `ResourceSender` now iterates the borrowed hash chunks directly
   through `handle_request_ref_into`. A benchmark fixture bug was fixed after
   this slice: the fixture had not confirmed outbound advertisement dispatch,
   so the reused request-window benchmark was measuring a trivial map miss.
   With the corrected fixture, the full report profile measures the reused
   request-window path at about 122.70 ns and `Reticulum resource request
   window` at 18.68x over Python. The advisory stretch target is 20x, keeping
   the focus on real sender-state reuse rather than the invalid earlier result.

   Seventy-eighth slice landed: inbound single-destination decrypt now has an
   `*_into` path from ratchet/private-key decrypt through `Destination` and the
   transport worker. The worker decrypts directly into `PacketDataBuffer`
   instead of allocating a `Vec` and copying it back into packet storage before
   delivery. A focused `rns_rpc/accept_inbound` Criterion run improved by about
   5.3%, and the fast Rust/Python gate now reports daemon inbound delivery at
   10.98x over Python, so the advisory daemon delivery stretch target was
   lifted from 5x to 10x.

   Seventy-ninth slice landed: LXMF payload encoding now has a manual
   MessagePack fast path for the common no-fields shape. It writes the fixed
   array, f64 timestamp, binary title/content, nil fields, and optional binary
   stamp directly instead of routing that hot case through Serde. Byte-for-byte
   compatibility tests compare the fast path against the previous Serde
   encoding, including nil values and bin16 large content. Focused Criterion
   runs improved `lxmf_core/message_to_wire` by about 49.6% and
   `lxmf_core/large_message_to_wire` by about 48.4%; the fast Rust/Python gate
   now reports `LXMF message encode` at 1071.03x and `LXMF large message encode`
   at 553.30x over Python. The full report profile confirms the gain at
   1063.23x and 555.88x over Python, so the advisory encode stretch targets
   were lifted to 1000x and 500x respectively.

   Eightieth slice landed: timer-driven resource-manager maintenance now uses
   non-blocking enqueue for retry polling and link-state cleanup. If the
   bounded resource-manager lane is saturated, retry polling skips that tick
   and link-state cleanup is deferred to a later maintenance pass instead of
   making link maintenance wait behind queued resource work. Regression tests
   hold the resource manager busy, fill the lane, and verify both operations
   return within the short timeout.

   Eighty-first slice verified the worker-lane budget gate after the
   resource-manager saturation change. `cargo xtask ci --stage
   sdk-perf-budget-check` passes and writes `target/criterion/bench-budget-report.txt`
   with the current daemon and worker-lane budget profile: `rns_rpc_accept_inbound`
   at 25.29 us p50, `rns_rpc_send_message_v2_bridge_schedule` at 143.41 us p50,
   `rns_rpc_event_sink_dispatch` at 607.24 ns p50, and
   `rns_transport_resource_prepare_send` at 107.58 us p50. A later run also
   covers worker-process local/stdio comparisons for resource completion and
   outbound encryption, including `reticulumd_worker_local_outbound_encrypt`
   at 60.49 us p50 and
   `reticulumd_worker_stdio_outbound_encrypt_round_trip` at 102.95 us p50.
   This confirms the active perf CI budget covers the async-isolation surfaces
   we are optimizing: inbound accept, bridge scheduling, event-sink dispatch,
   resource preparation, and process-boundary overhead for representative
   worker jobs.

   Eighty-second slice extended the performance workflow with a manual
   `workflow_dispatch` Rust/Python aggregated report job. Pull requests still
   run the fast speedup floor gate, while explicit performance investigations
   can now produce repeated-run `python-impl-report/report.json`,
   `python-impl-report/report.txt`, per-run comparison artifacts, and isolated
   resource measurements directly from CI without making every PR pay the full
   report cost.

   Eighty-third slice started the optional multi-process bridge without
   changing the default runtime: `transport::worker_boundary` now defines a
   process-safe coarse-job contract and backend trait for announce validation,
   outbound encryption, single-destination decrypt, resource preparation, and
   resource completion. The contract serializes packet wire bytes, hashes, and
   byte buffers rather than local mutex-protected transport objects, giving the
   later local/remote worker swap a stable shape to build on. The boundary now
   also has versioned request/response envelopes with per-job timeouts plus
   explicit timeout and cancellation errors, matching the process-boundary rule
   that deadlines and cancellation semantics must exist before supervisor work.
   The envelope codec rejects unsupported protocol versions on both encode and
   decode, so remote worker rollout failures become explicit boundary errors
   instead of silently mixing incompatible contracts. `WorkerClient` now wraps
   backend submission with per-job timeout enforcement and result-id
   correlation checks, giving local and future remote workers the same deadline
   semantics. The same client can now process encoded request bytes into
   encoded response bytes, which gives future pipes, sockets, or supervised
   child processes a single codec and error-mapping path to reuse. Encoded
   worker request and response envelopes are capped at 16 MiB each so future IPC
   transports inherit explicit memory backpressure instead of accepting
   unbounded payloads. The worker boundary also defines a 4-byte big-endian
   length-delimited frame format for pipe or socket transports, with incomplete
   and oversized frames rejected before MsgPack envelope decode. Async
   `read_worker_frame` and `write_worker_frame` helpers now provide the
   canonical pipe/socket I/O path and reject oversized lengths before payload
   allocation. `handle_worker_frame` now reads one framed request, dispatches it
   through `WorkerClient`, and writes one framed response, so a future child
   process service loop can reuse a tested single-request primitive.
   `serve_worker_frames` repeats that primitive until EOF and reports the
   handled request count, giving a later supervised worker process a minimal
   reusable serve loop. `serve_worker_frames_until_cancelled` adds the same
   loop with a cancellation token so a supervisor can stop worker serving
   cleanly without waiting for another frame. Serve loops now return a
   `WorkerServeSummary` with handled count and stop reason (`eof` or
   `cancelled`), which gives later supervisor policy enough signal to decide
   whether a worker exited normally or was deliberately stopped. `reticulumd
   --worker-stdio` now provides a hidden child-process entrypoint for the framed
   worker protocol on stdin/stdout. It handles the first concrete remote worker
   job, `ValidateAnnounce`, by validating real packet wire bytes and returning
   destination identity material, name hash, app-data, and ratchet output
   through the framed response. The transport crate can rebuild a normal
   `ValidatedAnnounce` from that enriched result and rejects inconsistent
   identity/name/address-hash tuples. `TransportConfig` can now carry an
   optional announce `WorkerBackend`; the inbound announce path uses it when
   present and falls back to the existing bounded single-process blocking pool
   when no backend is configured or when a remote/process backend fails. That
   keeps worker-process outages from dropping otherwise valid announces.
   `reticulumd` passes the process-backed backend into transport startup when
   `--worker-process-count` is enabled, so announce validation is the first
   measured hot path with a real local-vs-process worker selector.
   The daemon module now also has a
   `WorkerStdioProcess` client primitive for spawning that child, submitting
   framed requests, reading framed responses, and shutting it down with a
   timeout. Child processes use kill-on-drop as a defensive cleanup path for
   dropped daemon contexts or abandoned pools. `WorkerStdioPool` adds
   ownership over multiple stdio worker children, with explicit zero-worker
   rejection, so the supervisor work can route jobs through a pool instead of a
   single child. Pool dispatch now scans for an idle child before waiting on the
   round-robin slot, so one busy process does not block jobs that another child
   can accept immediately. Parent-side submit timeouts now kill and replace a
   stalled child so one wedged process cannot hold its pool slot forever; tests
   assert both idle-slot selection and replacement with a different process id.
   `WorkerStdioPoolBackend` adapts that pool into the existing `WorkerBackend`
   trait by encoding jobs into worker envelopes, submitting them through the
   stdio pool, and decoding responses back into worker results. Outbound
   single-destination encryption is now the second worker-backed hot path:
   `TransportConfig` can carry an outbound `WorkerBackend`, `reticulumd`
   passes the process-backed pool into it, `reticulumd --worker-stdio` handles
   real `OutboundEncrypt` jobs, and transport falls back to the bounded local
   encryption lane if the remote/process worker fails. Single-destination
   inbound decrypt is now the third worker-backed hot path: the worker boundary
   carries packet wire bytes plus private identity bytes for the trusted local
   child, returns plaintext and `ratchet_used`, and transport falls back to the
   local bounded decrypt lane when the process worker cannot handle the job.
   Resource completion now has a serializable completion snapshot and the
   worker `ResourceComplete` job carries the real completion fields instead of
   an underspecified parts list, preparing the resource worker boundary without
   moving resource-manager state across IPC prematurely. Resource completion
   snapshots now convert through the worker job type with a tested wrong-kind
   guard, so future process handlers have one central mapping path. Resource
   completion worker results now return resource proof plus payload fields
   rather than prebuilt link proof packet bytes, keeping link packet
   construction on the router side where link context is owned. Resource
   completion also snapshots `LinkPacketContext` before entering the blocking
   worker, so completion/decrypt/proof work no longer holds or reacquires the
   full link mutex inside the worker thread. Resource send preparation now uses the same
   link-context snapshot shape before entering its blocking worker, so resource
   hashing, encryption, chunking, and advertisement construction also avoid
   carrying the full link mutex into CPU-heavy work. The local outbound
   encryption lane now applies immediate bounded backpressure with
   `try_acquire_owned`; when all crypto permits are busy, encrypted send
   returns `DroppedEncryptFailed` instead of waiting behind the saturated crypto
   lane. The regression test
   `outbound_encryption_saturation_returns_without_waiting` holds every crypto
   permit and verifies a new encrypted send completes within the short timeout.
   Resource send preparation now applies the same immediate bounded
   backpressure: if all local resource-prepare workers are busy,
   `send_resource` returns `ConnectionError` without awaiting a worker permit.
   `send_resource_returns_when_prepare_workers_are_saturated` covers that
   behavior. The remaining local wire CPU lanes now use the same immediate
   permit admission: single-destination decrypt, link-resource decrypt, and
   resource completion all return promptly under saturated worker lanes, covered
   by `local_single_destination_decrypt_returns_when_workers_are_saturated`,
   `link_resource_decrypt_returns_when_workers_are_saturated`, and
   `resource_completion_returns_when_workers_are_saturated`. Interface TX
   dispatch now also treats full bounded interface queues as immediate
   backpressure instead of waiting for enqueue capacity; `full_interface_tx_queue_returns_without_waiting`
   covers that behavior. Resource manager lane commit and outbound dispatch
   confirmation also use immediate bounded admission; when the lane is full and
   the manager lock is busy, commit returns `ConnectionError` and confirmation
   returns without waiting. `resource_lane_commit_prepared_send_fails_when_manager_queue_is_full`
   and `resource_lane_confirm_dispatch_returns_when_manager_queue_is_full`
   cover those cases. Physical interface receive loops now also use
   non-blocking admission into the shared transport RX queue: UDP, serial, TCP
   client, and native BLE drop on a full RX queue instead of awaiting enqueue
   capacity inside their reader loops.
   Interface worker process bridges now use the same non-blocking queue
   admission at the parent/child channel boundary: child inbound frames are
   dropped when the parent transport RX queue is full, and UDP child outbound
   frames are dropped when the child TX queue is full. The regression tests
   `interface_rx_forwarder_drops_when_transport_channel_is_full` and
   `udp_interface_worker_stdio_drops_outbound_when_tx_channel_is_full` cover
   those bridge paths. Parent-side interface-worker manager dispatch now uses
   `InterfaceManager::send_from_shared`, which snapshots interface dispatch
   work under the manager mutex and performs bounded queue admission after
   releasing that mutex. The security-review gate now enforces this regression
   class with `run_no_async_mutex_send_across_await_check`, which rejects
   production `.lock().await.send(...)` and `.lock().await.dispatch...`
   patterns.
   Hidden CLI options `--worker-process-count`,
   `--worker-process-timeout-ms`, and `--worker-process-command` now parse and
   validate the future process-worker pool configuration: `0` workers keeps
   the process pool disabled, while enabled pools require a nonzero timeout.
   The command override lets operators point the pool at a separately built
   framed worker binary instead of only the current daemon executable.
   Hidden `--worker-process-tcp` adds an externally managed worker endpoint:
   the daemon can connect the same framed worker pool to a supervisor-owned TCP
   worker instead of owning the child process lifecycle. Unix-socket endpoint
   plumbing is also present on Unix targets behind `--worker-process-unix-socket`.
   The first interface-process boundary artifact now exists at
   `crates/libs/rns-transport/src/transport/interface_boundary.rs`, with
   versioned msgpack envelopes for inbound/outbound packet events, health, and
   shutdown. It reuses the worker frame helpers, caps interface events at 1 MiB,
   rejects unsupported protocol versions, and round-trips packet wire bytes into
   the existing `RxMessage`/`TxMessage` types. Async read/write helpers now
   exchange those envelopes over `AsyncRead`/`AsyncWrite` streams, with tests
   covering stream round trips and oversized frame rejection before allocation.
   The same module now has a reusable serve loop that reports EOF, shutdown, or
   cancellation separately for future supervisor policy, plus channel forwarders
   that bridge the existing interface TX/RX MPSC channels to the framed
   interface-worker stream. The benchmark suite now includes
   `rns_transport/interface_worker_ipc_envelope`; the latest budget report
   measured p50 1166.34 ns and keeps interface IPC overhead inside CI gates.
   `reticulumd --interface-worker-stdio` is now a hidden child-process
   entrypoint that consumes framed interface envelopes until shutdown/EOF, with
   integration coverage against a real spawned daemon child. A parent-side
   `InterfaceWorkerStdioProcess` primitive can spawn an interface worker
   executable, send framed events, read framed stdout events, and shut it down
   cleanly. Its channel bridge now connects the existing transport
   `InterfaceTxReceiver`/`InterfaceRxSender` pair to the child stdin/stdout
   stream and reports sent/received counts plus the stop reason, with a
   bidirectional unit test covering outbound transport-to-child and inbound
   child-to-transport forwarding. The bridge also has a cancellation-aware
   variant for supervisor shutdown: cancellation writes a shutdown frame to the
   child and returns a `Cancelled` stop reason instead of waiting for more
   interface traffic. A registration helper now installs the process-backed
   bridge as a normal `InterfaceManager` channel, with coverage for manager
   outbound dispatch into the child and inbound child packets returning through
   the manager receiver. Hidden daemon startup flags can now create those
   process-backed interface channels during bootstrap, keep the bridge handles
   alive in `BootstrapContext`, and expose the runtime interface metadata
   through `list_interfaces`. `daemon_status_ex` also exposes
   `interface_worker_processes` with
   enabled/count/shutdown-timeout/restart-backoff/live/stopped plus aggregate
   child restart and child error counters for operator visibility. The
   process-backed bridge now supervises child lifecycle directly: EOF, child
   shutdown, or transient bridge errors restart the child after the configured
   backoff while preserving the parent-side manager channel, and explicit
   cancellation or transport-channel close still stop the bridge. The hidden
   interface-worker
   child entrypoint now has a UDP mode that runs the UDP interface loop behind
   framed stdio, forwards outbound interface events into the UDP tx channel,
   and emits UDP rx packets as framed inbound events with the parent-assigned
   interface address. Configured UDP interfaces now select this process-backed
   runtime when interface worker processes are enabled, while preserving their
   normal config/listing shape. Configured serial interfaces now have the same
   process-backed runtime option, with hidden child flags carrying the serial
   device, baud rate, line settings, MTU, and reconnect timing. Configured TCP
   client interfaces now also select the process-backed runtime when interface
   worker processes are enabled, with hidden child flags carrying the remote
   connect address into a `TcpClient` loop behind framed stdio. Configured BLE
   GATT interfaces now also select the process-backed runtime, with hidden
   child flags carrying the adapter, peripheral, GATT characteristic settings,
   MTU, and reconnect timing. The BLE child path translates between the
   parent-assigned interface address and the child-local BLE manager channel.
   TCP server/listener process ownership now uses hidden
   `--interface-worker-tcp-listen` child mode when interface worker processes
   are enabled. Accepted TCP clients remain directly routable through
   `InterfaceManager::register_remote_iface_alias`, which registers each
   child/process-owned dynamic interface address on the parent host bridge tx
   channel. Configured-interface metadata now has daemon-level restart coverage:
   after a process-backed interface child exits and is replaced, status and
   interface listing preserve the original runtime address and process manager
   metadata.
   Bootstrap now retains an optional process-backed `WorkerBackend` in
   `BootstrapContext` when the hidden worker count is nonzero, so the pool
   lifetime is attached to the daemon context before individual hot paths opt
   into remote execution. `WorkerProcessRuntimeStatus` records
   enabled/count/timeout metadata for tests and future status reporting without
   requiring backend downcasts. `daemon_status_ex.worker_processes` now exposes
   that runtime selection to operators and tests.
   The `worker_stdio_process` integration test proves the protocol path against
   a real spawned child process, including multiple framed jobs served by one
   child before EOF; unwired job kinds still return explicit
   `BackendUnavailable` responses. Resource completion snapshots now also
   convert into typed `ResourceCompletionOutcome` values and
   `WorkerResultKind::ResourceCompleted`, and the stdio worker can complete
   resource jobs through the same backend used for announce, outbound encrypt,
   and single-destination decrypt. Link packet context is now snapshotted into a
   serializable worker payload, with tests proving a restored context can
   decrypt encrypted resource streams; the parent still builds the final proof
   packet on the router side that owns link state. Transport tests now prove
   resource completion uses a configured worker backend and falls back to the
   local bounded worker when that backend fails. The process pool now has
   request-level isolation coverage:
   `worker_process_pool_serves_idle_child_while_peer_child_is_stalled` holds one
   child blocked inside a framed request while another child serves a separate
   request promptly, and
   `worker_process_backend_serves_idle_child_while_peer_child_is_stalled`
   proves the same behavior through the `WorkerBackend` wrapper used by
   transport hot paths. Backend timeout replacement is covered by
   `worker_process_backend_replaces_timed_out_child_and_serves_next_request`.
   `WorkerStdioPool::snapshot` now reports worker count, idle/busy slots,
   request timeouts, and child replacements for basic managed-pool runtime
   health, and `daemon_status_ex.worker_processes` exposes those fields through
   a periodic live backend-status publisher. The backend timeout-replacement
   test now starts that publisher on a short interval and waits for the status
   snapshot to show timeout/replacement counters without a manual refresh.
   The packet receive loop now spawns announce worker validation off-lane, and
   `packet_receive_continues_while_announce_worker_is_stalled` verifies a
   stalled announce worker does not stop the receive loop from draining and
   broadcasting the next unrelated packet. Worker-process restart coverage now
   includes `worker_process_restart_does_not_corrupt_daemon_message_state`,
   which proves a timed-out child can be replaced while daemon SDK CAS state,
   announce-derived route/discovery state, outbound message content, and
   delivered receipt state remain intact.

2. Resource state reuse

   The benchmark suite shows resource request-window reuse is orders of
   magnitude faster than rebuilding request-window state. Push reuse deeper into
   per-link and per-resource paths, and avoid per-message scratch allocation.

3. Async isolation

   Use per-peer, per-link, or per-destination queues with bounded backpressure.
   A slow peer, storage write, resource transfer, or proof/crypto job must only
   slow its own lane.

4. Hot-path allocation control

   Keep adding `*_into` and buffer-reuse APIs for packet, message, and resource
   construction. Avoid generic `rmpv::Value` in hot paths; keep it at
   compatibility boundaries.

   Current report-profile gaps from `cargo run -p xtask --
   python-impl-bench-report`: identity encrypt/decrypt are still the weakest
   timing wins at 2.60x and 3.14x, announce create/validate and identity
   sign/verify sit around 4.35x-6.66x, and daemon inbound delivery clears its
   raised 10x stretch in the full report profile at 10.87x. The corrected
   borrowed resource-request fixture now reports 18.95x over Python in the full
   report profile, so the resource stretch target is held at 20x while further
   work looks for real reusable sender-state wins. After the no-fields
   MessagePack encode fast path, the full report profile clears the raised
   encode stretch targets at 1081.38x for small-message encode and 560.41x for
   large-message encode. Core Fernet decrypt now uses the same padding-helper
   path as transport Fernet, moving report-profile identity decrypt from 1.92x
   to 3.14x over Python while the SDK perf budget keeps the new p50 under
   budget. The core identity encrypt Criterion split now measures
   `rns_core/identity_encrypt` at about 106.26 us total, with
   `rns_core/identity_encrypt_key_schedule` at about 58.39 us and
   `rns_core/identity_fernet_encrypt_only` at about 54.69 us, so the remaining
   encrypt cost is roughly half asymmetric key schedule and half Fernet payload
   encryption. The SDK perf budget runner now carries matching diagnostic
   budgets for both split probes. The key-schedule side is now split further:
   the latest SDK perf budget report measures `rns_core_identity_ephemeral_keypair`
   at 13.33 us p50, `rns_core_identity_x25519_exchange` at 35.39 us p50, and
   `rns_core_identity_hkdf_sha256` at 1.89 us p50. That makes X25519 curve work,
   not HKDF, the dominant key-schedule floor. `x25519-dalek` already resolves
   with precomputed tables, so there is no obvious curve dependency feature left
   to flip. A controlled `fernet-aes128` probe measured 2 KiB Fernet encrypt at
   about 42.67 us versus 54.27 us by default, and decrypt at about 16.21 us
   versus 19.42 us; the feature now propagates through the public crate graph
   but remains opt-in until the compatibility/security decision is made. The
   matching decrypt split is now budgeted
   too: `rns_core_identity_decrypt` measures about 57.43 us p50, with
   `rns_core_identity_decrypt_key_schedule` at 37.55 us and
   `rns_core_identity_fernet_decrypt_only` at 19.66 us in the latest SDK perf
   budget report. Decrypt is therefore mostly asymmetric/HKDF work, while
   Fernet verify/decrypt remains a meaningful secondary cost.
   The next crypto-throughput probe now measures batch scheduling directly:
   `rns_core/identity_encrypt_batch_64` reports about 6.79 ms for 64 serial
   encryptions, while `rns_core/identity_encrypt_batch_64_parallel` reports
   about 1.38 ms on the current machine. Decrypt shows the same shape:
   `rns_core/identity_decrypt_batch_64` reports about 3.68 ms serial and
   `rns_core/identity_decrypt_batch_64_parallel` about 0.82 ms. Those probes
   are now in the SDK perf budget table as throughput guards, giving the next
   implementation pass concrete evidence that a bounded parallel crypto
   scheduler can improve batch throughput without making process IPC the
   default path.
   The worker process contract now has batch crypto envelopes for outbound
   encryption and single-destination decrypt jobs. The stdio child handles both
   batch forms, and the real spawned-child integration test proves batch
   encrypt/decrypt traffic can cross the framed worker protocol before runtime
   paths start coalescing jobs automatically. The stdio child now maps batch
   items across local worker threads, and the SDK perf budget includes the
   end-to-end batch process comparison: `reticulumd_worker_local_outbound_encrypt_batch_64`
   currently measures about 3.90 ms p50 for serial local batch completion,
   while `reticulumd_worker_stdio_outbound_encrypt_batch_64_round_trip`
   measures about 0.96 ms p50 through one framed child batch request.
   The inbound decrypt side now has the same daemon-worker evidence:
   `reticulumd_worker_local_inbound_decrypt_batch_64` measures
   about 4.31 ms p50 for 64 serial decryptions, while
   `reticulumd_worker_stdio_inbound_decrypt_batch_64_round_trip`
   measures about 1.01 ms p50 through one framed child batch request. Both
   batch encrypt and decrypt worker paths are now protected by SDK perf
   budgets. Runtime outbound encryption and inbound single-destination decrypt
   now use bounded crypto batch lanes that coalesce queued requests into the
   measured batch worker paths with non-blocking admission and ordered
   per-packet replies.
   Local outbound encryption also writes directly into the packet's fixed
   `PacketDataBuffer` through `ratchets::encrypt_for_public_key_into`, removing
   the previous temporary heap ciphertext and copy back into packet storage on
   the local worker path.

5. Optional process isolation

   Add multi-process mode only after the worker-lane version is measured. Split
   by fault and scaling boundary: interface workers, control/API, router state,
   and heavy crypto/resource workers. Avoid per-packet IPC. The concrete
   rollout gates, process boundaries, and IPC rules are tracked in
   `docs/plans/2026-05-10-multiprocess-scaling-plan.md`.

## Done Criteria

- `cargo run -p xtask -- python-impl-bench-compare` enforces minimum Rust vs
  Python p50 speedups for encode/decode, announce validation, crypto, resource
  request-window handling, and daemon inbound delivery accept.
- `.github/workflows/performance.yml` runs both `cargo xtask ci --stage
  sdk-perf-budget-check` and `cargo xtask ci --stage python-impl-perf-gate` on
  pull requests, preserving budget and comparison artifacts.
- `cargo run -p xtask -- python-impl-bench-report` produces an operator report
  with minimum and stretch speedup targets.
- `cargo run -p xtask -- sdk-perf-budget-check` includes core announce and
  identity crypto budgets plus daemon inbound, bridge-backed delivery
  scheduling, message storage writer-lane insertion, resource preparation, and
  event-sink dispatch budgets via the
  `rns_core/announce_validate`, `rns_core/announce_validate_batch_64`,
  `rns_core/identity_*`, `rns_rpc/accept_inbound`,
  `rns_rpc/send_message_v2_bridge_schedule`, `rns_rpc/message_store_insert`,
  `rns_transport/resource_prepare_send`,
  `rns_transport/resource_worker_ipc_envelope`, and
  `rns_rpc/event_sink_dispatch` benchmarks.
- Crypto and resource stretch gaps are tracked from benchmark evidence, not
  broad “Rust is faster” claims.
- Daemon performance work includes non-blocking isolation evidence: bounded
  queues, no global locks held across `.await`, and tests or benches showing one
  slow lane does not block unrelated lanes.
- Message storage writer admission uses non-blocking queue admission for
  scheduled and synchronous write commands, with saturation tests proving a full
  storage lane returns `WouldBlock` instead of blocking the caller.
