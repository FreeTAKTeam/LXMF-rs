# Item 4 configured-root lifecycle cutover

This refines the remaining family gate in [TRANSPORT.md](TRANSPORT.md) after
`a0772c1d`; it does not add another PR or change the numbered plan. The three
discovery process cases and finite deeper relay matrix are already qualified.
The first ten TCP/Backbone/UDP/Pipe/Serial/KISS/I2P rows now have the declared
software lifecycle evidence below. Other family rows and whole-item integration
remain open.

## Concrete gap and owner

The original gap was that the startup dispatcher returned native handles and
effects but primarily seeded the mutation worker's map for the four named-management
kinds and private Backbone clients. Discovery projected that map, so other
configured bootstrap roots missed both disable and restore. Unsupported seed keys
were skipped before the restore diagnostic. Copying the four-kind spawn match
would have preserved the gap and duplicated carrier defaults, status and shutdown
rules. The board below distinguishes completed ownership/disable work from the
remaining family composition work.

The existing interface mutation worker remains the sole lifetime/mutation owner.
Reuse the configured dispatcher and its typed startup batch. Retain each real
configured root's original recipe index, native address and owned effects in the
existing managed entry. Do not recover ownership from public `_runtime` or names.
Validation-only, rejected and unavailable-backend outcomes have no invented root.
Named public management remains restricted to its four kinds.
Non-four-kind configured roots retain protected startup-only origin during
four-kind Apply reconciliation, so omission cannot remove them.

## Cutover and research rationale

First extract a callable per-recipe dispatcher without changing behavior. The
initial loop delegates with the original index, label, selected-listener context,
storage/identity/reconnect inputs and Auto plan builder. Do not invoke a reindexed
one-element whole configuration. TCP/Backbone listener construction precedes the
dispatcher today; restoration must reuse that constructor explicitly, including
override/shadowing, shared-local classification, tuning and readiness.

