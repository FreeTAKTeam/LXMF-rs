# Optional Multi-Process Scaling Plan

Status: planned after worker-lane stabilization

This plan defines the optional multi-process runtime that can be added after the
single-process async daemon and worker-pool model is measured and stable. The
goal is production-scale isolation, not per-packet IPC.

## Entry Criteria

- `cargo run -p xtask -- python-impl-bench-report` has current artifacts and
  identified stretch gaps.
- `cargo run -p xtask -- sdk-perf-budget-check` passes with budgets for inbound
  accept, bridge scheduling, resource preparation, and event-sink dispatch.
- Worker-lane tests cover slow event sinks, slow outbound delivery, storage
  writes, resource preparation/completion, and busy link or destination locks.
- No known hot receive path holds the global transport handler lock across an
  `.await`.

## Process Boundaries

1. Router/control process

   Owns route tables, path state, RPC control, SDK state, status snapshots, and
   compatibility-facing APIs. This process remains the operator-facing daemon
   endpoint. The first boundary artifact is
   `crates/libs/rns-rpc/src/rpc/control_boundary.rs`: a versioned msgpack
   envelope for existing `RpcRequest`/`RpcResponse` values, health messages, and
   shutdown. It uses a bounded 4-byte length-delimited frame with a 4 MiB cap,
   rejects unsupported protocol versions, rejects oversized frame lengths before
   payload allocation, and round-trips over any async stream. This preserves the
   public RPC method surface while giving a future router/control split a
   process-safe control-plane contract. A reusable `serve_control_router` loop
   now reads framed control requests, dispatches them through a caller-provided
   RPC handler such as `RpcDaemon::handle_rpc`, writes framed responses, and
   stops cleanly on shutdown or EOF. `rns_rpc/control_boundary_envelope` budgets
   the frame plus msgpack overhead for representative `daemon_status_ex`
   request/response traffic; the latest SDK perf budget report measured p50
   12582.94 ns and passed. `reticulumd --control-router-stdio` now exposes this
   as a real hidden child-process runtime: it bootstraps the daemon, serves
   framed control requests through `RpcDaemon::handle_rpc`, keeps stdout clean
   for protocol frames, preserves request ids on internal errors, and exits on a
   shutdown envelope. The integration test
   `reticulumd_control_router_stdio_serves_rpc_until_shutdown` covers the
   spawned-process request/shutdown lifecycle. The daemon side also has a
   `ControlRouterStdioProcess` client primitive that spawns a framed
   control-router child, sends an RPC request, verifies the response sequence,
   and performs explicit shutdown. Its unit test drives the primitive against a
   framed mock child, and `request_with_timeout` kills a stalled child and
   returns `RequestTimedOut` so parent callers do not block indefinitely.
   `ControlRouterStdioPool` adds a small fixed-size supervisor primitive over
   those children: it prefers idle children, reports idle/busy/timeout and
   replacement counters, and replaces a timed-out child before serving the next
   request. `daemon_status_ex.control_router_processes` exposes that health
   shape through the public daemon status snapshot, matching the existing worker
   process status fields. These tests give a concrete parent/supervisor
   building block before the full router/control topology is selected. Hidden
   `--control-router-process-*` startup options now let `reticulumd` create
   and retain that pool, pass child daemon paths such as `--db`, and route
   eligible read-only `/rpc` requests through it. The routed set is deliberately
   conservative (`status` first) so richer parent-owned status snapshots and
   mutating RPCs cannot diverge parent and child daemon state. Route tests also
   cover the isolation case where a stalled routed child times out while
   unrelated in-process RPCs keep completing.
   `reticulumd/control_router_stdio_status_round_trip` measures the same path
   through a reused real child process; the latest SDK perf budget report
   measured p50 52691.29 ns, p95/p99 56141.06 ns, throughput 18978.47 ops/sec,
   and passed. The routed production-facing HTTP path is now budgeted separately as
   `reticulumd/control_router_http_status_routed_round_trip`; it starts a real
   parent daemon, sends external HTTP `/rpc` status calls over loopback TCP, and
   measures the parent HTTP parse/auth/route work plus the child control-router
   pool hop. The latest report measured p50 183932.89 ns, p95/p99 195243.42 ns,
   throughput 5436.77 ops/sec, and passed.

