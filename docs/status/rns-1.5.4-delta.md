# Forward RNS 1.5.4 parity candidate

Status: **open and incomplete**. This ledger is the forward candidate for issue
[#605](https://github.com/FreeTAKTeam/LXMF-rs/issues/605); it does not replace the
active RNS 1.5.2 release baseline in [`rns-1.5-delta.md`](rns-1.5-delta.md),
and it does not claim that PR #604 completes the epic.

## Frozen reference boundary

| Role | Implementation | Version | Revision | Authority |
| --- | --- | --- | --- | --- |
| Active release baseline | Reticulum-Python | 1.5.2 | `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6` | Existing release gates and historical status |
| Forward parity candidate | Reticulum-Python | `1.5.4-dev` | `99de23c040d507e3fefca19e87b182302902725d` | `tools/interop/independent-implementations.toml` `[parity_target]` |
| LXMF companion reference | LXMF-Python | pinned active revision | `727830cefda83d9c6e3982b48675425f3f988f9c` | Generated inventory input |

The forward RNS revision is an immutable development commit, not a release tag.
Changing it requires a reviewed update to the canonical manifest and this
ledger. The active release pin remains unchanged until the exact candidate
acceptance gate is complete.

## Callable inventory measurement

The existing generator was run against the exact detached checkouts with:

```text
python3 tools/scripts/python_surface_inventory.py \
  --python-rns-path .tmp/python-refs/Reticulum/RNS \
  --python-lxmf-path .tmp/python-refs/LXMF/LXMF \
  --mapping docs/status/python-surface-mapping.json \
  --json-out target/issue-605/python-surface-parity-1.5.4.json
```

The candidate scan found **1,868 callable/manual rows**: 1,867 provisional
partial rows and one provenance-backed `CRNS` not-applicable row. It adds ten
callable IDs relative to the 1,858-row active baseline:

- `RNS.Interfaces.util.HDLC.HDLC.frame`
- `RNS.Utilities.rngit.media.available_backends`
- `RNS.Utilities.rngit.media.convert_file_to_webp`
- `RNS.Utilities.rngit.media.convert_to_webp`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.clean_links`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.cleanup_link_temporary_resources`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.get_webp_stream`
- `RNS.Utilities.rngit.pages.NomadNetworkNode.serve_media`
- `RNS.Utilities.rngit.server.ReticulumGitNode.load_allowed_permissions`
- `RNS.Utilities.rngit.server.ReticulumGitNode.update_repository_permissions`

The candidate output deliberately demotes inherited callable mappings to
`partial`. Existing wildcard rules are useful navigation hints, but a symbol
match is not behavioral proof and cannot promote a forward row to complete.
The generated active-baseline JSON remains 1,857 complete, zero partial, and
one not-applicable for historical release compatibility; its separate
`behavioral_contract` reports the current forward status as incomplete.

The Verify workflow now checks out the exact parity target and runs this
generator against `Reticulum-parity/RNS` and the pinned LXMF checkout. An
unmapped target callable or stale target revision fails the job; the generated
candidate JSON/Rust summary is uploaded as raw evidence. The same `xtask` docs
and release helper accepts `PYTHON_RNS_PARITY_PATH` and
`PYTHON_LXMF_PARITY_PATH` for local or hosted exact-target runs. Verified
behavioral rows additionally require an existing repository-relative evidence
artifact; planned or unverified rows remain explicitly incomplete.

## Behavioral contract and child work

The machine-checked contract is stored in
[`python-surface-mapping.json`](python-surface-mapping.json) and materialized
in the generated [`python-surface-parity.json`](python-surface-parity.json).
It requires every forward requirement to name its Python reference path, exact
reference commit, Rust owner surface, implementation status, evidence status,
test command, evidence artifact, and owning issue. The #615 software gate is
closed; eight other tracked requirements remain open, with physical acceptance
separately tracked under #616:

| Owner | Requirement | Current status |
| ---: | --- | --- |
| #607 | Review and integrate the initial PR increment | closed; merged PR #604 and its acceptance checks are recorded |
| #608 | Wire IFAC into production carrier ingress and egress | partial; pinned Python TCP/UDP Channel and Resource evidence, UDP daemon success/rejection including valid-frame tampering, live credential rotation and restart, shared-instance and virtual-child policy traces, a Unix PipeInterface worker IFAC/HDLC loopback, serial-stream wrong-key rejection plus authenticated ingress/egress, KISS/AX.25 stream wrong-key rejection plus authenticated ingress/egress with runtime-counter assertions, outbound I2P fake-SAM stream and incoming accepted-stream worker regressions, Meshtastic tunnel and Weave stream regressions, and AutoInterface peer-data, LoRa, RNode bearer, and RNodeMulti KISS-vport wrong-key rejection/authenticated ingress/egress; broader carrier and lifecycle matrices remain pending |
| #609 | Close transport, local-client, and shared-instance gaps | partial; two-peer shared-instance recovery delivers one queued OPPORTUNISTIC LXMF message after relay replacement; the two-relay restart test transfers fresh raw Resources in both directions. An isolated pinned-Python test verifies in-flight large DIRECT LXMF Resource retry after upstream relay replacement, on a distinct Link and Resource, with exactly one message delivery. Other evidence includes attached-client duplicate delegation, standalone repeated-LinkRequest suppression, pinned-Python clean-close reason mapping, five-attempt Channel retry exhaustion over localhost TCP, transport-disabled local LinkRequest delivery from a virtual child, expired persisted-route rejection followed by fresh-announce recovery, and strict announce-job deadline equality with exactly one local-client retransmit. Deeper relay replacement and duplicate behavior remain open; the broad reverse-delivery scenario has an intermittent B-to-A timeout |
| #610 | Prove Resource collision, stream, and mixed-peer behavior | partial; exact-checksum 50 MiB pinned-Python transfers rerun in both directions at PR #630 head `0d9b5dd6`, within the 512 MiB per-process peak-RSS bound; deterministic sender-window anchor and global hashmap-segment indexing are regression-tested; Python peers verify response packet selection at MDU-1 and MDU and Resource selection at MDU+1; a pinned-Python sender cancellation after data in split segment 2 produces Rust `InboundFailed(remote_cancelled)` through production TCP/Link/Resource; broader timeout/reconnect and consumer callback/status evidence remains open |
| #611 | Exercise every reference utility through real network workflows | partial / unverified |
| #612 | Match rngit permission, resolver, work, storage, and wire schemas | partial / unverified |
| #613 | Match rngit NomadNet pages, media, and link cleanup | partial / unverified |
| #614 | Validate native interface runtimes and Windows BLE behavior | partial / hardware-unverified |
| #615 | Run differential conformance and exact-candidate software release acceptance | closed; scoped software acceptance completed, with physical/platform/public-network/soak work tracked separately |
| #616 | Maintain separate physical, platform, client, network-soak, and operational evidence | not-applicable to software / hardware-unverified |

The SDK/RPC parity advisory now exposes an optional `forward_behavioral`
checkpoint generated from this contract, including the exact target revision,
partial/incomplete level, requirement counts, and verified-evidence count. The
existing overall, Reticulum, and LXMF fields retain their active-baseline
callable-inventory values and do not prove forward behavioral completion. No
row is promoted by a local parser, mock, attached-node-only check, or old
release artifact.

The bounded contract-fixture evidence is recorded in
[`evidence/606-behavioral-contract.md`](../goals/reticulum-reference-parity-605/evidence/606-behavioral-contract.md),
and the merged PR #604 review/check record is recorded in
[`evidence/607-pr604-review.md`](../goals/reticulum-reference-parity-605/evidence/607-pr604-review.md).
Both remain partial and do not promote the forward behavioral rows.

The #608 implementation slice now has committed local software evidence in
[`evidence/608-ifac.md`](../goals/reticulum-reference-parity-605/evidence/608-ifac.md):
configured carrier ingress/egress, live reconfiguration, child inheritance,
fail-closed malformed- and wrong-key-frame handling, and feature-gated carrier
builds are covered. Pinned Python↔Rust TCP Channel/Resource and UDP
Channel/Resource software traffic is evidenced; UDP Resources transfer in both
directions on the same Rust-initiated Link. PR #628 additionally verifies
bidirectional authenticated UDP direct-message delivery through separate
`lxmd`/`reticulumd` and Python processes, with an active Python link and zero
live IFAC violations. Its later regressions also reject a valid authenticated
UDP frame tampered in transit before routing, rotate credentials through live
interface reconfiguration, reject the old key, and preserve identity plus
authenticated delivery across daemon restart. Another pinned-Python trace
exercises an IFAC-protected UDP shared-instance owner, an attached Rust local
client, and a separate Python peer, with bidirectional delivery and zero IFAC
violations; a focused ingress regression verifies inherited policy on a
virtual child. Invalid live IFAC reconfiguration now returns a static,
structured `CONFIG_INVALID_IFAC` RPC error without displacing the active
authenticated configuration; after restart a plaintext peer remains rejected.
The focused RPC regression also verifies that failed interface application
does not replace stored interfaces. The packet decoder now derives authentication and verified-wire
provenance from one IFAC-state snapshot across hot reconfiguration. A Unix
subprocess regression also exercises the production PipeInterface worker with
the reference 8-byte default IFAC tag and authenticated HDLC echo; this is
loopback carrier evidence, not a Python-peer trace. A separate deterministic
serial-stream test rejects a wrong-key frame before admission and verifies
authenticated ingress and egress through the production serial stream worker;
it does not claim physical serial evidence. The KISS/AX.25 stream worker also
has a deterministic duplex regression for wrong-key rejection, authenticated
ingress/egress, the 8-byte default tag, and runtime counters; it is not modem,
radio, or Python-peer KISS evidence. The outbound I2P peer loop now also has
fake-SAM regressions for wrong-key rejection before packet admission,
authenticated ingress/egress, and shared parent IFAC rotation on an established
virtual peer. These are software stream tests, not public I2P evidence, and do
not cover broader tunnel lifecycle behavior. Other carrier families remain
open; hardware and public-network evidence remain separate acceptance gates.
The transport ingress suite also verifies that an already-attached accepted
child decoder rejects the old parent IFAC key and admits the newly configured
key after a live parent update. The existing config propagation was sufficient,
so this increment adds regression evidence without a production behavior
change; #608 remains partial.
Commit `49b7999f` adds software-only production-worker regressions for
Meshtastic tunnel reassembly, Weave streams, and the incoming I2P accepted
stream. Each rejects wrong-key traffic before admission and verifies
authenticated ingress/egress; the I2P case uses a local TCP pair rather than a
SAM router, so the SAM accept-loop integration remains unverified. The transport library passes 823 unit tests, including 394 tests
matching the `ifac` filter, with all-feature Clippy, architecture boundaries,
module-size, and formatting checks passing. These new adapter cases narrow but
do not close the remaining carrier-family or physical acceptance gaps.

The #609 implementation slice now has committed local software evidence in
[`evidence/609-transport-local-shared.md`](../goals/reticulum-reference-parity-605/evidence/609-transport-local-shared.md):
parent/child classification, shared-boundary receive-hop accounting, immediate
single local-client retransmit, direct sibling announce fan-out, daemon local
TCP/Unix parent marking, and a direct pinned Python↔Rust application/restart
trace are covered. A new ignored pinned-Python trace also exchanges a Channel
message and reply between two Python nodes over two Rust carrier interfaces
owned by one forwarding Rust transport; its companion trace forwards a split
Resource and waits for the remote endpoint's exact size and SHA-256 callback.
The same topology now proves application-link close/reconnect, and a separate
fault-injected pinned-Python trace proves Rust pending-link establishment
cleanup after the path is available. A pinned two-carrier Python trace now
injects a duplicate link-request proof and asserts Rust forwards it exactly
once before the Python client completes a single Channel delivery; this case
runs in PR `Verify` CI. Another exact-target trace duplicates an ordinary
LinkRequest on ingress and verifies the Rust forwarder emits one copy; focused
ingress tests also verify attached clients accept duplicates for owner-side
filtering. These cases do not close the remaining packet/proof classes. Rust
link events now expose the pinned
`TIMEOUT`, `INITIATOR_CLOSED`, and `DESTINATION_CLOSED` reason codes, with
role-aware and establishment-timeout regressions. A pinned-Python clean-close
trace over TCP verifies that Python `Link.teardown()` reaches the Rust caller
as `INITIATOR_CLOSED`; a second pinned-Python TCP trace drops Link delivery
proofs, verifies all five Channel attempts, and observes the resulting
caller-visible reason in Rust. This is localhost software-carrier evidence;
physical and public-network evidence remains separate. The row remains
partial; broader shared-instance
and multi-hop production traces comparing other packet/proof duplicate classes
remain unverified. A new two-peer pinned-Python shared-instance
trace verifies path relearning, fresh links, and raw packet exchange in both
directions after replacing the transport-enabled Rust daemon. It queues an
OPPORTUNISTIC LXMF message while the relay is down and verifies receipt and
acknowledgement after restart; this does not establish queued DIRECT delivery.
The two-relay restart test also transfers fresh raw Resources in both directions
after recovery. A separate isolated pinned-Python test pauses a large DIRECT
LXMF Resource transfer in flight, restarts and replaces its upstream relay,
then verifies retry on a distinct Link and Resource and exactly one message
delivery. These are bounded acceptance cells: deeper multi-relay replacement
and duplicate behavior remain open, and the broad reverse-delivery scenario
still has an intermittent B-to-A timeout. #609 therefore remains partial; none
of this evidence completes #609 or parent issue #605. Physical/HIL evidence is
excluded. A focused
transport save/restart regression proves a newer cached `PATH_RESPONSE`
announce supersedes scheduled state without becoming retransmission work after
restore. A real-socket `TcpClient` regression also proves redial preserves the
interface identity and resumes bidirectional HDLC packet traffic; it is
carrier-level evidence, not the broader duplicate or queue-recovery cases.
An additional focused regression verifies that a locally hosted destination's
valid announce, when received through a shared-instance child, does not become
a remote route or fan out to a sibling client. This proves one software matrix
cell only and does not promote the #609 row. Path-table startup restore also
rejects a cached announce whose recovered identity is already blackholed,
matching the pinned `Transport.py` startup predicate; the save/blackhole/restore
test verifies the route and recovered destination identity are both absent.
This closes one cached-route validity cell only and leaves #609 partial.

A disjoint transport-disabled regression now covers a different local/shared
matrix cell: a LinkRequest arriving from one shared-instance virtual child for
a locally hosted destination produces exactly one `LinkRequestProof` routed to
that child; the sibling receives no transit copy. This matches the frozen
Python source path in `Transport._inbound`, `Destination.receive`, and
`Link.validate_request` at Reticulum `99de23c040d507e3fefca19e87b182302902725d`.
It is a source-level comparison plus production Rust packet-processing test,
not a live Python/Rust socket trace, and does not promote #609.

The #610 implementation slice now has committed local evidence in
[`evidence/610-resource.md`](../goals/reticulum-reference-parity-605/evidence/610-resource.md):
deterministic collision regeneration, window-bounded fragment admission,
the exact moving sender-window anchor and hashmap-segment index at a segment
boundary, link-close terminal resource events, local loss/duplication/
reordering recovery, split cancellation cleanup, pinned-Python cancellation terminal events in both
directions, reader-backed source retention, a pinned-Python split reader-backed
transfer with an exact SHA-256 acknowledgement, and
two-carrier pinned-Python split Resource forwarding with an exact remote
endpoint callback, plus
bidirectional pinned-Python release-profile transfers covering empty, boundary,
split, and 50 MiB payloads with exact SHA-256 checks. A bounded independent `rns-rs` PR
profile additionally passes 1 MiB direct/multi-hop transfers, deterministic
loss recovery, terminal timeout, and latency cases. Two pinned-Python fault
matrices now cover loss, duplication, reordering, and complete missing-fragment
terminal failure in both Python-sender → Rust-receiver and Rust-sender →
Python-receiver directions through real TCP/HDLC carriers, and a pinned-Python
receiver-shutdown trace observes Rust's terminal outbound failure after the
Resource advertisement is admitted. That cancellation now uses a reader-backed
Rust split send. A reader-backed pinned-Python matrix also covers loss,
duplication, reordering, and complete missing-fragment failure, while a
separate trace injects a later source-read error after the first segment is
accepted; the split success trace now reads from a real file handle. Commit
`8b29132c` adds the reciprocal pinned-Python file-like-reader fault trace: the
reference reader raises during later segment preparation, the Rust receiver
emits a terminal inbound failure, and the Python sender exits unsuccessfully
after its bounded timeout. The current #610 candidate additionally drops
Resource traffic and keepalives during an in-flight 70,000-byte transfer,
observes `OutboundFailed` when the Link closes, restores forwarding, and proves
a second Rust-to-Python Resource completes with the exact SHA-256 acknowledged
on a fresh Link. A reciprocal Python-initiated trace now observes inbound
Resource failure and Link closure, reconnects with a distinct Link, and checks
the exact recovered Resource checksum against the Python sender report. The row
remains partial and unverified: both recovery directions now run in PR `Verify`,
but broader timeout traces, every consumer callback/status assertion, and the
full hosted/physical/soak matrix are still open;
exact 50 MiB peak-RSS values in both directions are now recorded by the
candidate's Linux release-profile memory probe. A focused daemon completion
consumer test now also verifies the transport-completion receipt metadata, peer
byte accounting, duplicate-notification suppression, exactly-once event, and
tracking cleanup. A companion timeout-failure consumer regression verifies the
single `resource-failed` receipt metadata, duplicate suppression, tracking
cleanup, and peer backoff state. These focused tests do not certify the
remaining consumer callback/status matrix. Separate `lxmf-runtime` consumer regressions verify that actual
`OutboundFailed` and `OutboundCancelled` Resource events reach callers as
distinct transport errors and cleanup is attempted; broader SDK/daemon consumer
matrices remain open.

The 2026-09-23 #610 candidate adds the Python `RESOURCE_ICL`/`RESOURCE_RCL`
terminal distinction: inbound remote cancellation maps to `InboundFailed`,
outbound peer rejection maps to `OutboundRejected`, and local cancellation
remains `OutboundCancelled`. A pinned-Python receiver exercises the RCL path;
transport, SDK, daemon receipt/remote-control, `rncp`, and independent-event
consumers preserve the rejection outcome and cleanup. This is focused software
evidence and does not close the wider #610 callback/status or operational gates.

The #611 implementation slice now has committed local evidence in
[`evidence/611-utilities.md`](../goals/reticulum-reference-parity-605/evidence/611-utilities.md):
native `rncp` listener, discovery, Link identification, authenticated send and
fetch Resource workflows, binary-safe metadata, jail/save side effects, and
terminal failures across two independent Rust processes. A pinned Python/Rust
trace now also covers an identified Rust fetch client, Python's allow-listed
fetch jail, the Python reference's unassociated file Resource shape, exact
binary save, and rejection of a second identity. The `rncp
--no-compress` option now reaches the Resource manager for both outbound sends
and fetch responses. A pinned Python/Rust trace now proves reciprocal identity
allow-list authorization for the send and fetch roles, the ordinary
metadata-bearing Resource contract in both directions, and overwrite replacement
without a collision suffix on both Rust and Python save paths. Its exact file
assertion also exercises the completed Python fetch resource-conclusion/save
callback. Commit `3c6757ba` additionally covers default and explicit
no-compression send modes in both Python↔Rust directions, a bzip2-compressed
payload, and Python listener default/no-compression fetch responses into a Rust
client. The issue-specific `rncp_listener_reports_received_file_disk_error`
process regression forces the Rust listener's post-delivery file save to fail
and asserts its diagnostic, keeping transport receipt distinct from app-level
save status. The ignored exact-target `rncp_python_listener_reports_received_file_disk_error`
trace also verifies that a pinned Python receiver logs its save callback error
after a Rust sender reports successful Resource delivery. PR #631 adds
exact-target process assertions for packed and received Resource advertisement
transfer/data sizes and compression flags across Python→Rust sends, Rust→Python
sends, and Python default/`-C` fetch responses. Verify now runs that focused
compression matrix automatically. The new exact-target
`rncp_python_fetch_client_save_error_is_reported_but_never_resolved` trace
forces the Python fetch save directory to fail after preflight and observes the
callback's save-error output. The pinned callback returns without resolving the
transfer, leaving the client running; this is recorded as a reference defect,
not accepted terminal failure handling. The row remains partial and unverified
because accurate Python fetch-client terminal failure status, the
complete utility option/behavior matrix, rngit network workflows, and
pinned-Python receive-side cancellation remain open or owned by #612/#613. A slow-proxy
`rncp` regression now proves one delayed/rate-limited TCP send completes under
the adaptive timeout; this is software-path evidence only and makes no
carrier-specific or physical timing claim.
Commit
`2b281b87` also adds process-level assertions for a missing fetch and a denied
sender, including nonzero exit status and preserved failure categories.
Commit `053ef246` extends the same process gate to malformed allowed identities
and a save path that is a file rather than a directory. Commit `9dc9bd62`
adds an unused-endpoint process check that preserves the nonzero
`path discovery timed out` outcome. Commit `d66b19d1` adds a persisted-identity
listener restart check on the same TCP endpoint with a second binary transfer.
Commit `9b8e4ed6` adds a fetch save-directory disk-error check with nonzero
status and preserved `Is a directory` output. Commit `397a9525` adds explicit
client Ctrl-C cancellation handling with a nonzero status and preserved
`operation cancelled by user` output during path discovery. The new
`rncp_ctrl_c_during_resource_transfer_reports_cancellation` process test sends
SIGINT after the CLI announces the active Resource-transfer phase, requires
the same explicit cancellation error and nonzero status, and verifies the
receiver did not expose a completed file. It covers the native Rust-to-Rust
workflow; pinned-Python receiver cancellation remains open. Commit `e668ae60`
adds three concurrent
client processes with exact listener-side byte verification. Commit `a5f57dba`
adds flushed non-silent client phase output and an interrupted-Resource process
check with nonzero status and no partial saved file. Commit `27bb3fac` gates
network work on initial TCP client readiness, preserving cancellation handling
while proving the medium-path timeout lower bound after interface activation.
Commit `708dc980` adds a pinned-Python listener/Rust-client restart trace with
stable identity and exact binary transfers before and after restart.

Commit `f86ecc1c` adds the first two-direction pinned-Python `rnprobe` process
exchange: a Python `rnprobe` reaches the Rust daemon's opt-in
`rnstransport.probe` responder, and native `rnprobe` reaches a Python
`PROVE_ALL` responder. Both isolated TCP roles deliver two probes with zero
loss. Public/multi-hop, physical-carrier, and probe fault/restart evidence
remain open.

The #631 `rnpath` follow-up adds one software network trace beyond the existing
mock-RPC tests: a separate pinned-Python Reticulum process waits to announce
until after the Rust `rnpath-rs` client process is launched, using a separate
`reticulumd` process's live TCP RPC. The CLI returns the announced destination
and one-hop result. Verify runs the exact-target trace against the frozen
1.5.4 development checkout; path-table, remote-management, and broader utility
acceptance remain partial.

Commits `f24e0038` and `a32b6d71` add a bounded native `rnsh` TCP/Link/Channel workflow with
the frozen Python message family, exact no-aspect destination hashing,
authenticated/no-auth listener modes, root-scoped command launch, stream
forwarding, timeout, and mirrored exit status. Rust process/auth tests and
`e57afb99`'s reciprocal pinned-Python initiator→Rust listener and Rust
initiator→pinned-Python listener tests pass; `662dcdbe` orders the execute
envelope before stdin EOF and adds bounded reference-listener EOF grace, with
the immediate non-TTY EOF case passing. PTY/resize, full
fault/restart/cancellation coverage, native outbound compression, and
public/multi-hop evidence remain open; #611 stays partial/unverified.

The #612 implementation slice now has committed evidence in
[`evidence/612-rngit.md`](../goals/reticulum-reference-parity-605/evidence/612-rngit.md):
bounded node-owned permission resolvers, configured-access merging, canonical
companion paths, atomic permission refresh, production work-item handlers,
Python-shaped MessagePack persistence, Python-produced binary metadata, and
authenticated-peer signature verification on the live work path. Pinned
Python Link requests now reach the production
`git.repositories` service for `/git/list`, `/git/fetch`, `/git/push`,
`/git/delete`, `/git/create`, `/git/sync`, `/git/fork`, and `/git/mirror`; the
returned bundle passes `git bundle verify`, contains the expected main ref,
creates and removes a Python-named remote ref, registers new repositories,
syncs a configured remote, and clones local-source fork/mirror targets. The
same trace exercises `/mgmt/perms` and signed `/mgmt/work` create/list/view/
comment/edit/perms/complete/activate/delete requests, including invalid
signature rejection and binary identity/signature round-trip. Commit
`0ad07dc0` adds local regressions for atomic work-directory reservation across
independent node instances and rollback when proposed-document permission
setup fails. Commit `409ef98e` adds a pinned-Python production trace that
creates and signs a work item, restarts the Rust `rngit` process on the same
root and identity, and verifies list/view persistence. That trace now also
creates a numbered MessagePack comment before shutdown and verifies its ID and
content in the pinned Python `work_view` response after restart. A new Verify
test runs
four independent pinned-Python work creators concurrently against one Rust
server, checks distinct assigned IDs and persisted root files, and is included
in this issue's dedicated PR. It also exercises invalid list scope, malformed
document IDs, unknown operations, and the pinned Python `rngit work` CLI
lifecycle (create/list/view/edit/comment/perms/complete/activate/propose/delete)
over production Links with deterministic editor input. The row remains partial
and unverified because the hash-only compatibility seam, broader
cross-process/network restart and fault transcripts, non-work CLI paths, and
complete end-to-end rngit network matrix remain open; hosted evidence for the
new lane is pending. The local permission-update follow-up fixes a failed-read
transaction: configured state is committed only after the current sidecar is
successfully read, validated, and merged. Regressions verify that a malformed
sidecar read, failing resolver refresh, and failed atomic replacement do not
change the loaded permission policy. This is one verified software slice, not
completion of #612. A follow-up work-storage regression rejects malformed
MessagePack roots and trailing bytes; over a production Reticulum Link, the
pinned Python `rngit work view` command receives the reference-compatible
`REMOTE_FAIL` response `Error loading document`. Other malformed-document
shapes and operations remain unverified. A separate local follow-up now tests
Python-shaped missing/malformed metadata defaults and errors (including a
missing `edited` timestamp default), malformed top-level request error bodies
without filesystem changes, corrupt-root work
completion failure handling, and case-sensitive special permission aliases.
These additional regressions are unit-level only; their hosted result and
mixed-peer behavior remain pending, and #612 remains partial.

Commits `3dcd5259`, `869b8c84`, `02b75605`, and `f9c5b81e` also add a native Rust-client
request adapter and a production compatibility bridge. Its pinned-Python trace
covers reciprocal `/git/list` and raw `/git/fetch` Resource handling, verifies
the returned bundle with `git bundle verify`, covers an oversized `/git/push`
bundle and remote ref, and covers signed `/mgmt/work` requests including an
oversized request Resource. Commit `f26ce90d` verifies native Python-compatible
release list/view/latest/delete request shapes, and `e0dedb0e` verifies the
multi-step release create/init, artifact, finalize, and raw artifact-fetch
workflow; restart/fault evidence and the broader utility matrix remain open.

The #613 implementation slice now has committed evidence in
[`evidence/613-rngit-pages.md`](../goals/reticulum-reference-parity-605/evidence/613-rngit-pages.md):
the `nomadnetwork.node` service path, page/file endpoints, `var_*` request
fields, access/no-identity behavior, image markup, media filename metadata,
bounded WebP conversion with raw fallback, link-scoped temporary cleanup, and
explicit no-compression Resource responses. A new pinned Python/NomadNet
client trace establishes and identifies a real TCP Reticulum Link, checks
successful and negative repository/page/file requests, verifies denied access,
downloads raw media, checks metadata, size, and SHA-256, and validates a live
`ffmpeg` PNG-to-WebP response with filename metadata. The periodic cleanup sweep
now removes tracked temporary directories for stale, closed, or missing links;
a deterministic regression proves those states are cleaned while active-link
media is retained. A new injected deletion-failure regression verifies the
directory remains tracked and is removed on the subsequent Link cleanup retry;
the conversion-fallback and link-cleanup handlers log path/Link context.
A focused Unix timeout regression also verifies that both WebP pipeline child
processes are terminated and reaped after a bounded deadline. A separate-process
pinned-Python test now synchronizes on partial `/media` Resource progress,
tears down the Link, and verifies no false completion, receiver Resource state
or files, Linux server child processes, or serving temp directory remain; the
transport Resource-manager link-close regression asserts tracked sender and
receiver state is cleared.
The same PR adds a fail-closed Verify lane that installs ImageMagick and
GraphicsMagick and runs the production-Link WebP test with their `convert` and
`gm` commands. It checks real conversion, configured quality/resize arguments,
decoded dimensions, response metadata, raw fallback, and Link-scoped cleanup.
Hosted Verify run `36024299709` passed at PR #633 head
`317cc142dde34ebcdf30a3c007f7d5a5568557aa`. ImageMagick 7's `magick` command,
`avconv`, and full rendering parity remain unverified.
An abrupt Python process exit still left the Rust link
`Active` after 104 seconds without inbound traffic, so the live stale-transition
path and other filesystem failures remain unverified. The separate
`git.repositories` list/fetch/push/delete/create/sync/fork/mirror trace does
not complete the page issue's broader Git/work acceptance. The utility row
remains partial because ImageMagick 7 `magick`, `avconv`, complete reference
rendering, remaining page/file cases, other restart/fault cleanup paths, and end-to-end
rngit Git/work network workflows remain open. Commit `e41189c8` initially added
live malformed-media requests with missing keys, missing paths, and
insufficient path components. The later #613 production-handler follow-up
updates those cases to require the pinned scalar `False` response, with no
Resource metadata or media bytes, and adds same-Link coverage for private
access, absent blobs, malformed/empty paths, and invalid refs. The
key-presence contract is now explicit too: a present `None` key is accepted
and returns the expected Resource bytes and filename metadata, while an absent
key is denied; the Rust handler already matches. The
issue-specific follow-up observes the converted-media temp directory during
the active Python Link and verifies the production `LinkEvent::Closed` handler
removes it after teardown, against frozen Python commit
`99de23c040d507e3fefca19e87b182302902725d`. Periodic stale-link, fault, and
cancellation cleanup remain open.

The #614 implementation slice now has committed local evidence in
[`evidence/614-native-interfaces.md`](../goals/reticulum-reference-parity-605/evidence/614-native-interfaces.md):
the Windows BLE backend queries the WinRT paired-device selector and filters
scan candidates by strict paired Bluetooth address before normal identifier,
alias, or service matching; the full feature-gated transport test lane and
Clippy pass. The row remains partial and hardware-unverified because this
Linux host lacks a MinGW/Windows SDK sysroot, and native Windows, AutoInterface
platform, cross-family, live Python/client, and physical carrier evidence are
still open. A dedicated `windows-rnode-ble` PR check now builds the target-gated
WinRT resolver and runs the BLE library tests on a hosted Windows runner; its
result is pending until this PR's check completes and does not replace physical
paired-device evidence.
The current PR worktree also adds a worker-level software regression for
native-style notification EOF: cleanup precedes reconnect through a fresh
backend, and cancellation closes the recovered session. This does not verify
native GATT EOF or physical recovery, so #614 remains partial.

The scoped #615 software gate is complete at PR #626 head
`b863e1d115395232a41445dbdfe1ccb08ee6abeb` (merged as
`a649f51e9671007c08aeff469877038e2db7a716`), with detailed local and hosted
evidence in
[`evidence/615-release-acceptance.md`](../goals/reticulum-reference-parity-605/evidence/615-release-acceptance.md).
The final release check passed with 2,684 tests and one skip; hosted PR HIL
passed `23/23`, the exact pinned matrix passed `30/30` with zero skips, and the
Independent and CI workflows passed. This completes only #615's software
acceptance gate. The remaining forward behavior rows keep their own statuses,
the #605 contract remains incomplete, and physical/platform/client/network-soak
evidence remains separate under #616.

The #623 wire-conformance increment adds
[`evidence/623-wire-conformance.md`](../goals/reticulum-reference-parity-605/evidence/623-wire-conformance.md),
the committed byte corpus at
`tools/interop/python-rust-wire-conformance-v1.json`, and the stable
`cargo xtask interop` gate. Verify now runs the same Python decoder and Rust
decoder test and uploads the report. This bounded wire lane is one input to the
completed #615 software gate; it does not complete #605 while broader live,
fault, restart, multi-hop, platform, client, and hardware requirements remain
open under their respective rows and #616.

## Merged base increment

The candidate branch now includes the merged PR #604 base increment at
`3ed5932d` (RNS 1.5.4 BLE lifecycle/EOF handling, HDLC framing vectors, and
rngit work-transition and companion-sidecar corrections). Those changes are
preserved here as a bounded increment; they do not promote the forward
candidate or close issue #605. Their local tests and provenance checks were
inputs to the completed #615 software gate, while native hardware and
public-network claims remain outside local validation.

## Acceptance gate

This candidate is not release-complete until the contract coverage is complete,
all applicable rows have verified evidence, the exact candidate CI/release gate
passes, and the independent child issues have been reviewed. Hardware and
public-network evidence remain a separate axis and cannot be inferred from
software tests. The `--require-behavioral-complete` generator mode is expected
to fail while this ledger is incomplete.
