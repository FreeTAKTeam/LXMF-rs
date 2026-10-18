# Item 4 transport execution contract

This refines item 4 of [PLAN.md](PLAN.md), without reducing its scope or creating
another PR. The starting main is `9c27747753488ed950c52a2c7717ddec14478786`.
The roadmap and parity matrices remain truth owners. References remain RNS
`99de23c040d507e3fefca19e87b182302902725d` for #608/#609 and
`7f2b3b9b524c9386316379af1313b43a5e4f7a5d` for #649; canonical RNS 1.5.2 is not
promoted. Rendering overlaps remain item 6.

## Owners and cutover

| Slice | Existing owner | Required cutover and evidence |
| --- | --- | --- |
| IFAC configuration/activation | InterfaceManager; daemon bootstrap/hot-apply | Resolve credential aliases consistently; install fallible policy before spawning root workers. Preserve owned worker handles, multicast peer routing, carrier defaults and shared-local plaintext exceptions. Prove first ingress/egress, not only eventual metadata. |
| IFAC lifecycle cells | Existing carrier workers and daemon status | Compose applicable startup/error, reconfiguration, child, stop/restart cells below. No transient plaintext fallback, credentials in status, or orphan workers. |
| Duplicate membership | PacketCache and production ingress/routing | Separate duplicate remembering from TTL source/proof tracking inside the same cache owner. Replace TTL membership with exact current/previous generations. Retain native owned proof keys and reference routing exceptions. |
| Queue admission | Existing inbound queues/cache | Final duplicate check, synchronous enqueue and remembering share one short critical section. Remember only on accepted enqueue; remove duplicate rollback state. Preserve path-request rollback and distinct filter/full counters. |
| Deeper recovery | Existing mixed-peer relay fixtures | Three predeclared clean topology runs, both directions and packet/Resource representations before/after intermediate replacement. Exact recipient content and sender completion are separate assertions. |
| Request receipts | Native Link/Transport request path | One exact Link-generation/request-ID owner, one sticky terminal outcome, cleanup on timeout/rejection/close/cancel. Cut over applicable waiters; no notification callback under native locks. |
| Discovery runtime | Existing interface mutation worker and lifecycle planner | Consume persisted/new discoveries, reserve endpoint/name atomically, qualify default/override, monitor and clean up. Internal synthesized records and explicit operator origin share the existing worker. |
| Live child policy | InterfaceManager setters | Propagate mode, outgoing, gravity and pacing to existing children; retain initial inheritance and current shared-config/IFAC propagation. Unrelated interface trees remain unchanged. |

Keep old public spawn APIs forwarding; do not spawn unowned tasks in the daemon.
ReticulumRuntimePolicy and DaemonConfig are exhaustive public structs: keep their
layouts/signatures. A small parsed-runtime wrapper with private fields supplies
the additive override. Parse existing typed views from one immutable file
snapshot, preserving invalid-interface/runtime-policy fallback.

## IFAC software family inventory

The first checkpoint implements the raw-record alias and hot-apply activation
parts of the first owner row. Three SDK v2/typed ZeroMQ to real UDP tests first
failed on plaintext admission or the wrong selected key, then passed with one
nonempty alias resolver shared by validation and runtime conversion. RPC legacy
aliases remain a separate compatibility extension; whitespace is not trimmed.

Shared-policy publication now acquires each unique IFAC state before changing
anything. Poisoned parent, inherited or plaintext-local child state rejects the
whole publication without changing healthy states or metadata. Prepared spawn
preserves old public wrappers, carrier defaults, control handles and the owned
worker tree. TCP client/listener, UDP unicast/multicast and Pipe hot-apply install
policy before constructing a worker. Configured bootstrap roots now use the same
preparation owner, including carrier-specific IFAC defaults, control handles,
multicast routing, Auto activation and available BLE backend paths. These changes
do not qualify every family cell below.

Shared-client attachment has an additional owned start gate. The manager retains
the task and cancellation token; Transport commits attached-client mode and sends
the gate under one handler guard without an await. Cancellation can interrupt
lock acquisition, dropped startup cancels the gated context, and manager stop can
join it while the handler remains blocked. Failed handoff preserves the previous
mode. Successful handoff is not a liveness lease and does not roll mode back when
one worker later stops. Automatic shared TCP/Unix accepted clients opt into the
frozen plaintext exception using the existing trusted synthetic-origin marker;
explicit Local records retain configured IFAC. Real stream and daemon regressions
cover first traffic, rejection, parent rotation, new children and cleanup. Live
non-IFAC child-policy propagation has the common owner qualification below;
family-specific lifecycle composition remains required.

The existing native mutation worker now also owns committed interface-record
publication. Opted-in SDK/RPC commands report queued/incomplete, and completion
events follow native installation, retaining legacy synchronous bridge behavior.
Reload completion context travels through that same command. Runtime metadata
from callers is discarded; disabled and failed records clear stale bindings.
Blocked/fast worker, queued-order, old-policy rollback, failed replacement,
disabled/unmanaged retention, malformed metadata and later detach/reload/TCP
commands have focused regressions. A completed command can report a failed
interface: actual carrier readiness stays explicit in its runtime status.

Each row covers applicable configured startup/error, live configuration or
reconnect, child inheritance/rotation, stop/restart and diagnostics. Existing
focused codec/worker tests are inputs, not a pass for every composition cell.
The first thirteen rows have the declared software lifecycle qualification below;
other rows remain open. Record exact test names/results when qualified.

| Family | Existing bounded evidence | Missing composition to qualify |
| --- | --- | --- |
| TCP server / Backbone | Four declared native traces plus alias admission controls qualify applicable configured error, child rotation, named TCP rollback/reload/detach/attach, restart and unrelated traffic | Locally qualified for these software lifecycle traces; exact-`localhost` IPv6 wire coverage depends on a dual resolver environment. Whole-item integration/hosted CI remain pending |
| TCP / Backbone client | Declared configured refusal, same-socket activation, live key change, reconnect, changed-key restart and exact unrelated traffic; preceding named constructor/hook traces | Locally qualified for these software lifecycle traces; whole-item integration/hosted CI remain pending |
| UDP unicast | Declared configured/named native trace qualifies committed live key rotation, actual failed bind/owned cleanup, authenticated rollback/reload/detach/attach and unrelated traffic; preceding raw IFAC/SDK guards remain inputs | Locally qualified for this software lifecycle trace; whole-item integration/hosted CI remain pending |
| UDP multicast | Declared native loopback group egress, signed unicast peer ingress, existing/new peer key rotation, peer-isolated replies and same-destination route rebinding after changed-key replacement/restart | Locally qualified for this software lifecycle trace; native group ingress is not claimed. Whole-item integration/hosted CI remain pending |
| Pipe | Two declared Linux native-process traces qualify missing-executable failure/repair, default IFAC authentication, live rotation/respawn, named rollback/reload/detach/attach, effective MTU, unrelated traffic and child reaping | Locally qualified for these Linux software lifecycle traces; whole-item integration/hosted CI remain pending |
| Serial | Declared Linux PTY trace qualifies native open failure, same-root activation/key rotation/reopen, exact authenticated HDLC, changed-key configured restart, native MTU and unrelated traffic | Locally qualified for this Linux software trace; whole-item integration/hosted CI remain pending |
| KISS / AX.25 serial | Two declared Linux PTY traces qualify native open failure/rotation/reopen/restart, independent initial command bytes and APZRNS/N0CALL-0 UI framing, exact IFAC traffic, native MTU and cleanup | Locally qualified for these Linux software traces; AX.25 address validation is not claimed. Whole-item integration/hosted CI remain pending |
| KISS TCP | Declared configured refusal/same-socket activation, initial command bytes, native live key rotation/reconnect, changed-key restart and exact authenticated/unrelated traffic | Locally qualified for this software trace; whole-item integration/hosted CI remain pending |
| I2P outgoing | Declared configured refusal, exact sixteen-byte-default IFAC, live child rotation, reconnect/restart, native MTU and unrelated traffic; actual native queue pressure and joined SAM setup shutdown | Locally qualified for these fake-SAM software traces; public router interoperability and whole-item integration/hosted CI remain pending |
| I2P incoming | Declared preparation failure/redaction/recovery, persisted key file, old/new accepted-child rotation and peer isolation, session recreation retaining both children, changed-key restart and native setup/stream shutdown | Locally qualified for these fake-SAM software traces; recovered startup metadata remains a snapshot. Public router interoperability and whole-item integration/hosted CI remain pending |
| AutoInterface | Two configured Linux manual-binding traces qualify native activation failure/partial cleanup, sixteen-byte IFAC, old/new virtual-child key and non-IFAC policy inheritance, peer-isolated configured-port routing and changed-identity restart; two queue-pressure regressions qualify joined receive-owner shutdown | Locally qualified for these software traces; manual bindings do not prove OS adoption or multicast ingress. Whole-item integration/hosted CI remain pending |
| Meshtastic | Configured unavailable-bearer failure, simulated multi-chunk native admission/status, live same-root key rotation, unrelated TCP and changed-identity/key restart; two blocked native queue regressions plus independent bearer IFAC controls | Locally qualified for these software traces; the daemon path requires simulation loopback. External device/RF and whole-item integration/hosted CI remain separate |
| Weave | Configured Linux PTY failure/repair, independently verified signed handshake, sixteen-byte IFAC rejection/authentication, old/new child policy and isolated replies, same-root reconnect, actual remote-display wire commands and changed-identity/key/MTU restart; eight native policy/interrupted-owner/pressure regressions | Locally qualified for these software traces; physical devices/RF/public-network and whole-item integration/hosted CI remain separate |
| LoRa / RNode serial, TCP and existing bearer paths | Authenticated stream/fake bearer and startup-monitor failures | Configured validation failure, reconnect/restart/status |
| RNodeMulti | Authenticated vport, parent removes children on shutdown | Active-vport rotation and failed/restarted configuration |
| RNode BLE | Fake-backend native ingress/egress and startup/runtime recovery | Daemon status composition and active-child rotation |
| Generic BLE GATT | Production codec calls and fake startup lifecycle | Available backend packet-loop failure/reconnect composition |
| VR-N76 KISS BLE | Codec calls and configuration/lifecycle primitives | Software-backend authentication, failure and restart |
| Local Unix shared instance | Configured synthetic/explicit first ingress, native ingress/egress, old/new child rotation, attachment mode and joined shutdown | Live non-IFAC policy and broader restart/error composition |
| Local TCP shared instance | Native/explicit first ingress/egress, old/new child rotation, configured synthetic sidecar and attachment mode, gated cancellation/join | Live non-IFAC policy and broader restart/error composition |

`reticulum_ble` currently reports `native_backend_pending`; retain truthful
degraded configuration/status tests. Its missing native dual-role implementation
belongs to #650, outside items 1–6. Do not convert an absent carrier into positive
IFAC evidence. Physical equipment, RF and public networks remain outside this PR.
Only TCP client/server, UDP and Pipe currently support live named management;
other rows use configured restart/native reconnect rather than speculative APIs.