2. Interface worker processes

   Own physical or virtual interfaces and frame parsing/encoding. They forward
   normalized packets and interface telemetry to the router/control process.
   Interface workers should be restartable without losing SDK state.

3. Crypto/resource worker processes

   Own heavy CPU/resource jobs when local worker pools are not enough:
   identity encrypt/decrypt, signature verification, announce validation,
   resource preparation, resource completion, and decompression. They operate on
   coarse jobs, not individual packet dispatch decisions.

4. Storage worker process

   Owns SQLite and retention cleanup for deployments where storage stalls must
   be fault-isolated from routing and interface receive loops. The existing
   in-process storage writer lane is the compatibility model for this boundary.

## IPC Rules

- Do not send every packet over IPC when an in-process lane is sufficient.
- Use bounded request queues with explicit backpressure and drop/error policies.
- Prefer coarse work units: accepted inbound messages, resource jobs, announce
  validation batches, bridge-delivery jobs, and storage mutations.
- Keep SDK/RPC contracts stable; process placement must not change public
  method names, payload shapes, receipt semantics, or LXMF interoperability.
- Every process boundary needs timeout, cancellation, and restart behavior
  specified before implementation.

## IPC Library Decision

ZeroMQ is a useful candidate for operator-managed worker fabrics, but it is not
the default boundary for the current daemon. The Rust choices split between
`zmq`, which binds to `libzmq` and brings a native system dependency plus
blocking socket semantics that need careful Tokio isolation, and the native
`zeromq` crate, which the upstream ZeroMQ Rust page still describes as not
production-ready. Older Tokio wrappers such as `tokio-zmq`, `async_zmq`, and
`tmq` are not a strong fit for a core daemon dependency.

ZeroMQ-style DEALER/ROUTER or PUSH/PULL patterns could help if we later want
externally managed, language-agnostic worker pools with built-in queueing,
high-water marks, multipart messages, and reconnect behavior. It would not fix
the measured local hot-path bottlenecks: current evidence shows local bounded
lanes beat process round trips for resource completion and outbound encryption,
and the weakest Rust-vs-Python gap is identity crypto work rather than the IPC
transport.

Default recommendation: keep the current length-delimited msgpack envelopes over
Tokio stdio/TCP/Unix-socket transports for first-party daemon children. Add
ZeroMQ only as an optional worker transport after a direct benchmark proves it
beats or materially simplifies the existing framed transport for coarse jobs.
For first-party local IPC alternatives, prefer Tokio Unix sockets or the
`interprocess` crate's Tokio local sockets on platforms where portability across
Unix sockets and Windows named pipes matters. For schema-heavy external APIs,
evaluate Cap'n Proto or tonic separately; for huge resource payloads where copy
cost dominates, evaluate shared-memory IPC instead of a message-queue library.

## Rollout Order

