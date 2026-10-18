# Item 4 discovery runtime contract

This refines [PLAN.md](PLAN.md) and [TRANSPORT.md](TRANSPORT.md) after local
checkpoint `f5be0db8`. It is preparation, not completed discovery qualification
or another PR. Exact target RNS is
`7f2b3b9b524c9386316379af1313b43a5e4f7a5d`; canonical pins are unchanged.

## Owner and cutover

Persisted evidence stays in InterfaceDiscoveryStore, populated by the existing
authenticated discovery announce worker. The existing interface mutation worker
owns candidate admission, name/endpoint/slot reservation, prepared installation,
status publication, monitoring and removal. Extend its existing managed entry
with private trusted operator/discovery origin. Caller `_runtime` never grants
origin or changes native discovery bookkeeping. External Apply must preserve
internal discoveries rather than remove them for being absent from its four-kind
desired map. Conflicting operator input cannot replace an internal worker by name.

Start discovery only after the complete startup interface-record snapshot has
been published. Parse daemon configuration, runtime policy and rejected-IFAC
diagnostics from one immutable file read, retaining their independent existing
fallbacks. Add the upstream `autoconnect_unverified_implementations` setting in a
private parsed-runtime wrapper; keep exhaustive public DaemonConfig and
ReticulumRuntimePolicy layouts and existing signatures intact. Invalid additive
override input defaults only that option off, with a diagnostic; it cannot discard
otherwise-valid established runtime fields that previously ignored the option.
Activate only when discovery and a positive autoconnect limit are configured,
for a standalone/shared owner; an attached client does not start peer discovery.

Use the existing bounded mutation queue for authenticated persisted observations,
held weakly by the announcement side so it does not extend queue lifetime.
Backpressure queue admission after persistence rather than dropping a live
observation; report a closed worker explicitly. Five-second store polling covers
the reference's eligible periodic candidates. Do not create another interface
mutation task or registration map. A skipped missed tick avoids a burst of old
monitor actions after a slow mutation. These lifecycle choices use Tokio's
[WeakSender](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.WeakSender.html)
and [missed-tick policy](https://docs.rs/tokio/latest/tokio/time/enum.MissedTickBehavior.html).

## Frozen reference bindings and explicit Rust intent

- Initial discovery lists transport entries, including retained unknown/stale
  entries; periodic refill lists available transport entries. Source allowlist,
  removal age, malformed-record handling and legacy IFAC sanitation remain in
  the store. Apply the existing native blackhole authority too.
- A live authenticated/persisted observation directly attempts admission without
  an initial/periodic transport filter, as target Discovery.py's callback does.
  Type, qualification, endpoint safety, native reservation and suppression still
  apply. Periodic scans are not claimed to replay arbitrary lost live callbacks.
- Only BackboneInterface is auto-connectable. Default qualification is exact
  implementation `RNS` with version at least 1.5.2. The explicit override changes
  qualification, not supported types or endpoint safety. Add a policy-aware
  initial-planning delegate while preserving existing callers' defaults.
- Preserve exact endpoint hashing/equality from the existing planner; do not add
  speculative DNS resolution or normalization rules. Apply target exclusions
  for onion/Yggdrasil endpoints and exact 127.0.0.1/0.0.0.0. Local process tests
  use another loopback address or hostname, not an exception to this rule.
- Allocate the original advertised name, then ` (2)`, ` (3)`, etc. Reserve against
  native records and the current managed tree before any installation await.
  Retaining disabled operator names is the existing Rust configuration boundary;
  no discovered name may overwrite that record. Endpoints and slots are likewise
  rechecked by the single mutation owner.
- On Linux use the existing Backbone TCP tuning/liveness builder; Windows/macOS
  select ordinary TCP. Default mode is Gateway for transport owners and the
  ordinary carrier default otherwise; explicit mode, gravity zero and configured
  overrides retain their meanings. Use 5 Mbps and the existing announce policy
  defaults, preserving optional `announces_to_internal` semantics.
- Monitor actual native online state every five seconds. Mark the first observed
  down time, recover without a second worker, and detach after at least twelve
  seconds observed down using an exact monotonic anchor in the existing managed
  entry. The existing lifecycle planner's integer projection cannot authorize
  teardown before actual elapsed time; test fractional and skipped-tick cases.
  Preserve the reference reservation `maximum / 4` and
  periodic one-candidate random refill when free slots exceed it. Do not replace
  this with retries-until-green or a silently different unlimited refill policy.
- Keep public named attach/reload config-backed and restricted to its existing
  four kinds. Origin-checked discovery detach can stop an internal Backbone and
  suppress that endpoint for this native owner's session. A successful configured
  attach/reload to the endpoint clears suppression at native readiness/rollback
  success, not queue acknowledgement; failures retain it. Do not
  recover a detached recipe from public status or caller metadata. A new owner
  starts a new session. Non-config discovered-name attach/reload is not added.
- Bootstrap-only disable/restore uses trusted startup recipes, not public status
  reconstruction. The first runtime composition covers the existing four managed
  kinds plus the private Backbone client path. Other configured family recipes
  remain an explicit item-4 family-composition requirement; they must be resolved
  before whole-item completion. Do not call a partial bootstrap composition general
  bootstrap recovery or silently mark unsupported families restored.

## Sequential board and evidence

| Task | Main-agent output | Evidence gate |
| --- | --- | --- |
| Immutable config and policy | Private parsed views, one file snapshot, unchanged public structs | Valid/default/invalid override, independent old typed-view fallbacks, snapshot replacement race and existing bootstrap malformed-IFAC regressions |
| Pure planner gaps | Policy-aware initial delegate, endpoint exclusions and collision allocation through existing planner | Exact target controlled predicate/name cases; old callers preserve defaults; malformed/missing/old/current implementation and version, override and unsupported-type controls |
| Native discovery composition | Trusted origin inside existing worker, initial/persisted/wakeup/periodic path, prepared carrier and truthful status | Real connection and exact packet provenance, competing name/endpoint/slot records, failed install rollback, external Apply retention, forged metadata rejection and unrelated traffic continuity |
| Monitor and operator intent | Existing lifecycle planner, native stop/join, session endpoint suppression, bootstrap recipes | Five-second scheduling and twelve-second boundary, recovery, automatic teardown, bootstrap restore, success/failure attach/reload, detach/wakeup races and dropped-owner cleanup |
| Target process traces | Three fresh local topologies at exact target | Rust accepts qualified Python by default; Python rejects honest Rust by default with positive readiness control; Python override accepts honest Rust. Prove the selected native interface and exact receiver bytes, not a connection counter alone |

For the three target process traces, declare 15 seconds for readiness and
30 seconds for discovery/connection/traffic in each fresh topology. Default
rejection observes at least two five-second monitoring periods and has a
qualified positive control, while retaining a free autoconnect slot. Assert that
the honest Rust advertisement was decoded/persisted with its real implementation
metadata. Retain every failed run and repair causes without
increasing these bounds or rerunning until green. This is separate from the
three-run deeper relay replacement matrix in TRANSPORT.md.

Main implements sequentially. PRE, correctness and maintainability POST reviews
gate checkpoints. Run focused regressions first, affected full suites/Clippy,
diagnostics, explicit include-file formatting, boundaries, size and architecture.
Update roadmap/matrix/progress with actual evidence and unresolved family cells;
there is no hosted or broad item-4 completion claim at a local checkpoint.