### TCP family constructor cutover board

Checkpoint `3b3b6723` qualifies trusted general restoration under BOOTSTRAP.md.
The first family exploration found a connected constructor gap before running
the finite TCP listener/client lifecycle cases: public/named TCP replacement
uses a bare client, ignores immutable client options for replacement, and loses
initial/reconnect tunnel synthesis. Startup already owns these defaults/hooks.
The separate public listener IPv6 omission stays in the listener case.

| Task | Owner / input / files | Output and evidence |
| --- | --- | --- |
| TCP client constructor alignment | Main agent; existing client builder, InterfaceRecord projection, mutation/named/spawn paths and their tests | Three native controls first: refused endpoint with zero retries, option-only native replacement, malformed option rejection before queueing. Reuse schema normalization/validation before retirement and the same canonical projection for construction/change detection |
| Reconnect hook preservation | Same worker and retained ConfiguredStartupRuntime; a borrowed native input value may group manager/transport/sender references | Clone sender only into the adapter. Observe authenticated initial tunnel output for a new root and later reconnect synthesis; preserve unrelated roots and join shutdown. No new task, registry, constructor defaults or retained Args |
| Finite TCP family scenarios | Existing configured startup and native/RPC fixtures after the constructor cutover review | TCP named IFAC rollback/reload/detach/reattach; configured TCP and Backbone client refusal/reconnect/restart variants; configured Backbone listener child rotation/restart. Preserve named-kind restrictions |

Both source PRE reviews approve the narrow borrowed input context, with schema,
initial synthesis and shutdown controls required. Implementation stays sequential
with the main agent; read-only reviews gate each bounded checkpoint. Packet
barriers stay two seconds, a five-second reconnect is bounded at eight seconds,
and management retains its three-second readiness/twenty-second reply bounds.
No retries-until-green or physical/RF/public-network expansion qualify these rows.

The constructor controls change each of the five immutable options alone and
require a fresh connection plus the old connection's EOF. The configured named
trace declares five connections: initial, failed-listener rollback, rollback
reconnect, successful client replacement and replacement reconnect. Each must
deliver a fresh signed tunnel request and exact authenticated data. The failed
replacement uses an occupied test-owned listener to reach actual bind failure;
rollback must retain the old client options, reconnect hook and unrelated traffic.

The three native controls retain pre-fix failures; the strengthened final run
passes 23 TCP-related cases. Construction/change detection now share schema
normalization/validation and the existing startup builder; replacement/rollback
adapters retain initial and reconnect tunnel hooks. The five-connection trace
passes with signed IFAC output, old/new MTU, unrelated traffic and joined shutdown.
Both independent POST reviews approve; 31 library/646 binary daemon tests and
focused gates pass. These qualify the two constructor tasks, with finite TCP
family scenarios still pending.

### Finite TCP and Backbone family cases

Checkpoint `fcc1b32c04a52550eecda86ebfb19afa659eb3b8` closes constructor
alignment. Main-agent implementation remains sequential. The next bounded task
uses production configured startup, native IFAC/HDLC wire and the existing
mutation/named-management owner; no new public management kind is introduced.
Files are the existing hot-apply listener constructor, bootstrap policy test
registration, one focused TCP-family test module and these status/task documents.

| Declared case | Required trace |
| --- | --- |
| TCP named listener | Strict configured startup; old/new accepted children and live parent key rotation; occupied-listener named reload with attempted-key rejection and previous-key rollback; successful changed-key reload; detach/attach. Old children must reach EOF/removal at each retirement. Prefer-IPv6 and MTU survive public reconstruction. Exact unrelated TCP traffic/address persists throughout |
| Configured TCP client | Bound, unlistened test-owned socket produces real failure/status; turn the same socket into a listener; authenticate first connection; live key change and EOF/reconnect retain policy and root; full configured restart changes key. Preserve unrelated traffic during the root's failure/live transitions |
| Configured Backbone client | Same declared client trace through Backbone configuration, retaining its liveness/tuning. No public named-management extension; restart uses configured startup |
| Configured Backbone listener | Strict occupied bind failure and joined cleanup; authenticated existing/new children across live parent key rotation; whole configured stop/restart with changed key, child EOF/removal and retained liveness/MTU. No public named-management extension |

Each authentication phase sends wrong-key and plaintext frames before exact
authenticated ingress/egress. Negative frames must increment native violations
and cannot reach ingress; the matching packet then succeeds. All errors/status
must exclude test credentials. Two-second packet/readiness/EOF barriers and
eight-second native reconnect bounds remain fixed; two-second publication may
use a three-second observation bound. Global configured restart deliberately
retires/recreates all configured roots, while unrelated-root continuity is
asserted across single-root transitions. No retry-until-green qualifies a case.
The public listener preference omission must first produce a focused native red,
then use its existing builder option. Other behavior changes require a reproduced
failure before a minimal existing-owner fix. Family rows remain pending until
the complete declared traces, focused gates and independent POST reviews pass.

PRE reviews require the unchanged ten-second cleanup join bound, including an
already sleeping failed listener worker. The preference red uses `localhost`
after independently requiring both loopback resolver candidates and a successful
IPv6 loopback bind. Connections must actually reach IPv6 after public rollback,
reload and reattach; status alone cannot establish selection. Missing environment
prerequisites are explicit failures/unqualified evidence. Public/named
`tcp_server` retains its existing rejection of `i2p_tunneled=true` before
retirement; these traces do not extend that management capability or claim every
listener constructor option is publicly supported.

The IPv6 control also reaches the existing `localhost`-to-IPv4 rewrite in public
bind preparation. Preserve native resolver preference/fallback and existing bind
collision rejection when correcting it; do not hard-code IPv6 as a new required
address family. Public Apply commits the listener's live key rotation before
named rollback snapshots its record. Native-only rotation is appropriate for the
configured Backbone case and cannot stand in for that managed-record commit.

The first cohort records fixture failures before product qualification:
`localhost` is IPv4-only in this machine's hosts file, though `::1` binds and
`localhost.localdomain` resolves to both loopbacks. The fixed resolver fixture
chooses the first dual-loopback result from `[localhost, localhost.localdomain]`,
still requiring native IPv6 connectivity. Separate production bind-preparation
and both-order alias-collision controls cover preferred `localhost` without
changing system configuration. Exact dual-`localhost` wire evidence remains
environment-specific; a hosted dual-`localhost` environment uses it directly.
Status assertions observe the existing publication tick; fresh-manager restarts
may reuse native address hashes and instead require prior tree retirement, a new
transport identity and fresh authenticated connections. Configured listener
strictness uses its existing panic policy, avoiding the unrelated client's
separate CLI preflight probe. These fixture corrections do not change product
timing or turn a failed prerequisite into a pass. The native named trace also
requires rejected tunneled-listener reload to preserve its active root/traffic.
After rollback, the named trace also occupies the IPv6 candidate at a new port
and requires native IPv4 fallback, authenticated traffic and old-child retirement
before returning to an available IPv6 endpoint. Resolver selection requires all
returned candidates to be loopback addresses. Both PRE reviewers approve this
bounded refinement and the cohesive listener helper extraction if size requires it.
Correctness POST adds both-order expanded/bracketed IPv6 loopback and wildcard
controls: textual `::1` alone is not a numeric bind identity. Canonical numeric
endpoint keys and the existing conservative wildcard-port guard must cover those
aliases before queueing. This is the same bind-admission owner, without DNS I/O.

### Finite UDP family cases

Checkpoint `524aa7051b5c88ffd430043b6a1fc03b6eb0fa27` completes raw IFAC
admission preparation. The next task composes the two UDP rows through existing
configured startup, public Apply/named management and native Transport routing.
Main agent owns sequential implementation. Inputs are the current UDP worker,
multicast peer-routing owner and configured-root mutation fixtures. Allowed files
are focused daemon tests, their registration, existing UDP/mutation/routing owners
only if a regression demonstrates a defect, and task/status documents. Other
carriers, new public management kinds, default changes, physical/RF/public-network
traffic and item 5 work are forbidden. Both read-only PRE/POST reviews gate this
checkpoint; it remains part of the single eventual item-4 PR.

| Declared case | Required trace |
| --- | --- |
| Named unicast UDP | Configured startup and exact authenticated ingress/egress; public Apply commits live key rotation without replacing the root; an occupied named replacement reaches actual bind failure and restores the last committed key, rejecting attempted-key/plaintext input; successful changed-key reload, detach and reattach retire old roots. Unrelated TCP address and exact traffic persist, including while failed-worker cleanup is pending |
| Multicast UDP peers | Configured native multicast role with an actual loopback group datagram; real signed announces create two independent native virtual peers; authenticated ingress and direct replies use their exact peer/root addresses; wrong-key/plaintext cannot create a peer or enter ingress. Live parent key rotation reaches existing and newly learned peers. Changed-key named replacement and a full configured restart remove old children/peer-route ownership and permit fresh authenticated peer learning/replies |

Use test-owned IPv4 loopback sockets and a multicast output interface explicitly
bound to loopback. No network configuration changes. Existing two-second packet
barriers, three-second published readiness, twenty-second management replies and
ten-second owned cleanup remain fixed. A real bind-failed UDP worker currently
sleeps five seconds before retrying; observe its pending retirement and unrelated
traffic rather than relaxing cleanup or replacing it with a mock. Configured
restart intentionally recreates every root, so require old-tree retirement and
fresh native transport identity rather than unrelated-root continuity there.
Each authentication phase counts both wrong-key and plaintext rejection before
exact matching-key ingress/egress; status/errors must redact all test credentials.
Reuse existing test fixtures/helpers where their contract fits. Start with the
declared native traces; production changes require a retained failing regression
and the smallest fix in the current owner. No retry-until-green qualifies a row.