1. Extract interfaces behind a process-safe boundary while preserving the
   current in-process interface traits. The first boundary artifact is
   `crates/libs/rns-transport/src/transport/interface_boundary.rs`: a
   versioned msgpack envelope for interface-worker events. It carries normalized
   inbound packet events (`RxMessage` as interface id, packet wire bytes, and
   source), outbound packet events (`TxMessage` as direct/broadcast target plus
   packet wire bytes), health snapshots, and shutdown. The envelope uses the
   same 4-byte length-delimited frame helpers and codec error type as the
   crypto/resource worker boundary, caps each interface event at 1 MiB, rejects
   unsupported protocol versions, and round-trips packet wire bytes back into
   the existing in-process `RxMessage`/`TxMessage` types. Async read/write
   helpers now move those envelopes over any `AsyncRead`/`AsyncWrite` stream,
   giving the future interface child process the same pipe-ready primitive used
   by worker stdio processes. A reusable serve loop processes envelopes until
   EOF, explicit shutdown, or cancellation, returning a handled count and stop
   reason for future supervisor policy. Channel forwarders now adapt the
   existing `InterfaceTxReceiver` and `InterfaceRxSender` MPSC channels to the
   framed interface-worker stream, so the future child runtime can attach to the
   current in-process interface plumbing without changing transport semantics.
   `rns_transport/interface_worker_ipc_envelope` now budgets the frame plus
   msgpack overhead for representative interface events; the latest report
   measured p50 1150.77 ns and passed the SDK perf budget. `reticulumd` now has
   a hidden `--interface-worker-stdio` child-process entrypoint that consumes
   framed interface envelopes until shutdown or EOF, giving the interface
   protocol a real spawned-process lifecycle before physical interface drivers
   are moved out of the router process. The daemon side now also has an
   `InterfaceWorkerStdioProcess` client primitive that spawns the child, sends
   framed events, reads framed envelopes from child stdout, and shuts it down
   with an explicit shutdown envelope. Its `run_channel_bridge` helper
   multiplexes the current transport `InterfaceTxReceiver` and
   `InterfaceRxSender` channels over that child process boundary and reports
   sent/received counts plus the stop reason, so the next step can move
   physical interface drivers behind the same bridge without changing the
   transport channel contract. The cancellation-aware bridge variant lets a
   supervisor stop that task explicitly and sends a shutdown frame to the child
   before returning a `Cancelled` stop reason. `spawn_interface_worker_bridge`
   now registers the process-backed bridge as a normal `InterfaceManager`
   channel, so router send/receive paths can exercise the process boundary
   through the same manager contract used by in-process interfaces. Hidden
   daemon startup options can now register one or more process-backed interface
   bridges, optionally using a separately built interface-worker command, and
   `BootstrapContext` owns the bridge handles so cancellation remains tied to
   daemon lifetime. The restart delay is configurable through
   `--interface-worker-process-restart-backoff-ms`.
   `daemon_status_ex.interface_worker_processes` now reports
   enabled/count/shutdown-timeout/restart-backoff/live/stopped plus aggregate
   child restart and child error counters for that runtime, with a periodic
   publisher that updates live/stopped and restart/error counts after child
   exit. The
   registered bridge task now also restarts an interface child after EOF,
   child shutdown, or transient bridge error, using that configured backoff and
   preserving the parent-side `InterfaceManager` channel. Cancellation and
   transport-channel close remain terminal stop reasons for the bridge. The
   hidden interface-worker child entrypoint now has a UDP mode
   (`--interface-worker-udp-bind` plus optional forward address) that runs the
   UDP interface loop behind the framed stdio boundary and uses the
   parent-assigned interface address for inbound events. Normal configured UDP
   interfaces now choose this process-backed worker path when interface worker
   processes are enabled, preserving the existing `type = "udp"` config record
   while changing the runtime manager to `interface_worker_process`. Serial now
   follows the same configured-interface pattern: the hidden child entrypoint can
   rebuild `SerialInterface` from CLI flags, and normal configured serial
   records can run through the process-backed bridge when interface worker
   processes are enabled. TCP client now uses the same bridge: the hidden child
   mode accepts `--interface-worker-tcp-connect`, runs `TcpClient` behind the
   framed stdio boundary, and configured `type = "tcp_client"` records select
   the process-backed path when interface worker processes are enabled. BLE
   GATT now follows the same configured-interface pattern with hidden
   `--interface-worker-ble-*` child flags. Because BLE is spawned through its
   own child-local `InterfaceManager`, the stdio bridge translates the
   parent-assigned interface address to the child-local BLE channel for
   outbound frames and rewrites child-local inbound frames back to the parent
   address before they cross the process boundary. TCP server/listener now uses
   hidden `--interface-worker-tcp-listen` child mode when interface worker
   processes are enabled. Because each accepted TCP client becomes a
   child-local interface, `InterfaceManager::register_remote_iface_alias`
   registers process-owned child addresses on the parent host bridge tx channel
   so direct parent sends to those child addresses route back across the
   interface-worker boundary. Configured-interface metadata is also covered
   across child restart: the daemon-level restart test verifies
   `daemon_status_ex` and `list_interfaces` keep the original configured
   interface shape, runtime address, startup status, and process manager marker
   after the first child exits and the supervisor replaces it.
