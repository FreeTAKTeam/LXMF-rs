# Item 4 configured-root lifecycle cutover

This refines the remaining family gate in [TRANSPORT.md](TRANSPORT.md) after
`a0772c1d`; it does not add another PR or change the numbered plan. The three
discovery process cases and finite deeper relay matrix are already qualified.
No whole-family row is yet complete.

## Concrete gap and owner

The original gap was that the startup dispatcher returned native handles and
effects but primarily seeded the mutation worker's map for the four named-management
kinds and private Backbone clients. Discovery projected that map, so other
configured bootstrap roots missed both disable and restore. Unsupported seed keys
were skipped before the restore diagnostic. Copying the four-kind spawn match
would have preserved the gap and duplicated carrier defaults, status and shutdown
rules. The board below distinguishes completed ownership/disable work from the
remaining general restoration work.

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
| Status/control/shutdown cutover (locally verified) | Existing entry retains typed handles and unique Auto shutdown; one publication owner replaces old fixed loops | Single two-second publication, synchronous initial controls, old-handle rejection, cancelled shutdown recovery, admission fencing, all-worker joins and unrelated-root continuity pass. General restoration remains next |
| Trusted restoration | Initial and restoration dispatch share constructor/defaults/context, including selected listeners | Disable → native release → restore exact authenticated traffic/status; retain failed readiness and no transient plaintext fallback |
| Family composition | Qualify the finite twenty-row inventory using its applicable native/software backends | Managed streams/UDP/Pipe, PTY Serial/KISS, Auto/SAM, simulated radio/mesh, available BLE and shared-local cells; failed/degraded outcomes remain explicit |
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
recorded in PROGRESS.md. General trusted restoration and the twenty-family matrix
remain open.
