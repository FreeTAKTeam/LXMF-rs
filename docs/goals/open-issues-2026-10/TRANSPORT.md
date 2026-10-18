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
Record exact test names/results when qualified; no row is complete yet.

| Family | Existing bounded evidence | Missing composition to qualify |
| --- | --- | --- |
| TCP server / Backbone | Strict/best-effort bind failures, accepted-child rotation/restart | IFAC named reload rollback, detach/reattach and unrelated traffic continuity |
| TCP / Backbone client | Authenticated mixed TCP, connection/hot-apply tests | IFAC connection failure, configuration and reconnect |
| UDP unicast | Daemon/mixed rejection, rotation/restart, failed replacement retry | Raw-record alias/type collisions and named-management composition |
| UDP multicast | Production multicast routing helper and common codec | Authenticated ingress/egress, virtual peer rotation, replacement/restart |
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