2. Move resource and crypto workers behind an interchangeable local/remote
   worker trait. The first boundary artifact is
   `crates/libs/rns-transport/src/transport/worker_boundary.rs`: a serializable
   coarse-job contract for announce validation, outbound encryption,
   single-destination decrypt, resource preparation, and resource completion.
   It intentionally carries packet wire bytes, fixed hashes, and byte buffers
   instead of process-local locks or transport objects. The request/response
   envelopes include a protocol version, per-job timeout, job id, and explicit
   timeout/cancellation errors so remote workers can enforce deadlines without
   changing the default in-process worker pools. The envelope codec rejects
   unsupported protocol versions on encode/decode so independent worker
   processes fail closed during rolling upgrades or operator misconfiguration.
   `WorkerClient` wraps any backend implementation with the same deadline and
   result-correlation behavior the later remote IPC backend must preserve. It
   also exposes an encoded request/response path so a future pipe, socket, or
   supervisor transport can share the exact same envelope validation and error
   mapping as the in-process adapter. Encoded request and response envelopes are
   capped at 16 MiB each, making oversized worker payloads explicit boundary
   errors instead of unbounded IPC memory growth. The same module defines a
   4-byte big-endian length-delimited frame for pipe or socket transports, with
   incomplete and oversized frames rejected before envelope decode. Async
   `read_worker_frame`/`write_worker_frame` helpers provide the canonical I/O
   path for pipes or sockets and reject oversized lengths before payload
   allocation. `handle_worker_frame` reads one framed request, dispatches it
   through `WorkerClient`, and writes one framed response, giving a future child
   process service loop a tested single-request primitive. `serve_worker_frames`
   repeatedly applies that primitive until EOF and reports the handled request
   count, giving a future supervised worker process a minimal reusable serve
   loop. `serve_worker_frames_until_cancelled` adds the same loop with a
   cancellation token so a supervisor can stop a worker cleanly without waiting
   for another inbound frame. Serve loops return a `WorkerServeSummary` with the
   handled request count and stop reason (`eof` or `cancelled`) so supervisors
   can distinguish clean stream shutdown from requested termination.
   `reticulumd --worker-stdio` is now a hidden child-process entrypoint that
   speaks the framed worker protocol on stdin/stdout. Its first concrete job is
   `ValidateAnnounce`, which decodes packet wire bytes, runs real Reticulum
   announce validation, and returns destination identity material, name hash,
   app-data, and ratchet output through the same framed response envelope. The
   transport crate can rebuild a normal `ValidatedAnnounce` from that enriched
   worker result and rejects mismatched identity/name/address-hash tuples before
   route state is updated. `TransportConfig` now accepts an optional announce
   `WorkerBackend`, and the announce receive path uses that backend for
   validation when configured, falling back to the existing bounded
   single-process blocking worker pool when no backend is configured or when a
   remote/process backend fails. That keeps worker-process outages from
   dropping otherwise valid announces while preserving bounded local
   backpressure. `reticulumd` passes the process-backed backend into transport
   startup when `--worker-process-count` is nonzero, making announce validation
   the first hot path with a real local-vs-process worker selector. Unwired job
   kinds still return explicit
   `BackendUnavailable` results. The same daemon module now includes an
   internal `WorkerStdioProcess` client that spawns `reticulumd --worker-stdio`,
   submits framed requests, reads framed responses, and shuts the child down by
   closing stdin and waiting with a timeout. Child processes are configured with
   kill-on-drop so a dropped daemon context or abandoned pool cannot leave
   hidden stdio workers running. `WorkerStdioPool` layers a small process pool
   over those child-process clients, rejects zero-worker construction
   explicitly, and scans for an idle child before waiting on the selected slot,
   giving the later supervisor a concrete routing primitive that is not pinned
   behind one busy child. `WorkerStdioPoolBackend`
   adapts that pool back into the existing `WorkerBackend` trait by encoding
   jobs as worker requests, submitting them through the stdio pool, and decoding
   worker responses. That keeps the local-vs-process worker choice behind the
   same backend seam the in-process worker code already uses. Outbound
   single-destination encryption now uses that same backend choice: transport
   submits `OutboundEncrypt` jobs to the configured process backend, falls back
   to the bounded local encryption lane on backend failure, and the hidden stdio
   worker returns encrypted packet wire bytes through `PacketWire`.
   Single-destination inbound decrypt now follows the same pattern for trusted
   local process workers: transport submits packet wire bytes plus private
   identity bytes, the stdio worker returns plaintext plus `ratchet_used`, and
   failures fall back to the bounded local decrypt lane. Resource completion now
   has a serializable completion snapshot and a `ResourceComplete` worker job
   shape that carries all completion fields needed for a later process worker,
   instead of depending on private receiver internals at the IPC boundary.
   Snapshot conversion is centralized on the worker job type with wrong-kind
   validation so resource process handlers do not duplicate mapping logic.
   Resource completion worker results carry resource proof plus payload fields
   rather than prebuilt link proof packet wire, leaving link packet construction
   in the router/control side that owns link context.
   Completion now also snapshots link packet crypto context before blocking
   work, which is the in-process form of the future process handoff. Resource
   send preparation now uses the same link-context snapshot before hashing,
   encrypting, chunking, and building advertisements in a worker.
   Hidden daemon
   process-worker submits now enforce a parent-side timeout around the child
   pipe exchange; a timed-out child is killed and replaced so one stalled
   process cannot hold its pool slot forever. Hidden daemon
   options `--worker-process-count`, `--worker-process-timeout-ms`, and
   `--worker-process-command` now parse the future process-worker pool shape;
   count `0` keeps the process-backed pool disabled, and enabled pools require
   a nonzero timeout. The command override is the first operator hook for
   running a separately built framed worker binary instead of the current
   daemon executable.
   Hidden `--worker-process-tcp` now lets the daemon connect that same framed
   worker pool to externally managed TCP workers; Unix targets also have
   `--worker-process-unix-socket` endpoint plumbing for supervisor-owned local
   workers.
   Normal daemon bootstrap now retains an optional process-backed
   `WorkerBackend` in `BootstrapContext` when `--worker-process-count` is
   nonzero, using the configured worker command or the current executable as
   the hidden stdio worker child. The backend is held live for later hot-path
   routing while the default count `0` preserves the existing single-process
   behavior. `BootstrapContext` also
   exposes `WorkerProcessRuntimeStatus` (`enabled`, `worker_count`,
   `timeout_ms`) so tests and future status/RPC surfaces can report the selected
   runtime without downcasting the backend trait object. `daemon_status_ex` now
   reports the selected runtime under `worker_processes`, including whether the
   pool is enabled, worker count, and per-job timeout.
   `crates/apps/reticulumd/tests/worker_stdio_process.rs`
   proves a real child `reticulumd --worker-stdio` process can receive one
   framed announce-validation request over stdin, return a framed response over
   stdout, keep serving multiple framed jobs in the same child, and exit cleanly
   on EOF. Resource completion now has a serializable
   snapshot, a typed completion outcome, and an outcome-to-worker-result mapping
   for `WorkerResultKind::ResourceCompleted`; the child-process
   `ResourceComplete` handler can complete resource jobs using a serialized link
   decrypt context, while the parent keeps proof-packet construction on the
   router/control side that owns link state. Transport tests cover the
   configured resource worker selector and local fallback when the worker
   backend fails.