The runtime context retains the narrow startup policy/data actually needed;
do not retain an entire CLI argument value or copy RPC credentials into recipes.
Move unique startup/shutdown effects through ordinary owned values. Rust's
[mem::take](https://doc.rust-lang.org/std/mem/fn.take.html) transfers a field's
ownership without requiring Clone. Tokio's
[JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html)
documentation distinguishes dropping a handle from awaiting task completion;
retain stop/join evidence. Research uses current primary docs, while compilation
continues against the existing locked runtime and repository toolchain.

Status/control/shutdown effects must cut over together. Auto's retained runtime
must stop its listener/scheduler supervisor before host-root removal. I2P retains
its original default storage and transport identity. Stream roots retain tunnel
synthesis/reconnect hooks. RNode/Weave management projections replace retired
handles while preserving unrelated bindings and ambiguous-name behavior.

The simplest complete status owner is the existing managed entry with a closed
enum of the already-supported native status handles. Demote the fixed startup
refresh vectors and detached per-family loops into one existing mutation-worker
publication tick, replacing the three dynamic hot-apply refreshers too. Preserve
the immediate first tick, two-second cadence and Delay missed-tick policy.
Discovery retains its separate five-second Skip monitor and exact
twelve-second down anchor. Do not add an actor, interface registry, trait
framework, parallel mutation path or speculative public configuration.

Before the ownership cutover, Auto's shutdown helper removed the manager root
itself. Stage 1 preserved that behavior. The completed retirement cutover now
stops the carrier supervisor, fences/captures the native tree, then lets Transport
remove routes and registrations before joining every captured worker. It preserves
the cleanup evidence even if a concurrent manager cleanup removes registrations.

## Sequential board

| Task | Main-agent output | Evidence / dependency |
| --- | --- | --- |
| Dispatcher preparation (locally verified) | Same initial behavior through one per-recipe dispatcher and existing typed batch | Existing configured-startup tests plus mixed ordered/disabled/failure characterization; preserve original indices, selected listener, Auto injection and all batch effects. Review/checkpoint before behavior changes |
| Trusted root/effect retention (locally verified) | Every actual configured root enters the existing owner with original recipe identity and native effects | Native identical unnamed KISS roots reproduce the missing disable; collision, inactive/failed repair and original-slot controls pass. No caller metadata origin |
| Status/control/shutdown cutover (locally verified) | Existing entry retains typed handles and unique Auto shutdown; one publication owner replaces old fixed loops | Single two-second publication, synchronous initial controls, old-handle rejection, cancelled shutdown recovery, admission fencing, all-worker joins and unrelated-root continuity pass. General restoration is qualified below |
| Restoration constructor preparation (locally verified) | Per-recipe context carries only the strict preflight boolean; selected TCP/Backbone/Local listener construction is callable from its existing adapter module | Config-only strict listener failure, independent carrier preflight policy, original-index rejection, override and missing-recipe controls preserve native ownership, labels, counts and fail-closed preparation. No restoration behavior changes yet |
| Trusted restoration (locally verified) | Initial and restoration dispatch share constructor/defaults/context, including selected listeners; the same worker owns reconnect completion | Reordered KISS exact authenticated traffic and unrelated TCP continuity pass. Public endpoint replacement restores one original recipe in the current slot. Failed listener attempts retire and retry; common/degraded status and first control visibility are preserved |
| Family composition (in progress) | Qualify the finite twenty-row inventory using its applicable native/software backends | The first ten TCP/Backbone/UDP/Pipe/Serial/KISS/I2P rows have the declared software trace qualification below. Remaining Auto, simulated radio/mesh, available BLE and shared-local cells remain open; failed/degraded outcomes stay explicit |
| Whole-item integration | Complete item-4 regressions, release checks, independent final review, one PR, exact-head hosted CI and normal merge | Existing PLAN.md/ACCEPTANCE.md gates; no item 5 work before merge |

Implementation stays in the main agent. PRE, correctness and maintainability POST
gate each bounded checkpoint. Start at the existing owner; if a carrier's effect
cannot be represented or stopped correctly, preserve its explicit incomplete cell
and repair the shared boundary before continuing. Do not mark every family
qualified from codec tests, expand physical scope or implement #650's absent
reticulum_ble native dual-role backend. Physical/RF/public-network evidence remains
outside the authorized software lane.

Focused gates are the affected bootstrap/worker/control suites, diagnostics after
error-handling changes, explicit include formatting, Clippy, boundaries, module
size and architecture. Broaden when the shared startup/lifetime cutover warrants
it; preserve the known release package conflict and every failed run honestly.

## Root, publication and retirement evidence

The configured startup batch now transfers native addresses, status handles,
unique Auto supervision, Meshtastic simulation and RNode/Weave control bindings
into the existing managed entries. Original recipe indices and mutable private
record positions keep identical unnamed occurrences distinct. Ambiguous four-kind
identities retain protected startup origin; a unique failed recipe can still be
repaired through native named attachment. Unsupported, disabled and validation-only
rows acquire no invented runtime. Public status remains a projection.

The worker's immediate first publication and two-second Delay cadence replace
the seventeen detached startup/hot-apply refresh loops. Control projections are
installed before the worker is returned and are replaced, including with an empty
projection, on retirement. Retained old RNode handles reject work after their real
receivers disappear. Those control tests use retained adapters and cancel-aware
workers; they do not prove radio packet I/O.

Bootstrap uniquely owns the mutation worker's join handle; the daemon retains
only its command bridge. Cancelling a shutdown await leaves the original handle
available for a later join. Admission closes before retirement starts. A red/green
control confirms that an already-retiring owner cannot acknowledge new queued work.
Every captured native/auxiliary task is joined or aborted and joined, even after
another task fails. Shutdown reports failure after cleanup instead of detaching.

Manager retirement now fences the current tree and takes its workers under one
borrow. Both ordinary inherited and separately registered virtual children reject
a cancelled parent. A held-Transport-handler regression forces manager cleanup
before route cleanup resumes: captured workers still join, UDP virtual-peer routes
are removed, and an unrelated root survives. Deferred activation keeps the existing
non-cancelling worker-take API.

The native KISS omission control failed before cutover; the actual duplicate roots
now both close at discovery disable. Additional controls cover protected duplicate
TCP roots, reordered records around a discovered detach, native policy metadata,
failed Pipe repair, initial/retired controls, timeout/panic cleanup and cancellation.
Full all-feature daemon validation passes 31 library and 630 binary tests; affected
transport validation passes 1,047 tests with its 12 existing ignored cases.

Auto's initial full-suite exclusive-bind failures were attributed before changing
the tests. A natural run retained an old socket inode with no parent descriptor.
A controlled native Auto/Pipe cohort delayed child `execve` entry by 50 ms and
identified those retired discovery descriptors in the pre-exec Pipe child. The
same child then successfully executed `cat`, closing its copied CLOEXEC descriptors.
This matches the Linux [fork](https://man7.org/linux/man-pages/man2/fork.2.html) and
[exec](https://man7.org/linux/man-pages/man2/execve.2.html) descriptor rules. Seven
bootstrap Auto cases now run in separate exact-test processes, each requiring one
passed non-ignored test. All native assertions and immediate exclusive-bind probes
remain; the same controlled cohort changes from four failures to seven passes.
There is no production delay, bind retry or authentication relaxation. Temporary
process diagnostics are removed; logs and syscall traces remain under `/tmp` as
recorded in PROGRESS.md. General trusted restoration is qualified below; the
twenty-family matrix remains open.

The subsequent constructor preparation removes the CLI argument borrow from the
per-recipe context and lower helpers. They receive only the existing strict
preflight boolean; initial entry wrappers still translate Args without retaining
it. The selected listener keeps its separate CLI-strict-or-config-panic policy.
Its existing adapter module now owns the shared start function, including prepared
policy, tuning, native shared-client classification, readiness and failed-record
reporting. The manager guard ends before the readiness await. Failed readiness
still retains the real root/status for cleanup; a missing selected recipe never
falls back to unconfigured spawning. Three native contract cases pass before and
after extraction; a fourth directly checks missing-recipe rejection and unrelated
root preservation. Full all-feature daemon validation passes 31 library and 634
binary tests. This provided structural preparation for the trusted restore task
qualified below.

## Trusted restoration implementation contract

The completed restoration task retains only the original selection snapshot, the two existing
strictness inputs, storage path, transport identity hash and reconnect events.
The same worker also owns the unique reconnect synthesizer completion, including
when discovery is disabled. A small input value may group existing worker inputs;
it does not create another actor or registry. Context with a unique join handle is
not cloned; the selection snapshot is ordinary immutable configuration.

Restoration invokes the same per-recipe dispatcher and selected-listener starter
with the original recipe index. Its server address is the newly created root,
never the retired one. Batch-to-outcome association accepts the original index
explicitly. Installing that outcome preserves the managed entry's existing origin
and current publication slot, which can differ after Apply/reorder. The old
four-kind restore match is removed without a production compatibility fallback.
Consume tunnel synthesis requests before converting the batch into owned roots;
preserve status, Auto, simulation and control effects together.

A documented startup failure rejects the attempt: settle any root created before
readiness failed and publish a failed/no-native outcome so a later eligible tick
can try again. A merely offline or connecting non-strict TCP worker is not such a
failure. Preserve explicit failed/degraded/validation-only outcomes without
inventing native presence. Retire every native root and its control projection
before dropping the retained reconnect sender, then bounded-join the synthesizer;
timeout aborts and awaits, and failure remains observable through the existing
unique shutdown completion.

The first regression uses real configured KISS TCP at recipe index 2. Public Apply
moves its occurrence to slot 1. A real discovered replacement disables it; closing
that peer/listener must restore a new native KISS root in slot 1. Before and after,
wrong-key and plaintext frames are rejected and exact authenticated native ingress
and egress pass. An unrelated TCP root keeps its address and exact bidirectional
traffic before, during and after retirement. The fixture uses production transport
startup and the actual mutation worker, starting discovery at its already-validated
observation boundary. Native packet deadlines are two seconds and monitor-driven
disable/restore connection bounds are eight seconds each; no retry-until-green or
timing changes qualify it. This is one software family composition case. Additional
selected-listener failure, context/storage, Auto/effect, reconnect shutdown and
remaining family evidence stay explicit.

## Trusted restoration evidence

The owned startup context now enters the existing mutation worker even with
discovery disabled. Restoration calls the shared dispatcher and selected-listener
starter, consumes tunnel effects and converts the batch by its original index.
The old limited restore match is deleted. Existing origin/current-slot bindings
survive; an entry replaced through public Apply is bound from its old owned
record before the original recipe replaces it. Control projections publish before
each live record, including when a subsequent SAM preparation waits.

The actual KISS omission failed at restore accept in
`/tmp/lxmf-item4-bootstrap-restore-kiss-reordered-red.log`; native authentication,
reordering, disable and unrelated traffic had already passed. The initial shared
factory passes the full connection/authentication case in `...-kiss-green.log`.
Independent review then identified missing common metadata, duplicate publication
after endpoint replacement and delayed control visibility. Strengthened controls
produce five failures in `...-review-complete-red.log` and pass all eighteen
focused cases in `...-review-green.log` (10.05 seconds). An earlier endpoint test
used an unnamed disabled sentinel and correctly retained it during Apply; that
fixture failure is recorded separately in `...-review-red.log`.

Known strict listener failure leaves no stale root and retries after the test-owned
endpoint is freed. Actual discovery-disabled TCP retirement joins the synthesizer;
injected panic and timeout/cancel cases verify cleanup ordering and retained
completion. I2P retains the original default storage path/identity. Validation-only
LoRa acquires no runtime and pending native Reticulum BLE remains degraded. The
Weave control barrier retains a real adapter against an absent temporary device
while a following test-owned SAM greeting waits; it proves first control lookup,
not serial/radio delivery or I2P interoperability. No physical/RF/public network
is used. Broad gates and final checkpoint results are recorded in PROGRESS.md;
the twenty-family matrix and whole-item integration remain open.

## TCP and Backbone software lifecycle evidence

Four declared production-startup/native wire cases in
`tests/bootstrap_tcp_family.rs` now pass:

- `tcp_named_listener_ifac_rotation_rollback_reload_and_reattach_preserve_ipv6`
- `configured_tcp_client_ifac_refusal_live_rotation_reconnect_and_restart`
- `configured_backbone_client_ifac_refusal_live_rotation_reconnect_and_restart`
- `configured_backbone_listener_ifac_failure_child_rotation_and_restart`

Each authentication phase rejects wrong-key and plaintext frames on its exact
native root/child before admitting the matching authenticated packet and emitting
exact authenticated HDLC bytes. TCP public Apply commits live key rotation before
named rollback snapshots it. Existing/new children inherit that policy. A real
occupied listener rejects replacement, old children reach EOF/removal, and the
restored child rejects the attempted key while accepting the committed key.
Successful reload, detach/attach, native refusal/reconnect and full configured
restart use the same owners. Single-root changes preserve the unrelated TCP
address and exact traffic. Full restart intentionally retires all roots and
proves a fresh Transport identity/connection; native addresses can repeat in a
fresh manager. Status/error assertions exclude test credentials and cleanup keeps
the existing ten-second join bound.

Native preference forwarding and exact `localhost` bind preparation had separate
pre-fix reds. The final wire trace selects `localhost.localdomain` locally because
this machine's `localhost` is IPv4-only; it requires both loopback families, an
independent IPv6 bind and actual IPv6 connections after rollback/reload/reattach.
Occupying the preferred IPv6 candidate at another port proves native IPv4 fallback.
A dual-`localhost` environment uses that first fixed candidate instead. This is
dual-host IPv6 wire evidence plus exact-`localhost` preparation evidence, not a
claim that the local hosts file supplies both families for `localhost`.

The minimal public listener correction forwards its existing preference flag,
retains hostname resolution when preferred, and preserves the default IPv4 alias.
Numeric endpoint display and the existing conservative wildcard guard now cover
tested compressed/bracketed/expanded IPv6 aliases before queueing. These extra
controls reproduce the independent review's collision finding before passing.
The existing tunneled-listener public rejection still preserves the active root
and traffic. The cohesive listener helper extraction passed 36 existing hot-apply
cases before behavior changed. Four native traces and three alias controls pass;
both independent source POST reviews approve. PROGRESS.md records exact gate
results and retained fixture failures. Other family rows and whole-item evidence
remain open; no physical/RF/public-network qualification is inferred.

## UDP software lifecycle evidence

The two declared cases in `tests/bootstrap_udp_family.rs` pass through configured
startup and the existing owned public mutation path:

- `udp_named_ifac_rotation_failed_reload_rollback_detach_and_reattach`
- `udp_multicast_ifac_peer_rotation_replacement_and_configured_restart`

Unicast Apply commits a live key change while retaining the root. A real occupied
replacement reports bind_failed, enters owned retirement, and keeps management
pending while unrelated exact TCP traffic succeeds. Rollback rejects the attempted
key and accepts the committed key. Changed-key reload, detach/attach and final
joined shutdown retire old roots and preserve unrelated addresses/traffic.

Multicast group egress uses the loopback-bound native root and an independent
receiver joined on loopback; exact IFAC bytes and source address are required.
Signed announcement ingress is unicast, with distinct source ports. Existing/new
virtual peers inherit key rotation; both directions authenticate and each direct
reply remains absent at the other peer throughout a two-second observation.
Replacement removes old child/peer-route dispatch authority. Fresh signed
announcements from the same destination identities rebind paths to current native
children; configured restart repeats that evidence with a fresh Transport identity.
Cached PathTable presence alone is not treated as dispatch authority or eagerly
erased; the pinned target separates interface removal from path maintenance too.

No production behavior changes are needed. Input announces respect the existing
ingress guard through an explicit one-Hz schedule; packet/path deadlines and
production defaults remain unchanged. Both source POST reviews approve; 2/2
native traces, 31 library/662 binary daemon tests and focused gates pass, with
retained fixture failures in PROGRESS.md. Other family rows, whole-item
integration/hosted CI and merge remain open. Native multicast group ingress and
physical/RF/public-network acceptance are not claimed.

## Pipe software lifecycle evidence

Two Linux-only native-process cases in `tests/bootstrap_pipe_family.rs` pass:

- `configured_pipe_spawn_failure_repair_preserves_ifac_and_process_ownership`
- `pipe_named_ifac_rotation_respawn_failed_reload_rollback_detach_and_reattach`

Configured startup first fails to spawn a missing owned executable. Its sanitized
native status reports respawning while the root retains IFAC and MTU. Publishing
the complete executable permits automatic recovery on the same root. Its actual
stdin/stdout carry independent peer HDLC/IFAC traffic with Pipe's eight-byte tag
default; both wrong-key and plaintext frames are counted/rejected in every phase.
The helper relays bytes without crypto and reports its PID only over a separate
loopback connection. /proc parent/argv verify direct ownership before socket EOF
and later PID disappearance establish reaping.

Public Apply rotates the key on the same root/process; native exit/respawn keeps
that committed policy. Malformed shlex command preparation preserves old PID,
root and exact traffic. Missing-executable named reload fails and restores the
committed command/key/512-byte MTU. Successful changed-key reload and detach/attach
use a 1024-byte MTU and reap every observed prior generation. Unrelated TCP roots
retain their address/traffic; final shutdown joins all native/reconnect workers
and reaps the last child. The explicit fixture respawn delay is 0.25 seconds;
production defaults and declared deadlines remain unchanged.

Both source POST roles approve, with no production behavior changes. Initial
native validation passes 2/2 in 4.03 seconds; full daemon validation passes 31
library/664 binary tests and focused gates. Other family rows and
whole-item integration/hosted CI/merge remain open. This is Linux software process
evidence, with no Windows or physical-carrier qualification.

## Serial and KISS software lifecycle evidence

Four Linux-only cases in `tests/bootstrap_serial_family.rs` pass:

- `configured_serial_ifac_open_failure_rotation_reopen_and_restart`
- `configured_kiss_ifac_open_failure_rotation_reopen_and_restart`
- `configured_ax25_kiss_ifac_open_failure_rotation_reopen_and_restart`
- `configured_kiss_tcp_ifac_refusal_rotation_reconnect_and_restart`

The PTY cases start from an absent owned alias and real native open errors. An
owned raw PTY permits same-root activation, exact authenticated first traffic and
native live key rotation. Alias removal precedes closing its master; a new error
must occur before a fresh PTY is published atomically. Retained-root reopen keeps
the rotated key. KISS TCP instead uses an unlistened bound socket for refusal,
then listens on that same socket and proves retained policy across EOF/reconnect.

Every authentication phase rejects/counts wrong-key and plaintext input before
exact matching traffic, with the carrier's eight-byte default. Fixed independent
initialization bytes are consumed on every KISS open/connection/restart; AX.25
data has the pinned APZRNS/N0CALL-0 sixteen-byte UI header. This does not qualify
AX.25 address validation. All cases perform a changed-key configured restart,
native MTU 512/1024 checks, sanitized status and joined cleanup. Unrelated TCP
address/traffic persists during single-root transitions. Full restart proves old
tree retirement and a fresh Transport identity rather than new manager hashes.

The Python PTY child owns the master and closes its extra slave descriptor. The
Rust fixture keeps one buffered stdout reader across the child's path greeting
and raw traffic. Expected EIO
does not block stdin EOF; output EOF and successful child wait prove fixture
cleanup. No helper framing/crypto, unsafe Rust, new dependency, public API or
production behavior change is introduced. Both source POST reviews approve;
four final native cases pass in 6.08 seconds, and full daemon validation passes
31 library/668 binary tests plus focused gates. At this Serial/KISS checkpoint,
twelve other family rows remained open. The current inventory includes the I2P
qualification below; whole-item integration/hosted CI/merge remain open.

## Configured I2P and shared stream teardown

Six native fake-SAM traces in `tests/bootstrap_i2p_family.rs` and its nested
shutdown module qualify outgoing and incoming software lifecycle composition:

- `configured_i2p_outgoing_ifac_refusal_rotation_reconnect_and_restart`
- `configured_i2p_incoming_ifac_failure_child_rotation_session_recovery_and_restart`
- `configured_i2p_shutdown_joins_stalled_native_destination_generation`
- `configured_i2p_shutdown_joins_stalled_incoming_session_create`
- `configured_i2p_shutdown_joins_pending_incoming_stream_accept`
- `configured_i2p_shutdown_joins_outgoing_setup_under_native_queue_pressure`

All stall fixtures await configured startup at an unavailable reserved endpoint
before listening, separating native setup from bootstrap's own preparation probe.
The outgoing trace counts actual native direct admissions until enqueue failure.
Default sixteen-byte IFAC, negative admission, exact traffic, retained parent/child
rotation, native MTU and unrelated traffic are checked. Two incoming children
remain isolated and authenticated across SAM session recreation; changed-key
restart retires the old tree and uses a fresh Transport identity. Actual key-file
contents establish persistence after recovery. Initial preparation metadata is a
bootstrap snapshot; restarted preparation has a new identity-bound key path.

Root and accept loops now own and join their nested workers. Private task sets
retain accepted streams across session recreation, while pinned ACCEPT futures
allow completion reaping without restarting a handshake. Incoming SAM waits and
native direct enqueue observe existing stop tokens. Value-free SAM errors retain
expired-session recovery and command context; private destination fallback is
also removed from the ready log. No public API, defaults, queues or dependencies
change.

POST review additionally finds established shared HDLC handoffs could block
these joins. The unchanged shared function is first extracted with a crate-visible
re-export. Five valid blocked-input/output regressions fail before correction,
then pass after cancellation-selected receive admission and packet/keepalive
write/flush. Queues and peers remain blocked through joined completion, followed
by EOF; no cancelled pending frame emits output success. Partial writes are
abandoned by retiring that stream generation. Existing error handling, framing,
IFAC order, pacing and watchdog behavior remain.

Both final source reviews approve the bounded correction. These are loopback and
in-memory software traces; public routers, other families, hardware and whole-item
integration/hosted acceptance remain open. Ten of twenty family rows have local
qualification; this does not complete item 4.
