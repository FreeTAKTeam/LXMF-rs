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
The first four rows have the declared software lifecycle qualification below;
other rows remain open. Record exact test names/results when qualified.

| Family | Existing bounded evidence | Missing composition to qualify |
| --- | --- | --- |
| TCP server / Backbone | Four declared native traces plus alias admission controls qualify applicable configured error, child rotation, named TCP rollback/reload/detach/attach, restart and unrelated traffic | Locally qualified for these software lifecycle traces; exact-`localhost` IPv6 wire coverage depends on a dual resolver environment. Whole-item integration/hosted CI remain pending |
| TCP / Backbone client | Declared configured refusal, same-socket activation, live key change, reconnect, changed-key restart and exact unrelated traffic; preceding named constructor/hook traces | Locally qualified for these software lifecycle traces; whole-item integration/hosted CI remain pending |
| UDP unicast | Declared configured/named native trace qualifies committed live key rotation, actual failed bind/owned cleanup, authenticated rollback/reload/detach/attach and unrelated traffic; preceding raw IFAC/SDK guards remain inputs | Locally qualified for this software lifecycle trace; whole-item integration/hosted CI remain pending |
| UDP multicast | Declared native loopback group egress, signed unicast peer ingress, existing/new peer key rotation, peer-isolated replies and same-destination route rebinding after changed-key replacement/restart | Locally qualified for this software lifecycle trace; native group ingress is not claimed. Whole-item integration/hosted CI remain pending |
| Pipe | Authenticated cat roundtrip, wrong-key rejection, ordinary child cleanup | Configured startup failure, IFAC rotation/respawn/detach |
| Serial | Authenticated production stream, open/write failure primitives | Configured PTY open/error/reopen and changed-key restart |
| KISS / AX.25 serial | Authenticated worker and pinned Python PTY exchange | Configured error/reopen/restart, preserving AX.25 framing |
| KISS TCP | Shared authenticated KISS stream | Actual initial failure/reconnect with IFAC retained |
| I2P outgoing | Fake-SAM virtual peer authentication/rotation | SAM failure/reconnect, daemon status and cleanup |
| I2P incoming | Authenticated accepted stream | Fake-SAM accept-loop failure/restart and accepted-child rotation |
| AutoInterface | Authenticated peer transport bridge, owned-port restart | Configured activation failure/restart and live virtual-child policy |
| Meshtastic | Authenticated tunnel, live rotation and stop retaining policy | Configured failure/status and reactivation |
| Weave | Authenticated signed-handshake stream | Startup/reconnect errors, virtual endpoint rotation and restart |
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
The remaining sixteen family rows, whole-item integration, one PR, exact-head
hosted CI and merge remain open.

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