3. Add a storage-worker transport that preserves the existing writer-lane
   ordering semantics.
4. Add a supervisor mode that starts all child processes and reports health in
   daemon status.
5. Add independent-process mode for operators that want to run and restart
   workers separately.

## Required Evidence

- Benchmarks compare single-process worker lanes against multi-process mode for
  the same workloads. `reticulumd/worker_local_resource_complete` and
  `reticulumd/worker_stdio_resource_complete_round_trip` now compare the same
  unencrypted resource completion payload locally and through a reused real
  `reticulumd --worker-stdio` child process. The latest SDK perf budget report
  measured 24300.00 ns p50 for local completion and 74250.00 ns p50 for the
  process round-trip. Outbound encryption is measured the same way:
  `reticulumd_worker_local_outbound_encrypt` reports 60491.00 ns p50, while
  `reticulumd_worker_stdio_outbound_encrypt_round_trip` reports 97051.50 ns
  p50 through the reused child.
- IPC overhead is reported separately from crypto/resource/storage work.
  `rns_transport/resource_worker_ipc_envelope` is the first dedicated IPC
  envelope budget, covering frame plus msgpack decode/encode for representative
  resource completion request/response payloads. The latest SDK perf budget
  report measured 3575.71 ns p50 for that envelope.
  `rns_transport/interface_worker_ipc_envelope` separately measures interface
  worker frame plus msgpack overhead at 1150.77 ns p50.