Both PRE reviewers approve. Exact tests are
`udp_named_ifac_rotation_failed_reload_rollback_detach_and_reattach` and
`udp_multicast_ifac_peer_rotation_replacement_and_configured_restart`.
Multicast evidence is native group **egress**: an independent socket joins the
group on loopback and receives exact IFAC bytes from the native root's address.
Signed-announcement ingress is unicast, with two peer source ports on loopback;
do not claim native group ingress. Fresh signed announcements after retirement
must recreate parent/path bindings without resetting duplicate state, and old
child hashes must no longer dispatch. The failed-replacement test retains the
existing test-only UDP status handle, observes actual bind_failed and subsequent
root removal, and proves management is pending while unrelated traffic succeeds.
Only that existing test-only refresh field's visibility may broaden for this
native observation; no production status owner or test fixture framework is added.
Standard UdpSocket group membership/loopback semantics and Linux IP_MULTICAST_IF
support this existing loopback fixture shape:
[Rust UdpSocket](https://doc.rust-lang.org/std/net/struct.UdpSocket.html),
[Linux ip(7)](https://man7.org/linux/man-pages/man7/ip.7.html).

The initial native trace exposes fixture assumptions: a joined bind-failed worker
retains its last failure snapshot, while its root/dispatch authority is removed;
expect that failure snapshot rather than invent a closed-state requirement. The
first signed peer learns successfully, but sending the second within 30 ms hits
the unchanged 3.5 Hz new-interface ingress guard (360-second hold). The fixture
therefore schedules signed announce input at most once per second. Packet/path
observation bounds and production defaults remain unchanged; no cache reset,
ingress bypass or forced dispatch is allowed. Independent peer observations
also reject the other peer's exact direct-reply bytes during a full two-second
observation after each intended reply.

The paced run then reaches an overstrong path-cache assertion. Native stop
removes peer-route ownership and dispatch authority, while destination PathTable
entries may remain until maintenance. Target RNS 1.5.5 Transport.py:548-551 also
removes interface registration separately from path culling at :957-979. Do not
add eager cache erasure to satisfy this fixture. Reuse both destination identities
across replacement/restart and generate fresh signed announcements; require each
same destination's path to bind to its new native child/current parent. Negative
announces must preserve the prior path value and cannot create a new peer.

Both declared traces now pass (2/2, 16.24 seconds), with no production behavior
change. Source correctness and maintainability POST reviews approve. Full daemon
validation passes 31 library and 662 binary tests; all-target/all-feature daemon
Clippy, default check, architecture, diagnostics scanner, formatting, boundaries
and module size pass. PROGRESS.md retains all fixture failures and exact logs.
The two UDP inventory rows are locally qualified for these declared traces.
Other family rows, whole-item integration, one PR, exact-head
hosted CI and merge remain open.

### Finite Pipe family cases

Checkpoint `1a839601` qualifies the two declared UDP rows. Main agent's next
sequential task composes Pipe through configured startup, public mutation and its
existing process supervisor. Inputs are PipeInterface, daemon pipe builders,
native process cleanup and the current bootstrap/named-management fixtures.
Allowed files are one focused daemon test module/registration, existing Pipe or
mutation owners only for a reproduced defect, and task/status documents. Other
carriers, new public APIs, new process supervisors/dependencies, process-wide
signals and unrelated processes are forbidden. Both read-only PRE/POST reviews
gate this checkpoint; whole item 4 still produces one PR.

| Declared case | Required trace |
| --- | --- |
| `configured_pipe_spawn_failure_repair_preserves_ifac_and_process_ownership` | Configured missing executable reaches real native spawn failure/respawn status while IFAC remains installed; create the test-owned executable and observe automatic recovery on the same root. Exact authenticated first ingress/egress rejects wrong-key/plaintext input. Closing only that child's test control connection causes real exit, reap and respawn; the fresh child retains policy and constructor tuning. Unrelated TCP address/traffic and joined shutdown persist |
| `pipe_named_ifac_rotation_respawn_failed_reload_rollback_detach_and_reattach` | Configured real child first traffic, public Apply key rotation on the same root/process, actual exit/respawn under the committed key; malformed command rejection before retirement; a missing-executable named reload fails and rolls back to the committed command/key. Successful changed-key reload and detach/attach retire old roots/children; final owned shutdown reaps the final child. Preserve unrelated native traffic and explicit MTU/respawn tuning |

Use a Linux-only test-owned executable that relays bytes between its actual
stdin/stdout and a loopback TCP observation socket. A four-byte PID greeting is
sent only over the observation socket before raw HDLC/IFAC traffic. The helper
contains no crypto/policy and spawns no descendant: closing its observation socket
exits that exact child. Linux /proc absence plus old socket EOF after owned
retirement/respawn proves child reaping; no external signal or process kill probe.
This is Linux software process evidence, with no Windows/physical claim.
Python select supports pipes on Unix; this existing native-process fixture shape
is chosen over an echo-only child because both exact wire directions and process
generations must be independently observed. Reuse existing bootstrap/unrelated
TCP helpers; do not introduce a shared fixture framework.

Keep two-second packet/EOF/PID barriers, three-second native status/child-start
observation, twenty-second management reply and ten-second owned cleanup bounds.
The fixture explicitly configures a 0.25-second respawn delay rather than changing
production defaults. Peer IFAC uses the carrier's existing eight-byte default;
credentials stay out of process arguments/status/errors. Every authenticated
phase first counts and rejects wrong-key/plaintext frames. Production changes
require a retained failing regression and the smallest correction in the current
owner. Primary process semantics were checked in the pinned local Tokio 1.53.2
Child kill/wait implementation and
[Python select documentation](https://docs.python.org/3/library/select.html).

Both PRE reviewers approve. The missing owned path is argv[0]; repair writes and
sets executable permissions on a staging file before an atomic rename. Read the
PID greeting with read_exact and verify /proc PPid equals the test process and
cmdline contains the owned executable before observing later disappearance.
Use valid TOML with unbalanced shlex quoting for command-preparation rejection,
and require the old PID/root/socket plus exact traffic to survive. Native manager
MTU assertions cover initial/rollback 512 and replacement/reattach 1024. All raw
relay writes complete and stdout contains no fixture diagnostics.

Both declared cases pass on their first native run (2/2, 4.03 seconds), without
production changes. Source correctness and maintainability POST reviews approve.
The full daemon suite passes 31 library/664 binary tests, and all-target/all-feature
Clippy, default check, architecture, diagnostics, formatting, boundaries and module
size pass. PROGRESS.md records exact logs. The Pipe row is locally qualified
for its declared traces; other family rows and whole-item
integration, one PR, exact-head hosted CI and merge remain open.

### Finite Serial and KISS family cases

Checkpoint `61cf99e0` qualifies Pipe. The next sequential main-agent task covers
three existing inventory rows with four native software cases. Inputs are the
current Serial/KISS/KISS-TCP workers, configured builders, IFAC preparation and
the existing KISS fake-PTY fixture shape. Allowed files are a focused Linux daemon
test module/registration, current serial/KISS/mutation owners only for reproduced
defects, and task/status documents. Other families, new dependencies, Rust unsafe,
new public management kinds, protocol defaults and physical/radio devices are
forbidden. Both read-only PRE/POST roles gate this checkpoint; no extra PR.

| Declared case | Required trace |
| --- | --- |
| `configured_serial_ifac_open_failure_rotation_reopen_and_restart` | Missing owned device alias produces actual open failure/status while prepared IFAC remains installed. Publish a test-owned raw PTY slave through that alias; automatic reopen on the same root authenticates exact HDLC ingress/egress. Native live key rotation persists through master closure/open failure and a new PTY behind the same alias. Full configured changed-key restart authenticates fresh traffic and retires the old tree |
| `configured_kiss_ifac_open_failure_rotation_reopen_and_restart` | Same trace through configured KISS serial, with exact initial KISS command frames on initial open/reopen/restart and authenticated data framing |
| `configured_ax25_kiss_ifac_open_failure_rotation_reopen_and_restart` | Same trace through AX.25 KISS, preserving the pinned APZRNS/N0CALL-0 sixteen-byte UI header around IFAC data, escaping and native status |
| `configured_kiss_tcp_ifac_refusal_rotation_reconnect_and_restart` | A bound, unlistened owned TCP socket produces actual refusal/status; listening on that same socket permits native KISS connection, initial command frames and authenticated traffic. Native key rotation persists through EOF/reconnect on the same root. Full changed-key configured restart authenticates fresh traffic and retires the old tree |

All authentication phases count/reject wrong-key/plaintext frames before matching
ingress and exact egress. Native manager/status verify MTU 512, then restart MTU
1024, current IFAC and sanitized diagnostics. Preserve unrelated TCP address and
exact traffic during single-root failure/rotation/reopen; configured restart
deliberately recreates all roots and requires old-tree retirement/fresh Transport
identity. Native Serial/KISS eight-byte tag defaults remain authoritative. Keep
two-second packet/EOF barriers, three-second status/open/reconnect observation and
ten-second owned native cleanup. Fixture backoff is explicitly 50/100 ms without
changing production defaults; no retry-until-green or ingress/cache bypass.

The Linux PTY observer is a test-owned Python process using openpty/setraw. It
reports its slave path once, closes its extra slave descriptor and relays raw
bytes between master and its stdin/stdout. Reuse the existing fake-PTY EIO handling
while no native slave is open; it performs no framing, crypto or policy. The Rust
test retains the child handle with kill-on-drop and explicitly closes stdin,
observes stdout EOF and awaits successful child exit. Atomically replace only a
test-owned symlink for activation/reopen. A new PTY path may reuse its predecessor's
name; native failure, helper lifetime and fresh wire traffic establish the new
carrier generation. No process-wide signals, system TTY settings or hardware.
Reuse bootstrap/unrelated TCP helpers and a local reader/writer wire helper,
without introducing a shared fixture framework or process supervisor.

The pinned target's Serial/KISS/AX25KISS DEFAULT_IFAC_SIZE and AX25 destination
constructor were checked in their specific interface files. PTY behavior follows
[Python pty](https://docs.python.org/3/library/pty.html),
[Python tty](https://docs.python.org/3/library/tty.html) and
[Linux pty(7)](https://man7.org/linux/man-pages/man7/pty.7.html).

Maintainer PRE approves with explicit new open_errors increments after master
closure before replacement. Greeting reads must preserve following bytes even if
a reused PTY path permits early native output. Helper stdin/EOF stays responsive
during EIO, relay writes complete, fixed KISS initialization frames and the
APZRNS/N0CALL-0 header are independently asserted, and negative AX.25 frames keep
valid outer framing so they exercise IFAC rejection. Native policy/MTU and helper
EOF/child-wait observations retain their declared bounds.

Correctness PRE also approves. Keep CLI strict startup disabled so real workers
see missing devices/refused connections. One buffered stdout reader spans the
path greeting and raw wire; never discard prefetched bytes. Remove the owned alias
while the authenticated native descriptor is still open, then close its PTY
master and observe the new open failure before creating a replacement PTY. This
prevents retries from opening another concurrent test's reused slave name; publish
the new ready target atomically. AX.25 wire checks do not qualify address validation.

The initial build retains a fixture tuple type error, corrected by accepting the
existing reader/writer capabilities by value (including borrowed PTY handles).
The first native run passes four cases in 6.07 seconds. Alias removal is then
tightened to precede master closure, preventing retries from opening another
parallel test's reused slave name; the final run passes four cases in 6.08 seconds.
No production change or pre-fix runtime defect is claimed. Both source POST roles
approve. Full daemon validation passes 31 library/668 binary tests, and focused
Clippy/default/architecture/diagnostics/format/boundary/size gates pass.
The three Serial/KISS inventory rows are locally qualified for these declared
Linux software traces. At this Serial/KISS checkpoint, eight of twenty rows were
qualified and twelve families remained. The current inventory includes I2P below;
whole-item integration, one PR, exact-head hosted CI and merge remain
open. Physical behavior and AX.25 address validation remain unqualified.

### I2P software family composition board

Checkpoint `c94a90fe` records the reviewed Serial/KISS traces. The two I2P
inventory rows remain open. Inputs are configured bootstrap/restoration, the
existing fake-SAM codec tests, and the pinned I2PInterface's sixteen-byte IFAC
default. SAM is simulated only on owned loopback sockets; no router or public
network is used. Main-agent implementation follows independent read-only PRE.

| Task | Owner and allowed files | Declared target-perspective evidence |
| --- | --- | --- |
| Configured outgoing lifecycle | Existing bootstrap, native I2P peer/manager policy owners; new daemon tests and registration | One configured trace: reserved unlistened SAM endpoint gives real error/status, same-socket activation, exact default IFAC ingress/egress and wrong-key/plaintext rejection, live parent/child rotation, EOF/reconnect retaining the child, full changed-key restart, native MTU and unrelated TCP traffic |
| Configured incoming lifecycle | Same owners, existing accept loop and accepted-stream owner | One configured trace: missing SAM/destination preparation error then a malformed DEST REPLY containing an owned private destination, value-free diagnostics and recovery, persisted destination and sanitized daemon status, two accepted children spanning live parent rotation, counted rejection/exact authenticated traffic on old/new children, session error/recreation without losing active children, full changed-key restart and joined cleanup |
| Root shutdown under stalled SAM | Same configured startup and native I2P lifetime owner | Four separate traces: incoming destination generation, incoming SESSION CREATE, incoming STREAM ACCEPT waiting for a destination, and outgoing HELLO with a saturated direct-send queue. A read command barrier identifies each stall; daemon shutdown must join within the existing ten-second bound, remove the tree and close every observed SAM socket within two seconds, preserving unrelated traffic until final shutdown |
| Minimal ownership correction if red | `rns-transport/src/iface/i2p.rs` and private `i2p_parts` modules; focused native tests | First retain every declared result. If ownership structure obstructs the fix, extract root/accept orchestration without behavior changes, verify existing focused tests, then give the root and accept loop owned task sets, cancellation-aware SAM setup/direct enqueue, and explicit completion joins. Preserve public wrappers, session/peer behavior, defaults and diagnostics |

Each case runs once before any fix and once after necessary corrections; retain
fixture/compiler failures separately from runtime defects. Packet/negative/EOF
checks remain two seconds, setup/status/peer checks three seconds and configured
owned shutdown ten seconds. Fixture reconnect wait is explicitly 50 ms; production
timing/authentication and native queues remain unchanged. No new dependency,
public management kind, policy registry, unsafe code or broad refactor.

Research: the [SAM v3 specification](https://i2p.net/en/docs/api/samv3/) defines
separate session/control and virtual-stream sockets and ACCEPT's destination line.
[Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown) separates
cancellation from waiting for completion; the pinned
[JoinSet API](https://docs.rs/tokio/1.53.2/tokio/task/struct.JoinSet.html) provides
owned tasks and explicit joins. Prefer local task sets over a new shared worker
registry. If the existing structure already meets the traces, change only tests.
No nested task may be detached as a substitute for the daemon's joined teardown.
The four stall fixtures begin with an unlistened reserved endpoint and await
configured startup before serving the native handshake. This distinguishes the
native destination-generation stall from bootstrap's separate two-second probe.
Any error-redaction correction must preserve expired-session classification and
recreation while omitting returned private values from status and logs.

After source POST, run affected I2P library/daemon checks, all-feature daemon and
transport Clippy, default check, diagnostics, formatting, boundaries, module size
and architecture. Update this inventory, roadmap and matrix only for proven rows;
whole-item integration/release/hosted CI and the other families remain open.

Both PRE roles approve this board. Existing I2P tests pass 21/21 before changes.
The first fixture build retains a borrowed-policy replacement type error; the
replacement uses an owned clone as the existing manager contract requires.
An initial overqualified test filter selects zero cases and is not evidence.
The corrected six-case run in `/tmp/lxmf-item4-i2p-family-native-red.log` has two
passes and four failures: outgoing lifecycle/queue-pressure shutdown pass; three
incoming native SAM stages retain sockets after joined root shutdown, and a
malformed DEST REPLY exposes its owned private value in published diagnostics.
The mechanical root/accept extraction preserves behavior and public wrappers;
21 existing I2P tests still pass in `...-refactor.log`. No I2P row is qualified
by this structural step; the behavior correction and full trace remain pending.

POST review of the six-case green finds a connected shared-stream gap before
qualification: `tcp_client::run_hdlc_stream_with_runtime_and_ifac` awaits RX queue
admission and packet/keepalive write/flush outside cancellation. New mandatory
I2P joins can therefore wait until the outer daemon abort if established streams
are backpressured. Extend this board by five focused shared-owner regressions:
`hdlc_shutdown_joins_full_receive_handoff`,
`hdlc_shutdown_joins_blocked_packet_write`,
`hdlc_shutdown_joins_blocked_packet_flush`,
`hdlc_shutdown_joins_blocked_keepalive_write`, and
`hdlc_shutdown_joins_blocked_keepalive_flush`. Use the actual full native RX
channel and bounded Tokio duplex writer; an owned AsyncWrite observer supplies
write-progress/flush-entry barriers, with an explicit pending flush only in the
flush cases. Keep the queue/peer blocked through cancellation, then require
joined completion and peer EOF within two seconds. Cover global cancellation,
interface stop and RX EOF's internal stream stop without changing timers.

Allowed additional files are the existing shared HDLC function in
`iface/tcp_client.rs`, its private `tcp_client_parts/hdlc_stream.rs` extraction
if required to stay within module policy, and focused tests under that module's
`tests/` directory. Preserve every forwarding API/caller, error context, framing,
IFAC order, queue and watchdog behavior. First retain five pre-fix results and
verify a mechanical extraction with the existing shared-stream regressions;
then cancellation-select only blocked handoffs. No alternate stream owner,
registry, dependency or abort-as-success. This is a POST correctness correction
within item 4, not a new family or an extra PR.
Both PRE roles approve this correction. The shared function is mechanically
extracted into a 349-line private module with the existing crate-visible path
re-exported. Both structural POST reviews approve unchanged text/behavior;
40 existing TCP-client tests pass in `/tmp/lxmf-item4-i2p-hdlc-refactor.log`.
Pinned Tokio source documents that cancelling `write_all` can leave a partial
write. Cancellation here retires that stream generation: never resume the tail
or emit successful write/keepalive events. Queue/peer barriers stay held until
joined completion; the internal-stop case half-closes only the peer's input.

The first shared-stream run has four output failures and a false RX pass:
its empty default packet is rejected before admission. Keep that result in
`/tmp/lxmf-item4-i2p-hdlc-red.log`, without claiming RX coverage. A nonempty
roundtrip-validated packet then makes all five cases fail at their joined
completion bound in `...-valid-red.log`. Cancellation-aware handoffs make all
five pass in `...-green.log`, with queues/peers still held until join and EOF,
and no cancelled pending-frame success event. Both final source POST roles
approve the common owner correction and the I2P task hierarchy.

The six configured I2P scenarios pass in 8.35 seconds before the shared correction
and remain part of the full daemon suite. Wrong-key/plaintext rejection precedes
matching ingress/egress at every phase. Existing/new accepted children retain
parent policy and peer-isolated replies; both survive SAM session recreation.
Recovery checks the actual private key file; initial `private_key_persisted` and
`destination_prepare_error` are bootstrap snapshots, not refreshed claims.
Changed-identity restart verifies fresh key path/preparation metadata. Native
shutdown joins the root, peer/accept loops, accepted streams and internal HDLC
tasks before final route cleanup; stalled native SAM sockets close at all three
incoming stages. The outgoing pressure trace was already a passing control.

Default transport tests pass 1,023 with 12 existing ignores; default daemon tests
pass 31 library/673 binary cases. All-feature transport tests pass 1,052 with
the same 12 ignores; all-feature daemon tests pass 31 library/674 binary cases
in 33.98 seconds for the binary suite. All-target/all-feature transport/daemon Clippy, default
checks, diagnostics, formatting, boundaries, module size and architecture pass.
The first broad run retained one old raw-SAM-error expectation and one fixture
counter lint; the test now requires safe HELLO context and absence of an owned
private marker, and the counter uses its range index. An unformatted shortened
error block is formatted. These failures are preserved; neither checks nor
wire/queue/timing requirements are relaxed. Final logs use
`/tmp/lxmf-item4-i2p-family-*-final.log` and `*-all-features.log`.

The two I2P inventory rows are locally qualified for the declared fake-SAM
software traces. At that checkpoint, ten other families remained open; the
current inventory includes the subsequent Auto qualification. Whole-item
integration/release checks, one item-4 PR, exact-head hosted CI and merge remain open. No prepared router,
public network, hardware or full-issue qualification is inferred.

### AutoInterface software family composition board

Checkpoint `60f756a7` records the reviewed I2P/shared-stream correction; ten
software family rows remained open there. Main-agent implementation follows read-only
PRE, using the existing configured dispatcher, Auto plan-builder test hook,
native activation/transport bridge, unique Auto shutdown and mutation owner.
The native OS candidate input is replaced by owned Linux loopback bindings;
this is not host-interface discovery qualification. Filter reconciliation to
`lo` so it cannot adopt physical host interfaces. Reuse the existing exact-case
subprocess isolation for socket lifetime assertions.
Keep the existing fixture's candidate/adopted-device lists empty and supply
only manual loopback listener bindings. Native enumeration excludes loopback;
a synthetic adopted `lo` entry would be removed by immediate reconciliation.
Do not add a production retention exception or claim OS adoption evidence.
The two peers use distinct loopback IPs, because peer identity uses source IP
and configured data port rather than the ephemeral source port.

| Task | Main-agent files and owner | Declared target-perspective evidence |
| --- | --- | --- |
| Configured activation failure and repair | New `bootstrap_interface_startup_parts/tests/configured_auto_family.rs` and registration in existing Auto activation tests; existing configured dispatcher/native runtime | `configured_auto_bind_failure_repairs_with_authenticated_restart`: hold the owned data port, require actual native activation failure, safe failed record with no address/root and partial discovery-socket release, preserve unrelated TCP traffic and joined cleanup; release the conflict and configured restart on the same ports must admit exact authenticated traffic with the sixteen-byte default |
| Active virtual-child policy and restart | Same owners, existing InterfaceManager setters/Auto transport bridge | `configured_auto_virtual_children_rotate_policy_and_restart_on_owned_ports`: valid group peering then wrong-key/plaintext rejection and exact IFAC ingress/egress, parent key rotation on an existing child and inheritance by a second child, native mode/outgoing/gravity/pacing inheritance, peer-isolated replies to the configured data port despite an ephemeral source port, sanitized status and unrelated TCP continuity; full changed-key restart retires old children, releases sockets, reuses ports and admits fresh traffic |
| Blocked native receive-owner shutdown | Existing `auto_runtime_parts/autoruntimeplan_sections/spawn_peer_data_receive_loop.rs` and focused library tests/registration | `native_auto_receiver_shutdown_joins_full_transport_queue` and `native_auto_receiver_shutdown_joins_full_event_queue`: actual UDP receive/forwarding under a held full native RX or event queue, then existing shutdown watch must join the receive worker and release its data socket before manager retirement while the queue remains full |

Correctness PRE identifies the blocked receive-owner gap: forwarding and event
send await outside the shutdown watch, while the supervisor waits for producer
event channels to close before retirement. For the two focused regressions,
use the real native receive worker, a standalone InterfaceManager and owned UDP
socket; initialize its discovery state with one valid group token without
pre-registering a transport route. A valid first data frame creates a new child
then reaches the full native RX send. Under current-thread scheduling that
new-child barrier proves the uncontended forwarding path reached its blocked
handoff. For the full event queue, keep RX available and use the native delivered
status counter as the event-send barrier. Keep the relevant receiver/queue full
through watch shutdown, join and socket rebind; retire the manager tree only
after those observations. Transfer the sole transport bridge into the worker
so a test-held route/socket clone cannot disguise release.

Retain both pre-fix results; if red, select shutdown around the existing forward
and event-send futures in the receive owner. Do not add a worker/registry, change
the bridge API or return a fake delivery. The configured family cases establish
the healthy mutation-owner lifecycle; these focused native tests establish the
hidden blocked-handoff boundary. They do not substitute for one another.
Both revised PRE roles approve. The two focused Linux native cases retain
0/2 passing in `/tmp/lxmf-item4-auto-family-receive-red.log` (3.94 seconds).
Forwarding and both event sends now select the existing shutdown watch; true
or watch closure retires the producer, without treating false updates as stop.
The queue remains held through join and exclusive rebind before manager
retirement. Both cases pass in `...-green.log` (1.93 seconds), with the native
1,920 ms gate retained. Exact-case subprocess isolation also protects these
library lifetime assertions from unrelated Pipe fork-before-exec fd copies.
This correction alone does not qualify the Auto inventory row.

Keep the native 1,920 ms initial peering gate, default tag size, dedupe/peer
timeouts and scheduling unchanged. Wait for that declared protocol gate before
one peering input per peer; do not retry until green. Setup/status/child bounds
are three seconds, authenticated/negative/peer-isolation/EOF observations two
seconds, and owned shutdown ten seconds. Preserve every result. If a real
existing-owner defect appears, add the smallest focused regression/fix and
review it; no new Auto management API, dispatcher, registry, dependency or
physical-network activity. Failed/degraded outcomes remain truthful.

Use configured full restart for this family rather than adding unsupported
named management. Single-generation key/policy transitions keep the unrelated
TCP address and exact traffic; full restart deliberately retires every root.
Focused POST, daemon/default/all-feature Clippy, architecture, diagnostics,
formatting, boundaries and module size precede qualification. Update current
inventory/roadmap/matrix only for the proven Linux software row; nine other
families and whole-item integration/PR/hosted CI/merge would still remain.

The Auto software row is now locally qualified. Both configured cases pass
2/2 in 8.00 seconds; native queue regressions retain their two pre-fix failures
and two passing corrections. Both source POST roles approve. The first compile
attempt accessed a private field; the fixture now uses the public announcement
accessor. The first configured execution reaches authenticated traffic/live
policy, then fails an incorrect expected status count. Native status seeds
`adopted_device_count` from manual data listeners, so it reports one `lo`
listener while candidate/adopted plan arrays remain empty. The corrected
assertions preserve that distinction without changing production status.

Default affected suites pass 1,025 transport tests (twelve existing ignores)
and 31 library/675 binary daemon tests. The transport all-feature selection
passes 1,054 tests (twelve existing ignores); the separate daemon all-feature
command passes 31 library/676 binary tests. A combined package all-feature
command instead enables transport AES-128 without core AES-128 and fails
sixteen daemon propagation-decryption cases. This exposes unchanged independent
cipher-feature wiring; no execution of that combined command on the prior
checkpoint was performed. The failed run remains recorded, not reclassified
as passing. Its sixteen cases pass in the standalone daemon command. The
ordinary package gates, Clippy and focused source checks qualify Auto only.
At that checkpoint, nine family rows and whole-item integration/hosted CI
remained open; the current inventory includes subsequent Meshtastic evidence.

### Meshtastic software family composition board

Checkpoint `162db200` qualifies the Auto row; nine family rows remained open
at that checkpoint.
The existing configured Meshtastic path deliberately requires simulation
loopback. Its native bearer handle/tunnel IFAC tests already establish wrong-key
rejection, exact authenticated ingress/egress and live key rotation. Add the
missing configured lifetime/status composition without exposing another handle
or creating a new startup path. No native Meshtastic device or RF is involved.

| Task | Main-agent owner/files | Declared evidence |
| --- | --- | --- |
| Configured failure, status and reactivation | Existing configured dispatcher/prepared spawn and unique mutation owner; new `tests/bootstrap_meshtastic_family.rs` registered by `bootstrap_transport_policy_tests.rs` | One `configured_meshtastic_failure_rotation_and_restart_preserve_simulated_delivery` case: parser rejects the unavailable-bearer configuration; the existing programmatic startup guard reports safe failure/no native root while unrelated TCP passes. A valid simulation configuration admits exact looped multi-chunk packets, publishes explicitly simulated/redacted native status and retains prepared sixteen-byte IFAC/native MTU. Live same-root key rotation preserves actual delivery and unrelated TCP. Joined shutdown retires both native and loopback workers; changed-key/new-identity configured restart admits fresh traffic and resets native counters. No child-inheritance case is invented for a family without virtual children |
| Blocked native worker completion | Existing `meshtastic_parts/interface.rs`; new nested `meshtastic_parts/tests/blocked_shutdown.rs`, registration only in the existing test module | Two native RX/outbound queue cases prefill the actual relevant queue and drive the production worker future to its pending handoff. Keep queues/handles held through existing-token cancellation and actual task join; retain exact sentinels, absence of cancelled admission/output, closed inbound receiver and native stop state |

Before the correction, both blocked paths await outside the worker's cancellation select.
Use a standalone native manager and valid round-tripped tunnel frame, not a
daemon consumer that drains the receive queue. Manual polling of the real
pinned worker future gives a deterministic pending boundary before transferring
it into a joined Tokio task. For outbound pressure, first poll with TX empty to
consume the initial tick, then queue one nonempty packet and poll through native
queueing. Wait the explicitly configured native send-tick deadline, then poll
the sole ready tick against the full output queue. There
are no other input events. Keep the held queue unchanged through cancellation.
Retain both pre-fix outcomes; if red, select the existing stop tokens around
the existing receive-processing future and outbound send. Do not change tunnel
APIs, counters, fragmentation, default pacing, channel capacities or ownership.

Declare one run per scenario before execution. Software fixtures explicitly
configure a five-millisecond send delay and 64-byte chunk limit; production
defaults remain unchanged. Setup/status bounds are three seconds, packet and
negative/control bounds two seconds, and owned shutdown ten seconds. Preserve
every result. The configured trace establishes simulated lifecycle composition;
the existing independent bearer IFAC tests and new blocked-worker cases remain
separate evidence, without claiming a configured external radio peer.

Both PRE roles approve with those barriers. Correctness review corrects the
initial eight-byte assumption: Meshtastic inherits the Interface trait's
sixteen-byte default. Assert that native value with `ifac_size=None`; preserve
the production default. Exercise the independent context/interface stop tokens,
hold exact queue contents through join, and require closed native inbound
ownership afterward. Tunnel processing counters alone are not delivery evidence.

Main-agent implementation follows read-only PRE; correctness/maintainability
POST and focused/native/configured tests precede the affected package gates,
Clippy, diagnostics, architecture, formatting, boundaries and module-size checks.
Qualify only the Meshtastic software row if those pass, then continue the eight
remaining families and whole-item integration/one PR/hosted CI/merge.

The configured baseline passes 1/1 in 6.05 seconds in
`/tmp/lxmf-item4-meshtastic-configured-initial.log`. Both native queue cases
reach their declared pending handoffs and fail shutdown completion before the
correction (0/2 in 2.01 seconds, `...-shutdown-red.log`). Existing context and
interface cancellation now select around receive processing and outbound send;
the worker keeps its original tunnel, queues, status and IFAC owner. Both
pressure cases and the three existing independent native IFAC controls pass
5/5 in 0.02 seconds in `...-native-green.log`. Broader checks and source POST
were still pending at that focused result.

Both Meshtastic source POST roles approve and the default affected suites pass
1,027 transport tests (twelve existing ignores) and 31 library/676 binary daemon
tests. The first all-feature transport run stops with 1,055 passing and one
existing Auto exclusive-bind failure, plus twelve existing ignores. Its output
remains in `...-transport-all-features.log`. No later commands in the stopped
validation sequence ran. This natural failure is untraced; it is consistent
with the previously PID-attributed Auto/Pipe pre-exec descriptor-copy control,
not individually attributed to a child here. On this failure path, discovery
sockets are local values dropped by a failed data bind before worker spawning.

The bounded harness correction hoists the existing exact-case child helper
into Auto's private Unix-gated parent test module. Reuse it for both native
pressure cases and the three existing exclusive-bind lifecycle cases (normal
restart, echo-recovery restart and partial-startup failure/retry). Invoke before
socket creation, keep successful one-test/non-ignored child evidence and every
native assertion/deadline/immediate bind probe. No production behavior, shared
bind setting, sleeps or retries change. Read-only PRE precedes the correction;
declare one subsequent normal full-suite run rather than retrying until green.
Both PRE roles approve. To respect the active size limit, the three contiguous
lifetime cases move unchanged apart from the entry guard into
`auto_runtime_parts/tests/socket_lifecycle.rs`, included in the same private
module so exact case names remain stable. The old core test file drops from
1,448 to 1,105 lines; the focused file is 342 lines. No allowance is added.

The declared subsequent normal all-feature transport run passes 1,056 tests
with twelve existing ignores in `...-transport-all-features-isolated.log`.
A Rustfmt-normalized comparison confirms every original body is preserved
apart from the three entry guards. Separate daemon all-feature validation
passes 31 library/677 binary tests; affected all-target/all-feature Clippy,
default check, diagnostics, architecture, explicit formatting, boundaries and
module-size gates pass. Both Meshtastic source POST roles approve. This locally
qualifies the twelfth software family row only. The configured path is an
explicit loopback simulation, distinct from independent native bearer IFAC
evidence. At that checkpoint, eight families and whole-item integration/one
PR/hosted CI/merge remained open; no external radio or RF qualification was
inferred.

### Weave software family composition board

Checkpoint `b1e34761` qualifies simulated Meshtastic; eight families remained
open there. Read-only exploration maps Weave to its existing configured
dispatcher/prepared root, serial worker, native endpoint registry and
status/control projection. Both immutable reference files are byte-identical
for this surface: native IFAC is sixteen bytes, MTU 1,024 and baud 3,000,000.
Python assigns a new endpoint its parent's outgoing policy. Before this fix,
Rust overrode inherited outgoing with `true` in `ensure_weave_endpoint`.

| Task | Main-agent owner/files | Declared evidence |
| --- | --- | --- |
| Size-bounded mechanical preparation | Existing `iface/weave.rs`; private `weave_parts/{status,stream,frame_processing}.rs` and `weave_parts/tests/core.rs`; existing module-size allowlist. Existing serial PTY helper moves to shared private `tests/pty_peer.rs` with registrations/imports only | Run existing native Weave and four daemon Serial/KISS cases before/after. Preserve function bodies, API/privacy, exact test names and native assertions. Keep every new regular file below 500 lines/test file below 1,200; remove the obsolete root Weave allowance once its root meets the limit. Shared PTY script/ownership/explicit reaping stay unchanged |
| Parent-policy correction | Existing endpoint registration owner in the extracted frame-processing part; one focused native endpoint regression | With parent IFAC, mode/outgoing/gravity/pacing set, existing/new endpoints retain the authoritative manager policy. New-child outgoing=false fails before removing the redundant override; no new policy implementation or default |
| Cooperative native stream retirement | Existing generic stream owner and cleanup epilogue; new focused nested native tests | Five declared pressure cases: full native RX, discovery write/flush, established packet write/flush. Use valid default-sized IFAC and actual endpoint/handshake barriers, real duplex partial-write pressure or a notified pending-flush adapter. Keep queues/peers blocked through existing-token shutdown and joined completion/EOF; pending output does not increment success. Active children must be removed before parent retirement |
| Interrupted endpoint ownership handoffs | Existing manager parent-child truth and stream cleanup epilogue; two native held-lock controls | Cancel native registration after manager creation/before state indexing, and GC after state removal/before manager retirement. Final cleanup must remove every child of this parent while retaining an unrelated parent's child; no second registry or lock held across await |
| Configured failure and native PTY lifecycle | Existing daemon startup/mutation/control owners; new Linux `tests/bootstrap_weave_family.rs` | Strict owned missing-alias preflight fails safely with no root/control. A permissive missing-alias root reports reconnecting/offline while unrelated TCP passes; publishing an owned raw PTY allows signed DISCOVER/CONNECT verification, default-tag wrong-key/plaintext rejection and exact endpoint ingress/addressed egress. Live parent key/non-IFAC policy covers old/new endpoints and isolated replies. Alias removal before peer closure proves old-child cleanup, same-root reconnect and retained rotated policy. Full changed-key/new-identity restart proves fresh wire traffic, counters/control projection and joined cleanup. Observe an actual remote-display command on the owned PTY rather than treating a queued acknowledgement as transmission |

Preparation is mechanical and reviewed/checkpointed before behavior edits.
Keep the existing five-second native reconnect wait; accepted-but-unused backoff
configuration is not used as a fixture shortcut or advertised as newly working.
Opening a serial port sets native online before handshake, so readiness requires
actual frames and `wdcl_connected`/link state. Historical switch/display metadata
alone does not establish readiness. Endpoint creation before IFAC decode matches
the reference; do not invent a no-new-child authentication rule.

For cancellation, keep one existing stream owner and its cleanup epilogue.
Select the existing stop tokens around the inner I/O operation future, including
initial discovery and all later receive/write/flush/lock waits, then always
await endpoint cleanup before returning. Do not cancel away that epilogue or
resume a partially written frame on the retired stream. Preserve EOF/error
diagnostics, public helpers, framing, handshake validation, IFAC ordering,
counters and all protocol timing. An ordinary scoped async block is sufficient;
no extra worker, registry, queue, dependency, dispatcher or public API is needed.
The prior Tokio cancellation research applies to this existing owning future:
pending channel sends commit no message, while a cancelled write may have left
a partial frame and therefore retires its stream generation.

Declare one initial run per native/configured scenario and preserve every result.
Native pressure/cancellation and packet/negative/peer-isolation/EOF bounds are
two seconds; ordinary status/setup three seconds. Missing-device recovery gets
eight seconds to include the unchanged five-second wait. Owned root shutdown
remains ten seconds. Configured restart uses default MTU then an explicit 512
override, retaining sixteen-byte default IFAC. No real device, RF or public
network is used. Main-agent work follows PRE, structural POST and source POST,
then affected gates before qualification. Seven other families and whole-item
integration/one PR/hosted CI/merge would still remain afterward.

Both structural PRE roles approve. Behavior PRE identifies the two ownership
cut-points above: the stream's private index is incomplete while registration
or GC awaits another lock. Cleanup will reconcile the authoritative manager's
children for this uniquely owned Weave parent, then clear stream/status state;
it cannot rely only on the private index. Use held-manager/state barriers and
manual polling of the actual helper futures, cancel/drop at the demonstrated
pending boundary, then run the real final cleanup while the parent remains.
This is eight native behavior scenarios including parent policy, five pressure
cases and two ownership controls, plus the two configured cases. RX processing
counters are barriers only; exact queue contents establish admission or its
absence. The unchanged baselines pass 22 native Weave cases (0.12 seconds)
and four daemon Serial/KISS cases (6.07 seconds), recorded in
`/tmp/lxmf-item4-weave-{native,pty}-baseline.log`.

Mechanical preparation preserves all expanded Weave and shared PTY bodies under
Rustfmt normalization. Post-extraction native tests pass 22/22 in 0.11 seconds;
four Serial/KISS controls pass in 6.07 seconds. The root is now 426 lines,
status 466, stream 239, frame processing 342 and tests/core 1,127; remove the
obsolete root allowance without adding exceptions. Affected Clippy,
architecture, boundaries, module size, explicit formatting and diff checks pass.
That checkpoint changed no behavior or family qualification; the policy,
cancellation/ownership and configured PTY traces remained the next work then.

Subsequent native behavior evidence retains genuine 0/3 policy/interrupted-owner
and 0/5 pressured-operation failures before correction. Removing the outgoing
override, selecting the existing stops around the scoped I/O operation and
reconciling the existing manager tree in mandatory awaited cleanup yields 30/30
native cases in 0.10 seconds. Both native POST roles approve; the maintainer's
existing tree-helper refinement also passes 30/30. PROGRESS.md preserves the
fixture-only compile/parent-token corrections and every native result. This
native-only checkpoint did not yet qualify Weave.

The subsequent two configured PTY cases pass their first run (32.11 seconds).
Review tightens quiet observation to reject any extra byte/EOF within the same
two seconds, and uses two seconds for restart discovery already observed as
emitted; only missing-device recovery gets eight seconds. Both final source POST
roles approve. Final focused default/all-feature configured runs pass 2/2 in
32.10/32.12 seconds. The traces establish exact signed handshake, negative and
positive default-IFAC wire behavior, existing/new child key and non-IFAC policy,
isolated endpoint replies, native failure/same-root recovery, actual display
commands, unrelated traffic, joined observer/root/control cleanup and new
Transport identity/key/MTU restart with fresh state. No real device is used.

Default affected suites pass 1,035 transport and 31/678 daemon tests; the daemon
binary precedes the quiet assertion tightening, covered by the final focused
default run. Separate all-feature suites pass 1,064 transport and 31/679 daemon
tests, with twelve existing transport ignores in both scopes. Clippy, default
check, architecture, diagnostics scanner, boundaries, module size and explicit
formatting/diff gates pass. Detailed command/log scopes are in PROGRESS.md.
Weave now qualifies the thirteenth declared software row. Seven families and
whole-item integration/one PR/hosted CI/merge remain open. Backoff configuration
is unchanged; these traces do not establish physical/RF/public-network behavior.

### LoRa/RNode software family composition board

Weave checkpoint `92627fa3` qualifies thirteen software rows; seven remain.
The next row is ordinary LoRa, the Python RNode serial/TCP profile and the
existing caller-owned generic bearer. Keep RNodeMulti and native RNode BLE as
their later separate rows. Existing prepared Unicast activation, native status,
RNode management and unique configured mutation ownership remain authoritative;
this row has no virtual children. IFAC defaults to eight bytes. Ordinary LoRa
MTU is 220, Python RNode profile MTU 508, and the generic bearer uses its existing
configured MTU. Pinned Python RNode confirms IFAC eight/hardware MTU 508.

| Task | Existing owner / bounded main-agent files | Declared evidence |
| --- | --- | --- |
| Mechanical size preparation | Private same-namespace LoRa includes: extract native spawn and management/status handles from `lora_parts/lorainterface.rs`; move `module_extra.rs` tests to `lora_parts/tests/core.rs`. Extract the existing generic KISS stream to `kiss_parts/stream.rs`, retaining public paths and test names | Baseline/post native LoRa, KISS and generic-bearer suites plus four configured Serial/KISS controls; expanded bodies unchanged. Touched regular files below 500/test files below 1,200, remove only obsolete allowances, no new exceptions. Review/checkpoint before behavior |
| Native opening status | Existing LoRa serial/TCP supervisor and private status fields | Two owned missing/refused endpoints fail the native error-publication regression first, then publish contextual offline errors. Keep IFAC credentials outside diagnostics and clear prior errors through existing successful startup collection |
| Native supervisor retirement | Same LoRa open/connect/retry and stream/probe owner | After real failure has published its barrier, four long-backoff serial/TCP × global/interface-stop cases must fail a two-second join before selecting existing cancellation around connection/retry waits. Two active-stream controls cover the independent stop inputs and joined existing probe monitor. Preserve configured backoff values and native liveness timing |
| Shared KISS pending operations | Existing generic KISS stream/I/O helpers used by LoRa and prior Serial/KISS families; new nested native tests | Nine held-pressure cases: full native RX, init write/flush, packet write/flush, management write/flush and shutdown write/flush. Ready-shutdown and initial-write-failure controls prove exact normal cleanup frames/counters and preserve the no-cleanup error path. Keep exact queues/peers blocked through cancellation, join/EOF, no false successful output/admission. Existing command/data notifications use `try_send` and are not invented blocking handoffs |
| Generic bearer attempt | Existing `rnode_bearer_interface.rs` and status owner; private test module | For both existing bearer kinds, test full receive-queue cancellation and retained terminal offline status; caller-owned fake attempts cover opening/close failure, valid startup/authentication, live key rotation, read failure, exactly-once close and a fresh changed-key attempt. Keep bounded platform reads single-owner; no competing timeout/read or daemon backend invention |
| Configured ordinary LoRa serial | Existing startup/state guard, shared PTY observer, prepared worker/mutation/control owner; new Linux family tests | Owned invalid state fails before native root/control while unrelated TCP passes. Repair only owned state; missing alias reports offline error, PTY publication repairs the same root. Exact probe/radio echoes precede validated status, eight-byte IFAC negatives/traffic, live rotation and observed management bytes. Alias removal/closure/republication proves same-root recovery; joined shutdown and new Transport identity/key/explicit MTU restart retain unrelated traffic |
| Configured Python RNode TCP | Same existing startup/native owner, owned bound unlistened socket | Actual refusal then listen on the same port; invalid startup response closes the first connection, valid replacement recovers the same root. Default MTU 508, IFAC rejection/authentication, live rotation, native EOF/reconnect, exact management output and changed-key/identity/MTU restart qualify this path. Avoid strict-preflight dummy-peer confusion |

Do not alter public/exhaustive option structs or add workers, queues, a backend
registry, protocol timers or dispatcher paths. Existing generic-bearer retry
belongs to its caller. Its `runtime.poll().await` owns any bounded platform
read and must not be wrapped in a shorter cancellation race; select only the
blocked admission/write operations and always close the attempt. Final status
must publish offline while retaining relevant error/traffic metadata.

For shared KISS, evaluate a scoped I/O operation selected against the existing
stop, with final status outside it. Preserve normal ready shutdown frames; an
interrupted pending write may have produced a partial frame, so retire that
stream without appending cleanup bytes. A pending cleanup write/flush must also
observe the same stop. Prefer existing-token cancellation to a new timeout.
The PRE roles must check this cleanup distinction and normal-stop selection
before behavior implementation; the platform-bearer read rule above differs
from ordinary cancellable Tokio stream I/O.

Correctness PRE approves mechanical preparation and requires the initial-frame
write-failure path to keep its existing no-shutdown return. An explicit private
operation result must authorize cleanup only after an ordinary stream-loop exit,
not initial failure or outer interruption. Real manager contexts clone the stop
into their global cancellation field, but the public context can supply
independent tokens: exercise both in native supervisor/active-stream controls.
The existing private stream context may retain interface-stop; cancellation must
cancel its existing child token, then await the same KISS future and already-owned
probe monitor. Do not drop that compound owner or add another task.

Both PRE roles approve with these fences. Baselines pass 18 LoRa, four KISS,
five generic-bearer and four configured Serial/KISS cases. The mechanical split
preserves Rustfmt-normalized reconstructed method/stream/test bodies; the first
comparison helper omitted two separator blank lines, and its diagnostic diff
is retained separately from the corrected successful comparison. Post-extraction
tests pass the same counts. Both structural POST roles independently reconstruct
and approve the unchanged source. Affected Clippy, architecture, boundaries,
module size, explicit nine-file formatting and diff gates pass. No LoRa/RNode
behavior or family promotion occurred in that structural checkpoint (`581e9b52`).

The next bounded native opening/supervisor increment retains 0/2 status reds,
then 20/20 LoRa passes after contextual error publication. Using those failures
as actual retry barriers, four serial/TCP × independent global/interface-stop
cases fail their joins. Active-stream controls pass global stop and fail interface
stop before the correction. Existing connect/retry waits now observe both stops;
the private stream context cancels its existing child and then awaits the same
KISS future/probe monitor. Corrected native tests pass 26/26 in 0.16 seconds.
The first post-change compile failure for two missing private test fields is
retained separately; PROGRESS.md gives exact scopes and corrections.

Both bounded source POST roles and affected Clippy/diagnostics/architecture/
boundary/size/explicit-formatting gates pass. These active controls are
unpressured, use a sixty-second existing test startup guard and observe actual
initial I/O, EOF and release of the monitor's interface owner. They do not prove
independent protocol conformance or a stalled TCP connect syscall. At that
supervisor checkpoint, shared KISS pressure/cleanup, generic bearer and the two
configured lifecycle traces remained open. The inventory stayed thirteen of
twenty; no row promotion or production timing/default change was inferred.

The next shared-KISS checkpoint retains nine actual Pending RX/write/flush
failures and two passing cleanup controls before correction. The existing body
becomes a scoped operation: initial error keeps its no-cleanup return, ordinary
loop exit permits ready-first cleanup against the same stop, and outer
interruption retires possible partial bytes without appending output. Existing
inner stop, helper bodies, public options and timing stay intact. Final native
KISS tests pass 15/15 (0.04 seconds), including the nine pressure cases, normal
ready shutdown, initial failure and four prior controls. Exact held queues,
bytes, EOF, no false output/admission counters, a live parent and closed control
receiver qualify this native owner; they do not imply configured-family proof.

Both source POST roles and affected gates pass. Default transport passes 1,054
tests and transport all-feature 1,083, each with twelve existing ignores; default
daemon passes 31/678, including configured Serial/KISS. Exact commands/results
are in PROGRESS.md. At that shared-KISS checkpoint, generic bearer and configured
LoRa/RNode traces were pending; the later bearer checkpoint follows below.
Family totals stayed thirteen of twenty, with seven families and the
whole-item integration/one PR/hosted CI/merge still open. No physical or
public-network activity is used.

The locked-version [Tokio select documentation](https://docs.rs/tokio/1.53.2/tokio/macro.select.html)
confirms ordered polling with `biased`, assigns fairness to that ordering and
identifies `write_all` as unsafe to abandon and restart. The proposed use retires
an interrupted stream instead of restarting a partial write; existing inner stop
selection must keep an otherwise-ready I/O loop responsive. The normal-cleanup
control must prove that deterministic polling preserves ready shutdown frames.

Declare one first run per scenario and retain every compile/runtime failure.
Native pressure/packet/negative/silence/EOF/join bounds stay two seconds, normal
configured status/setup three, and configured owner shutdown ten. Native LoRa
retry can use its existing validated 50/100 ms test configuration; long-backoff
regressions deliberately retain a value above the owner's join bound. Preserve
the existing 3.5-second detect probe, fourteen-second TCP silence timeout and
probe-monitor startup policy. Generic bearer startup validation gets seven
seconds to include its unchanged five-second monitor deadline; packet, pressure
and joined-stop bounds remain two seconds after that readiness barrier. The
generic set is six cases: full receive queue, terminal offline cancellation and
caller-owned failure/rotation/read-error/changed-key retry for each existing kind.
Configured restarts use native defaults first,
then a valid explicit MTU 200. Only owned local state, PTYs, loopback sockets
and injected software bearers are used; no hardware, Bluetooth scan, RF or
public-network activity. Main-agent PRE/structural POST/source POST and affected
gates precede row qualification. Six other families and whole-item integration,
one PR, hosted CI and merge would remain afterward.

The following generic bearer checkpoint preserves two sequential close attempts
only when failed-open cleanup itself fails, as the existing runtime already
requires; each opened attempt has one terminal close call. First red is 0/6
(four production failures and two fixture assumptions), then the corrected six
cases all fail on full RX cancellation, healthy final online state and lost
cleanup diagnostics. Reusing existing admission cancellation and final status
ownership produces 11/11 passes, including five prior controls. Exact held queue
contents, both stop inputs, authenticated eight-byte IFAC/rotation, read plus
close error retention and fresh caller-created attempts provide software bearer
proof without changing native read ownership or startup timing. Both source
POST roles and focused gates pass; PROGRESS.md retains commands, logs and scopes.
Only the two configured LoRa/RNode serial/TCP traces remain for this family;
current qualification remains thirteen of twenty, with no physical claim.

### Raw IFAC type preparation for UDP composition

Checkpoint `c5ee78f50a3e8f13c5a1f0e3537f6d4702558694` records the reviewed
TCP/Backbone lifecycle traces. UDP exploration finds a common raw-record gap:
`as_str` ignores malformed credential aliases and `as_u64` treats a negative or
malformed bit count as absent. This can acknowledge a malformed replacement or
lose the presence check that forbids size-without-credentials plaintext.
Typed startup already rejects non-string credentials, deserializes a signed
integer size, and preserves negative sizes as present zero/default. It remains
the scalar-type/presence contract; legacy RPC alias names remain additive.

| Task | Main-agent files and owner | Required evidence |
| --- | --- | --- |
| Raw IFAC scalar admission | Existing `ifac_validation.rs` before queueing; `record_settings.rs` conversion/alias ordering; focused tests | All present non-null credential aliases in object settings must be strings, including lower-priority aliases; size must be a signed integer and normalize negative to present zero as typed startup does. Preserve absent/null options, empty-string fallback, byte-significant whitespace and existing opaque settings metadata. Keep the existing static invalid-IFAC error and crypto validation |
| Authority and native preservation | Same alias lists shared by validation/conversion, existing SDK v2/typed ZeroMQ UDP fixture | Finite invalid-type/presence controls fail before fix. A malformed SDK replacement is rejected with value-free diagnostics; prior native root/policy remains and exact authenticated ingress/egress still succeeds. Queue admission cannot retire an unrelated root |
| Later UDP family traces | Existing startup/mutation and multicast peer-route owner | Named unicast management and authenticated multicast virtual-peer rotation/replacement remain subsequent work; this preparation alone promotes neither UDP row |

Implementation stays with the main agent after read-only PRE review. No new
schema, credential precedence, default, worker, registry or public management
kind is introduced. Existing typed schema/normalization defines signed/null
semantics. Inspection of the metadata owner finds explicit compatibility for
non-object settings, preserved under `configured_settings`. Both PRE reviewers
approve retaining that path rather than inventing a global settings schema.
Validate only top-level object IFAC leaves, without recursively interpreting
opaque/nested metadata. The first container-rejection assertion was an incorrect
new fixture assumption; it becomes positive admission/envelope preservation.
Preserve packet and cleanup bounds, then run focused regressions,
diagnostics, affected daemon validation and the usual source/architecture gates.

The preparation is now locally verified. Four genuine contract regressions and
the real SDK malformed-replacement acknowledgement retain pre-fix failures; six
controls and the SDK/ZeroMQ native trace pass after the correction. Shared alias
lists, signed presence and static errors preserve existing compatibility. A
following valid mutation, retained UDP authentication, unrelated TCP traffic and
joined cleanup pass. Both source POST reviews approve, and full daemon validation
passes 31 library/660 binary tests plus focused gates. Neither UDP family row is
promoted by this input-boundary evidence alone.

## Duplicate and persistence contract

The foundation now separates current/previous membership from the existing
180-second proof/source records and retained Link receipts. Native maintenance
rotates membership every 250 ms when current exceeds 500,000; the separate
90-second TTL/Link cleanup remains unchanged. Production save writes current
membership only; disabled/attached saves perform no I/O and restore creates no
proof contexts. Outbound registration keeps recipient-bound proof context before
handoff even in attached mode, while that mode skips local membership. The
existing outbound pre-dispatch timing otherwise remains unchanged; this is not
new evidence of Python-exact failed/no-route outbound remembering.

Ordinary admission now repeats the filter, synchronously enqueues and remembers
under one handler/cache/queue order using current attached mode. Queue rejection
does not insert/promote membership or alter proof context; only path-request
admission needs rollback. Original wire hops are immutable filter input despite
later receive-hop accounting. Link destinations and LRPROOF defer remembering to
current drain state. This shape boundary covers valid native protocol traffic;
synthetic non-Link destination bits addressed to a Link-table ID are not evidence
for that shortcut. An authenticated local Resource-control Data regression proves
wrong-interface traffic leaves the transfer pending and forgets membership, while
identical correct-interface replay reaches its existing terminal outcome.

A frozen-Python differential covers all 21 contexts across 18,144 combinations of
packet/destination type, hops, transport identity, attached mode and current/
previous membership. This establishes the ordered filter predicate, including
invalid producer combinations; stronger production ingress guards and Link routing
are separate evidence. The next routing checkpoint now binds native pending proof
hops at the request generation's first actual dispatch, including unknown 128 and
shared zero/one-hop routes. Pending retries retain that binding and the one-shot
rebalance marker; public restart clears both. A valid exact-shape, matching-mode
proof can authorize one observed-hop rebalance, changing only an existing path's
hop field. The hop header itself is not signed. Equal-hop local selection remembers
before ordinary validation; invalid mismatches remain unremembered. Selection,
remembering and activation hold the same Link generation guard.
Public restart preserves the old displayed ID until its next request exists; a
private readiness fence rejects old dispatch/proof state in that interval. Normal
Pending retries retain readiness and binding, while standalone request/proof
primitives remain route-unbound only after they have actually made a request.

Transit entries capture their selected path's remaining distance. Valid first
proofs on the recorded next interface can rebalance that distance; validated
entries reject later changes. Identityless shared-owner handoff retains its
explicit trust exception: unknown distance binds to the first owner observation,
without inventing a path or overriding a known identity's signature requirement.
Validated transit LRPROOF uses the existing dispatch tail without originating
duplicate remembering, so repeated identical proofs still forward. Data, ordinary
proofs, Resource traffic and KeepAlive now share one early interface/hop gate.
Same-interface routes accept either stored distance only on that recorded
interface; rejecting a third interface is a stronger Rust guard than the frozen
same-interface branch. This is bounded source-grounded/native evidence, not a claim
that all stateful Python behavior is identical.

Actual enqueue/dequeue regressions cover table insertion, expiry and proof
activation before drain, local restart/replacement, wrong-interface Resource Data,
and retained packet proofs across teardown/replacement/TTL expiry. Native and
daemon fixture checks remain separate from the pinned Python Resource round trip.
These qualify the listed routing cells; the remaining family lifecycle, request,
discovery, live child policy and deeper process-recovery gates still apply.

- Preserve the frozen filter's ordered branches: attached-client bypass, foreign
  transport rejection, six context exemptions, Plain/Group rules, generation
  membership and Single-Announce replay. Inventory all 21 contexts separately
  from valid production packet shapes and explicit stronger Rust ingress guards.
- Remember transit Link packets only at the correct routing decision. Transit
  LRPROOF validation/forwarding differs from local pending LRPROOF remembering
  before signature validation. Preserve signed hop rebalancing and same-interface
  direction rules. Wrong-interface local Data forgets both generations so the
  identical frame can arrive correctly later.
  Capture private expected proof hops from the generation's actual request
  dispatch route, not the early timeout snapshot or a later mutable path lookup.
  Authenticate allowed hop rebalancing before changing that state. KeepAlive's
  separate relay path must use the same validated direction/hop decision.
  Unknown local Link proofs must still reach retained receipt-key validation
  after teardown; the local Data interface gate must not become a blanket Proof
  restriction. Forwarded transit LRPROOF needs the existing send owner without
  originating-outbound hash remembering.
- Use the frozen one-million maintenance threshold and strict current-count
  greater-than-half rotation. Add 250 ms membership maintenance without moving
  existing 90-second TTL/Link-table cleanup. Between-job overshoot is not a hard
  capacity violation; no silent per-insert rotation or lost proof context.
- Persist only current membership. Disabled/attached saves do no disk I/O; an
  owner's existing file stays byte-identical. Restore membership without invented
  destination-empty proof contexts; retain legacy reading.
- Required negative controls include route/Link changes across queueing, local
  invalid/replayed proofs versus repeated transit proofs, wrong-interface then
  correct-interface exact bytes, full queue with existing current/previous hashes
  and proof bindings, threshold boundaries and generation displacement/restart.

## Request, discovery and recovery evidence

The common live child-policy slice uses the existing InterfaceManager tree
relationship owner in `set_mode`, `set_outgoing`, `set_gravity` and
`set_announce_pacing`. A registered target and all current descendants change
together under its existing mutable owner; unknown roots cannot mutate even an
orphan record, and a child-address operation preserves ancestors, siblings and
unrelated trees. Accepted, virtual, nested and plaintext-local children retain
their IFAC choices; later-created children retain normal inheritance. No new
registry, public option, lifecycle task or IFAC policy path is introduced.

Existing queue timestamps/deadlines and PathTable entries remain committed.
Subsequent recursive path-request eligibility reads the new mode; direct sends
read outgoing, and queue release reads outgoing/pacing without redoing the mode
admission made when queued. A repeated exact signed equal-hop/equal-emitted
announce proves live child gravity decides path tie-breaking. These native
dispatch/queue/routing checks qualify the common owner, not every family-specific
restart/error cell or Python process-level policy transition.

Single-packet Python RequestReceipt remains SENT even after its PacketReceipt is
proved. Request-Resource completion sets DELIVERED. Timeout or response rejection
must finish either pending state once; a later duplicate/valid response cannot
reopen a rejected request. Exercise actual packet and Resource paths, exact IDs,
closure/cancellation, lost notification and oversized-then-later response cases.

The fifth local slice implements that bounded owner in
[REQUESTS.md](REQUESTS.md): native exact-Arc registration and handoff, retained
terminal outcomes, guarded packet/Resource phases, strict response admission,
scoped cleanup and existing-maintenance deadlines. Wire/native regressions cover
both representations, genuine signed ACKs, early responses, aborted dispatch,
malformed/oversized responses, queue loss, retirement and shutdown. Correlation
commits in ResourceManager only after receiver validation; relabelled duplicates
and rejected advertisements cannot authorize cleanup of unrelated transfers.
rncp/rngit consume retained outcomes and native commit latches for abandonable
optional cancellation/close I/O. Controlled target-reference guards and actual
utility mixed-peer traces remain separately identified in PROGRESS.md; these
checks do not complete the other item-4 cells.

Discovery cases include accepted/rejected/missing/malformed implementation and
version, default/override, persisted legacy IFAC, stale entries, duplicate names
and endpoints, Linux Backbone versus Windows/macOS TCP selection, monitoring
deadlines, bootstrap restoration, detach/reset races, inheritance and unrelated
traffic. Both Python 1.5.5 directions must retain honest implementation names:
Python's default rejection of Rust is a result, not an invitation to impersonate
RNS. Session detach suppression is explicit Rust operator intent; failed explicit
reattach preserves it and queued discovery work rechecks it before commit.

The seventh local slice prepares configuration and pure planning under
[DISCOVERY.md](DISCOVERY.md). All typed startup views use one immutable read;
the private additive override cannot invalidate otherwise valid established
views. Initial planning gains explicit policy while old callers retain verified
defaults. Native endpoint exclusions and collision allocation match the pinned
target's controlled predicates, including arbitrary-size version components
against the fixed minimum. The eighth local slice now connects this policy to
the existing mutation worker, with native initial/live/periodic admission,
monitoring, session suppression and trusted four-kind plus Backbone bootstrap
recipes. Actual packet provenance and status, key collisions, manager contention,
whole-tree stop and record retirement pass bounded native regressions. The
validated-event fixture preserves honest LXMF-rs metadata and proves persistence
before queue admission; outer network authentication is not its scope. Other
bootstrap family recipes remain open. All three declared Python process cases
now pass with honest implementation metadata, exact recipient bytes/signatures,
selected native path/parent and independent sender receipts. Default rejection
retains free capacity and a real qualified positive control for at least ten
seconds. This closes those three discovery process cells, not the family or
deeper relay matrix.

The deeper relay lane will extend the existing Python shared-owner/client and
Rust-relay fixture to three Rust relays. Predeclare three fresh process runs,
four delivery cells (small Direct and Direct Resource in both directions), each
before and after replacement of the intermediate relay: 24 successful message
cells are required. Record exact topology, readiness predicates and protocol-derived
deadlines beside the runnable case before its first run. Keep every run/result;
after a confirmed fix, a new candidate runs the whole declared set. Do not accept
retries-until-green or relax retry/timeout/performance policy.

The runnable lane is `cargo test -p lxmf-cli --test deep_relay_recovery --
--ignored --nocapture --test-threads=1`, with explicit absolute Python interpreter
and clean reference paths. Before its first run, freeze RNS
`99de23c040d507e3fefca19e87b182302902725d` and LXMF
`c3ff2d6dc2f256daab896dadc044dd5a913ecbb7`. Each topology is Python shared owner
→ attached Rust edge A → Rust middle B → Rust edge C → Python TCP client.
All five processes share a 60-second readiness bound, path learning/reconnection
has 30 seconds per phase, and each message has one shared 60-second bound for
native recipient validation and independent sender completion. The baseline's
six-second per-hop Link budget and LXMF's four-second processing interval inform
this loopback qualification bound; it is not a universal worst-case guarantee
for stale Link and retry behavior. No underlying timer is changed.

Each phase sends exact small Direct packet and deterministic 131,101-byte Direct
Resource content in both directions. Native source/destination/signature/method,
sender representation, Link and Resource IDs and final exactly-once recipient
counts identify all eight cells per topology. The Python fixture's explicit
native-LXMF mode retains LXMRouter callbacks without raw-resource interception
or keepalive wrappers. Only middle B is stopped/joined and replaced at the same
endpoints with a fresh empty state directory and new identity. Endpoint/edge
PIDs, identities and native Link-cache observations survive; the fixture does
not clear or pre-teardown endpoint Links. Fresh path announcements/requests are
normal protocol actions. Freshly learned paths are distinct from disk restoration.
The entire three-run set must pass on a candidate; retain failures and diagnose
them before a new candidate, without changing these bounds.

The final strict-bound candidate now passes all 24 cells in 104.92 seconds.
Both edge remote routes name the replacement middle identity before traffic;
endpoint paths report three hops in every phase. One immutable executable is
used across the set, native Link caches are retained, exact signed recipient
content and independent sender representation/receipt checks pass. Maximum
individual cells are 15.128, 14.864 and 14.633 seconds by run. The original
two-relay/raw-Resource fixture also passes with its default mode. See PROGRESS.md
for all candidate logs and review fixes. This is the finite local deeper gate;
the family inventory and whole-item integration/hosted gates remain open.

## Gates

Start with failing production-boundary regressions, then the smallest owner fix.
Use SDK v2/typed ZeroMQ for interface operations where available. Run focused
package/reference checks, issue-369 scanner after diagnostics edits, formatting,
Clippy, boundaries/module size and architecture. Broaden to workspace and release
checks as the main plan requires, retaining the known package conflict honestly.
Independent PRE/correctness/maintainability/POST/verifier gates, exact final-head
hosted checks and actual merge are required. Items 5 and 6 remain unimplemented
until their preceding PR is merged.
