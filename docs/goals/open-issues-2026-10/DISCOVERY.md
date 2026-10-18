# Item 4 discovery runtime contract

This refines [PLAN.md](PLAN.md) and [TRANSPORT.md](TRANSPORT.md) after local
checkpoint `f5be0db8`. Configuration/planner preparation and a bounded native
runtime now pass local review; target-process and full-family qualification
remain open. These are local checkpoints, not another PR. Exact target RNS is
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
  attach/reload to the endpoint clears suppression only when the requested
  operation succeeds at native readiness, not queue acknowledgement. A failed
  reload retains suppression even when rollback succeeds. Do not
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
| Immutable config and policy — locally verified | Private parsed views, one file snapshot, unchanged public structs | Valid/default/invalid override, independent old typed-view fallbacks, snapshot replacement race and existing bootstrap malformed-IFAC regressions |
| Pure planner gaps — locally verified | Policy-aware initial delegate, endpoint exclusions and collision allocation through existing planner | Exact target controlled predicate/name cases; old callers preserve defaults; malformed/missing/old/current implementation and version, override and unsupported-type controls |
| Native discovery composition — bounded local verification | Trusted origin inside existing worker, initial/persisted/wakeup/periodic path, prepared carrier and truthful status | Native IPv4/IPv6 exact packet provenance; competing names/keys/endpoints/slots; external Apply retention; forged metadata rejection; blackhole authority and weak-queue closure. Local daemon-process connection also passes; this is not a Python peer trace |
| Monitor and operator intent — bounded local verification | Existing lifecycle planner, native stop/join, session endpoint suppression, trusted four-kind plus Backbone bootstrap recipes | Fractional/skipped ticks, native observation time after lock acquisition, recovery, teardown, failed/successful explicit attach, detached endpoint suppression and dropped-owner cleanup; exact bootstrap keys and retained UDP/TCP status refresh. Other family recipes remain open |
| Target process traces — pending | Three fresh local topologies at exact target | Rust accepts qualified Python by default; Python rejects honest Rust by default with positive readiness control; Python override accepts honest Rust. Prove the selected native interface and exact receiver bytes, not a connection counter alone |

The native worker starts after the complete startup snapshot and can create the
transport for discovery-only configurations. Its existing managed map uses a
private key enum to distinguish configured keys from discovery endpoint hashes;
the per-snapshot lookup is ephemeral, not a second registry. Display names never
act as native keys. Native blackholes check both network and transport identity.
The first-down anchor is captured after native status observation, preventing
time spent waiting for the manager from becoming claimed downtime.

Private startup parsing retains the reference announce defaults (3,600 seconds,
grace 5, penalty 0) independently of the unverified override. Zero/invalid values
follow the documented fallback. Public configuration layouts remain unchanged.
Restored UDP/listener/pipe carriers reuse their existing status refreshers;
retained TCP-client handles are sampled by the mutation owner. Stop/join settles
every owned worker before returning an error and publishes retired record absence
even when stopping reports a diagnostic. This does not qualify every carrier's
bind-failure cancellation behavior.

The validated-event regression proves persistence precedes mutation admission,
and uses the production publisher's honest implementation `LXMF-rs`. It begins at
the validated event boundary; it does not prove outer network authentication.
Full final daemon validation passes 31 library and 616 binary tests. See
[PROGRESS.md](PROGRESS.md) for retained failures, exact local checks and limits.

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
