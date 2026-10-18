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
policy before constructing a worker. Configured bootstrap root cutover is still
required; these changes do not qualify every family cell below.

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
| Local Unix shared instance | Real plaintext exception across parent IFAC rotation | Immediate callback readiness and live non-IFAC policy |
| Local TCP shared instance | Shared-instance integration | First-frame plaintext exception, readiness and live non-IFAC policy |

`reticulum_ble` currently reports `native_backend_pending`; retain truthful
degraded configuration/status tests. Its missing native dual-role implementation
belongs to #650, outside items 1–6. Do not convert an absent carrier into positive
IFAC evidence. Physical equipment, RF and public networks remain outside this PR.
Only TCP client/server, UDP and Pipe currently support live named management;
other rows use configured restart/native reconnect rather than speculative APIs.

## Duplicate and persistence contract

- Preserve the frozen filter's ordered branches: attached-client bypass, foreign
  transport rejection, six context exemptions, Plain/Group rules, generation
  membership and Single-Announce replay. Inventory all 21 contexts separately
  from valid production packet shapes and explicit stronger Rust ingress guards.
- Remember transit Link packets only at the correct routing decision. Transit
  LRPROOF validation/forwarding differs from local pending LRPROOF remembering
  before signature validation. Preserve signed hop rebalancing and same-interface
  direction rules. Wrong-interface local Data forgets both generations so the
  identical frame can arrive correctly later.
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

Single-packet Python RequestReceipt remains SENT even after its PacketReceipt is
proved. Request-Resource completion sets DELIVERED. Timeout or response rejection
must finish either pending state once; a later duplicate/valid response cannot
reopen a rejected request. Exercise actual packet and Resource paths, exact IDs,
closure/cancellation, lost notification and oversized-then-later response cases.

Discovery cases include accepted/rejected/missing/malformed implementation and
version, default/override, persisted legacy IFAC, stale entries, duplicate names
and endpoints, Linux Backbone versus Windows/macOS TCP selection, monitoring
deadlines, bootstrap restoration, detach/reset races, inheritance and unrelated
traffic. Both Python 1.5.5 directions must retain honest implementation names:
Python's default rejection of Rust is a result, not an invitation to impersonate
RNS. Session detach suppression is explicit Rust operator intent; failed explicit
reattach preserves it and queued discovery work rechecks it before commit.

The deeper relay lane will extend the existing Python shared-owner/client and
Rust-relay fixture to three Rust relays. Predeclare three fresh process runs,
four delivery cells (small Direct and Direct Resource in both directions), each
before and after replacement of the intermediate relay: 24 successful message
cells are required. Record exact topology, readiness predicates and protocol-derived
deadlines beside the runnable case before its first run. Keep every run/result;
after a confirmed fix, a new candidate runs the whole declared set. Do not accept
retries-until-green or relax retry/timeout/performance policy.

## Gates

Start with failing production-boundary regressions, then the smallest owner fix.
Use SDK v2/typed ZeroMQ for interface operations where available. Run focused
package/reference checks, issue-369 scanner after diagnostics edits, formatting,
Clippy, boundaries/module size and architecture. Broaden to workspace and release
checks as the main plan requires, retaining the known package conflict honestly.
Independent PRE/correctness/maintainability/POST/verifier gates, exact final-head
hosted checks and actual merge are required. Items 5 and 6 remain unimplemented
until their preceding PR is merged.