- Integration tests show one stalled worker process does not block unrelated
  packet receive, RPC status, event-sink dispatch, or outbound delivery lanes.
  Current pool tests prove a timed-out child process is killed and replaced
  with a new process id, that the replacement can serve the next framed worker
  response, and that a busy pool slot does not stop selection of an idle child.
  `worker_process_pool_serves_idle_child_while_peer_child_is_stalled` proves
  request-level process isolation inside one pool by holding one child blocked
  while another child serves a separate framed request promptly.
  `worker_process_backend_serves_idle_child_while_peer_child_is_stalled` proves
  that same isolation through the `WorkerBackend` wrapper used by transport.
  `worker_process_backend_replaces_timed_out_child_and_serves_next_request`
  proves timeout replacement through that same backend wrapper.
  `WorkerStdioPool::snapshot` exposes worker count, idle/busy slots, request
  timeouts, and child replacements so independently managed pools have basic
  runtime health evidence under tests. `daemon_status_ex.worker_processes`
  reports those health fields to operators and is periodically refreshed from
  the live backend pool while the daemon runs; the timeout-replacement backend
  test now waits for that publisher to surface updated counters without a
  manual refresh.
  `packet_receive_continues_while_announce_worker_is_stalled` proves the
  transport receive loop keeps draining unrelated packets while announce worker
  validation is stalled.
  Focused daemon tests also hold a worker-process submit stalled while
  `daemon_status_ex` returns promptly through `RpcDaemon::handle_rpc` and while
  a real event-sink bridge receives a published event, and while bridge-backed
  `send_message_v2` starts outbound delivery, covering the first unrelated RPC,
  event-dispatch, and outbound-delivery integration paths.
  `worker_process_restart_does_not_corrupt_daemon_message_state` now covers the
  first worker-restart state check: it mutates SDK configuration, outbound
  message, receipt, and announce-derived route/discovery state while a worker
  child is stalled, lets the pool replace that child, and verifies the
  replacement responds while SDK CAS state, message content, announce state, and
  delivered receipt remain intact.
- Restart tests show interface and worker process restarts do not corrupt route,
  SDK, receipt, or message-store state.
- The default remains single-process async unless multi-process mode wins on a
  measured production-scale workload or provides required fault isolation.

## Current Placement Decision

The current measurements support a conservative default: keep local bounded
lanes as the default execution path and use process mode as an operator-selected
isolation boundary.

| Boundary | Current p50 evidence | Default decision |
| --- | ---: | --- |
| Message storage writer lane | `rns_rpc_message_store_insert`: 11.41 us | Keep in-process writer lane by default; split only for storage fault isolation. |
| Resource completion worker | local 24.25 us, stdio round trip 67.04 us | Keep local bounded worker by default; process mode is useful when a resource job may fault or stall. |
| Outbound encryption worker | local 64.65 us, stdio round trip 97.89 us | Keep local bounded crypto lane by default; process mode is justified for isolation or CPU partitioning, not latency. |
| Worker IPC envelope | resource envelope 3.56 us | Acceptable for coarse jobs; too expensive for per-packet routing decisions. |
| Interface IPC envelope | interface envelope 1.17 us | Acceptable for physical interface ownership and restart isolation. |
| Router/control status process | stdio status 55.23 us, routed HTTP status 179.34 us | Keep mutating/control-state ownership in parent; route only conservative read-only calls until ownership is split. |

Promotion rule: a process boundary can become a default only after an equivalent
local-vs-process benchmark shows either a latency/throughput win on a
production-scale workload or the operator requirement is fault isolation that
local lanes cannot provide. Until then, process mode stays optional and
coarse-grained.
