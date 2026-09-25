# Issue #609: transport, local-client, and shared-instance behavior

Status: partial. The shared-owner post-restart reverse-delivery slice now has
local pinned-Python evidence; this is not a claim of full #609 or hardware
acceptance.

## Reference and scope

- Target Reticulum revision: `99de23c040d507e3fefca19e87b182302902725d`.
- Reference surfaces: `RNS/Transport.py`, `RNS/Reticulum.py`, and
  `RNS/Interfaces/LocalInterface.py`.
- Rust owners: `rns-transport` interface-manager, announce table, and
  announce processing; `reticulumd` local TCP/Unix startup.
- Tested source candidate: `ce8245ba` on `corvo/issue-609-transport-parity`
  (PR #629).

The implementation now classifies a local client from its parent relationship
and the parent's shared-instance marker. Active local TCP and Unix listeners
are marked as shared-instance parents; accepted children inherit the runtime
configuration and remain distinguishable from the parent. An announce from a
local client is queued with an immediate timeout and the retry limit already
consumed, producing one immediate transport retransmit before the entry moves
to the bounded cache. Accepted announces are sent directly to sibling local
clients as Type-2 transport announces, excluding the receiving child, so this
fan-out does not become accidental network broadcast/transit.

The local-client classification regression now invokes
`Transport.is_local_client_interface` from the pinned Python checkout and
compares its parent, attached-child, ordinary-parent, and ordinary-child
results with Rust. This confirms the classification predicate only; it does
not claim that the broader shared-instance acceptance is complete. The pinned
Python source sets a local-client announce deadline to `now`, sets retries to
`PATHFINDER_R` (1), and checks announce deadlines every 1.0 s from its 0.25 s
jobs loop. Its deadline comparison is strict (`time.time() > deadline`), so
equality does not retransmit. The source-derived ideal polling bound is the
check interval plus one jobs poll: 1.25 s, with no runtime-jitter allowance.
The pinned differential executes the announce-check branch extracted from
`Transport.jobs()` with a controlled Python clock. It observes zero sends just
before and exactly at the deadline, one just after, and zero on the later
check (`[0, 0, 1, 0]`). The fake Python packet also asserts the exact announce
destination hash, originating local-client `attached_interface`, and transport
ID. Rust's deterministic production-worker-tick path observes the same packet
counts and asserts the broadcast target, destination, payload, transport ID,
and transport propagation type; its 1.0 s worker interval is strictly inside
Python's source-derived 1.25 s check-plus-poll bound. This is source-derived,
fake-clock schedule evidence, not a live socket timing measurement or a claim
about OS/runtime scheduling jitter.

The focused parent-interface differential now also traverses Rust's production
`handle_announce` path with forwarding disabled. It compares Python's exact
`Transport.is_local_client_interface` predicate against observable announce
queue/cache outcomes for an ordinary interface, a shared-instance owner, an
accepted child, a virtual child, and an ordinary child. Pinned Python returns
`false,false,true,true,false`; production Rust queues only the two children of
the shared owner and caches the other three. No production correction was
needed: the existing Rust predicate checks the receiving interface's parent
and that parent's shared-instance marker. The channel handles stay live for
the duration of the trace so transport cleanup cannot remove an accepted
child between setup and ingress. This verifies this specific classifier only;
it does not complete #609's broader routing and recovery acceptance.

The uncovered shared-child-to-remote-destination LinkRequest cell now matches
the pinned inbound policy. At Reticulum
`99de23c040d507e3fefca19e87b182302902725d`, `Transport._inbound` admits routing
when `transport_enabled or from_local_client`; for an addressed Type-2 packet
whose transport ID is this instance, its known-path branch strips the transport
header at a one-hop destination and transmits on the path table's received
interface. Thus a local child remains able to use the shared parent's known
remote route even when the parent's ordinary transit-forwarding policy is
disabled. Rust's production LinkRequest handler now preserves that exception
only for an interface classified as a local child, while ordinary ingress
still obeys the disabled policy. Two deterministic regressions exercise both
policy values with a remote known route: each asserts exactly one Type-1
LinkRequest on the selected next-hop interface, and no transmission on the
parent channel to sibling local clients. This does not cover other packet
classes, public-network behavior, or the remaining #609 acceptance matrix.

Rust's production tick and announce-table drain now accept an explicit
monotonic `Instant`. The deterministic worker-tick regression schedules the
prior tick just before the immediate deadline, verifies no send, then drives
the first 1.0 s tick after due and verifies exactly one transport broadcast to
the receiving local-client interface. A later tick emits nothing and leaves
the entry only in the bounded cache. This also covers the passive shared
instance path: the retransmit worker now drains local-client announcements even
when transport forwarding is disabled.

The differential executes the exact announce-check branch extracted from the
pinned `Transport.jobs()` source, not the full process-global jobs loop. This
keeps the schedule deterministic while exercising the reference branch's
deadline comparator, retry counter, and one-retransmit completion rule without
wall-clock sleeps. It does not claim a full live Python/Rust daemon schedule
trace or account for runtime scheduling jitter.

## Local software evidence

- `cargo test -p reticulum-rs-transport --lib` — 823 passed, 2 ignored.
- `cargo test -p reticulumd --bin reticulumd` — 465 passed.
- Focused regressions cover the production announce-ingress parent predicate
  across ordinary, shared-owner, accepted-child, virtual-child, and ordinary-
  child cases; passive shared-client retransmission on the first controlled
  worker tick; exact one-send routing; sibling direct fan-out; passive
  transport admission; and announce-table response/cache behavior.
- `cargo test -p reticulum-rs-transport --lib announce_ingress_uses_parent_classification_for_ordinary_owner_and_children` — 1 passed; production announce outcomes match the pinned predicate's five expected classifications.
- `RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c LXMF_PYTHON_BIN=python3 cargo test -p reticulum-rs-transport --lib pinned_python_parent_predicate_matches_production_announce_ingress -- --ignored --nocapture` — 1 passed against Python Reticulum `99de23c040d507e3fefca19e87b182302902725d`.
- `cargo test -p reticulum-rs-transport --lib local_client_announce_retransmits_on_first_worker_tick_once` — 1 passed.
- `cargo test -p reticulum-rs-transport --lib routes_remote_link_request -- --nocapture` — 2 passed; shared local children route a known remote LinkRequest to the next hop with transport policy enabled and disabled, with no sibling-client transmission.
- `RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 cargo test -p reticulum-rs-transport --lib pinned_python_local_client_schedule_matches_production_worker_tick -- --ignored --nocapture` — 1 passed against Python Reticulum `99de23c040d507e3fefca19e87b182302902725d`; the extracted `Transport.jobs()` branch and Rust production tick agree on `[0, 0, 1, 0]` sends before/equal/after/later. The Verify workflow repeats this command with `RETICULUM_PY_REPO` explicitly set to `${{ github.workspace }}/Reticulum-parity` and checks that checkout's HEAD equals `PYTHON_RETICULUM_PARITY_REF` before running it, rather than using the job-default 1.5.2 checkout. This is deterministic source/fake-clock evidence, not live socket timing.
- `RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 cargo test -p reticulum-rs-transport --lib pinned_python_local_client_classification_matches_parent_relationship -- --ignored --nocapture` — 1 passed against Python Reticulum `99de23c040d507e3fefca19e87b182302902725d`.
- `cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings` — passed.
- `tools/scripts/check-module-size.sh` and
  `tools/scripts/check-boundaries.sh` — passed.
- `RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LOG_DIR=target/interop/local-interface-python-shared-605 REPORT_PATH=target/interop/local-interface-python-shared-605/report.json TIMEOUT_SECS=45 bash tools/scripts/local-interface-python-shared-smoke.sh` — passed.
  - candidate: `92ce5720` (`codex/issue-605-parity`)
  - Python Reticulum: `99de23c040d507e3fefca19e87b182302902725d`
  - TCP and Linux abstract Unix `LocalClientInterface` rows reported
    `startup_status = attached`.
  - Both Python shared instances observed two live local clients with
    non-zero receive/transmit counters; each pinned Python traffic client
    connected and emitted three announces.
  - raw report: `target/interop/local-interface-python-shared-605/report.json`

## Multi-hop Python Channel and Resource traces

The ignored Python interop suite now includes
`python_to_python_channel_roundtrip_through_rust_transport`. It starts two
independent pinned-Python Reticulum processes on separate Rust `TcpServer`
interfaces owned by one Rust `Transport` with forwarding explicitly enabled.
The client path request crosses the Rust transport, the Python endpoint link
establishes through that route, and a Channel message plus its reply complete
end to end.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_channel_roundtrip_through_rust_transport -- --ignored --nocapture
# 1 passed; 30 filtered out; 1.63s
```

The trace also exercised the existing Python process helper with a workspace-
relative checkout path after the helper resolved relative `RETICULUM_PY_REPO`
values against the workspace root. This is a software multi-hop Channel trace,
not a shared-instance restart, Resource fault-injection, physical-carrier, or
public-network acceptance result.

A follow-on trace closes that first application link and establishes a fresh
Python `RNS.Link` over the same two Rust carriers before sending a second
Channel message. The client requires the second reply, and the endpoint log
records exactly one delivery for each message, so the reconnect is observed at
the application-link boundary rather than inferred from client completion.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_channel_reconnect_through_rust_transport -- --ignored --nocapture
# 1 passed; 42 filtered out; 2.03s
```

This covers link close/reconnect while the Rust forwarding transport remains
active. It does not cover reconnecting the underlying TCP carrier stream or
broader packet/proof duplicate handling; the separate persistence regression
below covers one scheduled-to-cached restart transition.

A separate fault-injected trace lets a pinned Python endpoint announce and
populate the Rust path table, then drops every outbound initial link-request
packet. The Rust link has a test-bounded three-second establishment deadline;
the trace requires the production link event and status to reach `Closed`.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  pinned_python_link_establishment_timeout_after_dropped_request -- --ignored --nocapture
# 1 passed; 43 filtered out; 4.11s
```

This proves pending link-establishment cleanup after a real mixed-peer path is
available. It does not yet prove caller-visible timeout-reason taxonomy,
underlying carrier-stream reconnect, or broader packet/proof duplicate
handling.

A companion trace routes the same Python Channel exchange through two Rust
carriers while a real TCP proxy duplicates the first decoded Channel frame
from the Python client. The endpoint log records exactly one logical
`python-1` delivery and the client receives its reply, proving Channel-level
sequence deduplication across the forwarding path rather than only transport
packet delivery.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_channel_duplicate_through_rust_transport -- --ignored --nocapture
# 1 passed; 41 filtered out; 1.87s
```

The same forwarding topology now carries a split Resource between two
independent pinned-Python nodes. The Python client waits for the remote
endpoint callback to report the exact `resource-sha256:{size}:{digest}` value,
so sender completion alone cannot make this trace pass.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_resource_roundtrip_through_rust_transport -- --ignored --nocapture
# 1 passed; 34 filtered out; 2.15s
```

## Shared-boundary receive accounting and application trace

The pinned Python `RNS/Transport.py::_inbound` path removes the receive-side
hop added at a local shared-instance boundary before it classifies a packet
for routing. The Rust prequeue path previously performed the increment but
not the matching subtraction. The first real application trace exposed that
discrepancy: Rust attached and learned the Python announce, but its outbound
LXMF link request timed out because it retained a foreign transport header;
the Python shared owner observed no link. Commit `8ae20c54` mirrors the
reference rule: an attached client removes one hop when no local child
interfaces exist, while a shared parent removes a hop only for an actual
local-client child.

- `cargo test -p reticulum-rs-transport --lib shared_instance_ -- --nocapture` — 9 passed, including the new attached-client and parent/child receive-hop regressions.
- `cargo test -p reticulum-rs-transport --lib` — 801 passed.
- `cargo test -p lxmf-cli --bin lxmd` — 14 passed.
- `RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PY_REPO=.tmp/python-refs/LXMF LXMF_PYTHON_BIN=python3 cargo test -p lxmf-cli --test python_lxmd_remote_relay python_shared_instance_rust_lxmd_application_and_restart_e2e -- --ignored --nocapture` — passed against Reticulum `99de23c040d507e3fefca19e87b182302902725d` and LXMF `727830cefda83d9c6e3982b48675425f3f988f9c`.
  - Rust attached as a `local_client` to the Python-owned TCP shared instance and exchanged LXMF messages in both directions before restart.
  - Rust restarted from the same state, reattached, preserved the delivery destination identity, and exchanged both directions again; Python reported the delivery link active.
  - This is a direct one-hop/application/restart trace, not multi-hop or physical-network acceptance.

## Current pinned-Python refresh

The following runs were repeated at exact checkout
`099227cce9c7d6bd55f66acf88516b9293a7e1a1`. The source under test is
unchanged from `6e5b1a4865594432a7fd0405fbaf800e71481cc1`; the intervening
commit only refreshes parity evidence. Process tests were run serially because
the pinned Reticulum helpers share a local-instance socket.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop python_to_python \
  -- --ignored --nocapture --test-threads=1
# 4 passed; 0 failed; 0 ignored; 0 measured; 40 filtered out; 7.74s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PY_REPO=.tmp/python-refs/LXMF \
  LXMF_PYTHON_BIN=python3 cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_shared_instance_rust_lxmd_application_and_restart_e2e \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; 4.90s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop pinned_python_link_ \
  -- --ignored --nocapture --test-threads=1
# 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; 19.51s
```

The four current `python_to_python` traces cover Channel round trip,
application-link reconnect, Channel duplicate suppression, and a split Resource
through two Rust carriers. The shared-instance trace again covers bidirectional
application traffic across Rust daemon restart with stable delivery identity.
The timeout pair covers both dropped link establishment requests and dropped
keepalives reaching terminal `Closed` state. A separate transport restart test
covers a newer cached path response superseding scheduled announce state.
The real-socket carrier reconnect regression below verifies packet resumption.
The two-peer shared-instance daemon-replacement trace now also proves one
faulted opportunistic LXMF attempt is retried through the replacement Rust
relay and delivered exactly once. Broader packet/proof duplicate handling,
Python `LXMRouter` process-restart persistence, and direct/resource retry modes
remain outside that result.

## Two-peer shared-instance recovery after Rust daemon replacement

`python_shared_instance_two_peer_relay_recovers_after_daemon_restart_e2e`
starts two independent pinned-Python shared-instance peers and attaches both as
local clients to one transport-enabled Rust relay. LXMF messages pass in both
directions before the fault. The Python sender arms a one-shot software fault
that drops the next LXMF DATA/NONE packet for peer B and submits an
opportunistic message through its real pinned `LXMRouter` queue. The test waits
until the packet has been dropped and verifies the message remains nonterminal
after exactly one delivery attempt. It then replaces the Rust daemon while the
Python endpoints and queued message remain alive. After both peers re-announce
and the replacement relay relearns their paths, the fault harness observes an
LXMF packet for that destination pass the sender's normal Reticulum outbound
path; peer B receives exactly one copy and peer A observes terminal
`DELIVERED` with at least two attempts. The normal pinned retry code and
schedule are unchanged. Fresh Python RNS links and raw link packets then pass
in both directions.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/LXMF LXMF_PYTHON_BIN=python3 \
  cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_shared_instance_two_peer_relay_recovers_after_daemon_restart_e2e \
  -- --ignored --nocapture --test-threads=1
# 2 consecutive passes; each passed with 0 failures (5.79s and 5.80s)
```

Verified on Rust test base `77edf207627909f416783efb41eca6039d46f9ab`,
Reticulum `99de23c040d507e3fefca19e87b182302902725d`, and LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c`. This proves a bounded
shared-instance opportunistic LXMF retry through one replaced Rust relay. It
does not prove persistence across a Python LXMRouter process restart,
direct/resource retry modes, deeper multi-relay replacement, or broader
packet/proof duplicate classes; the #609 row stays partial.

## Python LXMRouter delivered-ID cache across process restart

Pinned LXMF `LXMRouter.__init__` loads `local_deliveries` into
`locally_delivered_transient_ids`, `has_message()` consults that cache, and
`exit_handler()` flushes it. This is a delivered-ID duplicate-suppression
contract, not persistence of `pending_outbound` (which is initialized as an
empty in-memory list on each process start). The focused multi-process test
keeps one Rust `lxmd`/transport process alive while a Python endpoint owner is
gracefully stopped through `LXMRouter.exit_handler()` and restarted with the
same delivery identity, RNS config, and storage directory. It confirms the
first message's exact transient ID remains a `has_message()` hit after restart,
then proves fresh production-path Rust→Python delivery reaches Rust's terminal
`delivered` receipt with one Python callback; a Python→Rust reply also succeeds.
The test does not replay the old network packet, so packet/proof duplicate
handling across restart remains unclaimed.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/LXMF LXMF_PYTHON_BIN=python3 \
  cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_lxmf_router_delivery_cache_survives_process_restart_e2e \
  -- --ignored --nocapture --test-threads=1
# Two isolated passes; latest: 1 passed, 0 failed, 10 filtered out; 7.59s
```

Verified against LXMF `727830cefda83d9c6e3982b48675425f3f988f9c` and Reticulum
`99de23c040d507e3fefca19e87b182302902725d`. This closes only the supported
delivered-ID cache restore and post-restart traffic slice; queued Python
outbound persistence is not a reference contract, and broader #609 transport,
retry, duplicate-packet, and multi-relay acceptance remains open.

## Caller-visible close-reason parity

The pinned Python reference exposes `Link.teardown_reason` to its closed-link
callback, using `TIMEOUT = 0x01`, `INITIATOR_CLOSED = 0x02`, and
`DESTINATION_CLOSED = 0x03`. Rust previously published only `LinkEvent::Closed`.
`LinkCloseReason` now carries those values on `LinkEventData`, with role-aware
local/remote close classification and explicit timeout classification for
watchdog and establishment expiry.

API compatibility note: adding the public `close_reason` field to
`LinkEventData` is source-breaking for downstream struct literals and
exhaustive destructuring. Callers constructing this type must provide
`close_reason`; consumers that do not need the reason can use `None`. This is
an intentional API change to make the Python callback-visible behavior
available to Rust callers.

```text
cargo test -p reticulum-rs-transport --lib destination::link
  59 passed; 0 failed; includes callback-visible reason code assertions
cargo test -p reticulum-rs-transport --lib \
  a_pending_out_link_that_outlives_its_establishment_timeout
  1 passed; 0 failed; production maintenance emits Timeout (0x01)
cargo test -p reticulum-rs-transport --lib \
  channel_retry_exhaustion_sends_link_close_and_fails_pending_messages
  1 passed; 0 failed; timeout returns LinkClose and paired peer Link closes
```

The pinned Python `RNS/Channel.py::_packet_timeout` shuts down the channel and
calls the link outlet's `timed_out()`, which reaches `Link.teardown()` and sends
a LinkClose packet for an active link. The Rust retry-exhaustion regression
verifies that retry exhaustion returns one `PacketContext::LinkClose`, the
paired peer Link accepts it and closes, both endpoints emit one caller-visible
close event with the initiator role code, and repeated `close()` is idempotent.
The production maintenance loop forwards the returned packet through the Link's
ingress interface; a pinned-Python retry-exhaustion trace over a live carrier is
not part of this regression.

This closes the local event and LinkClose packet slice without promoting the
broader #609 row. The later two-peer trace below covers bounded shared-
instance path/link/raw-packet recovery after daemon replacement; LXMF queue
retry, deeper relay replacement, broader packet/proof duplicate handling, and
the wider transport matrix remain unverified.

## Pinned-Python initiated close reason over TCP

On source commit `93c3dc39`, the ignored Python/Rust integration test
`python_initiated_link_close_reports_initiator_reason_to_rust` starts the
production Rust TCP listener, establishes an outbound Python link, and invokes
the pinned reference's `Link.teardown()`. The Python process remains alive on a
stdin gate until Rust observes the production inbound-link event, avoiding a
sleep-based packet-flush assumption. Rust reports
`LinkCloseReason::InitiatorClosed` (`0x02`), matching Python's
`Link.INITIATOR_CLOSED`; after the test releases the process, Python confirms
its own teardown reason is `2`.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  python_initiated_link_close_reports_initiator_reason_to_rust -- \
  --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; 0.78s
```

Reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`. This verifies
an explicit clean close from the Python initiator only; it does not verify
Python Channel retry-exhaustion over a live carrier. Broader #609 routing,
retry, duplicate packet/proof, and shared-instance recovery cases remain open.

## Pinned-Python Channel retry exhaustion over TCP

On source commit `5df915c3`,
`python_channel_retry_exhaustion_sends_link_close_over_tcp` opens the production
Rust inbound Channel and routes the Python peer through the test TCP carrier
proxy. The proxy drops ordinary Link delivery proofs (`PacketType::Proof` with
context `None`) while preserving Link establishment proofs and other traffic.
The pinned Python Channel sends one message, exhausts its default five-attempt
budget, and tears down the initiating Link. The test verifies that at least one
delivery proof was dropped, Python reports exactly five attempts and close
reason `2`, and the Rust caller receives one closed-link event with
`InitiatorClosed`.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  python_channel_retry_exhaustion_sends_link_close_over_tcp -- \
  --ignored --nocapture --test-threads=1
# 2 consecutive runs; each 1 passed in 5.33s
```

Reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`. This is
localhost software-carrier evidence, not physical-radio, public-network, or
broader relay-recovery evidence; other #609 routing, retry, duplicate, and
shared-instance gaps remain open.

## Cached-versus-scheduled announce persistence

`newer_cached_path_announce_survives_scheduled_queue_restart` exercises the
production announce, path-table save, and restore APIs. It first accepts an
ordinary scheduled announce, then a strictly newer `PATH_RESPONSE` announce
for the same destination. After save/restart/restore, the path remains
reachable, the restored bounded cache contains the newer accepted payload,
and no retransmission-queue entry is recreated. The test observes the whole
state transition rather than only the announce table's in-memory lookup.

```text
cargo test -p reticulum-rs-transport --lib \
  newer_cached_path_announce_survives_scheduled_queue_restart -- --nocapture
# 1 passed; 0 failed; 1.13s
```

This closes that specific local persistence scenario; it is not a Python↔Rust
restart trace, a carrier-stream reconnect result, or broad duplicate-proof
coverage.

## Expired persisted route restart and recovery

`expired_persisted_route_is_not_reused_before_fresh_announce_recovery` follows
a learned remote route through production save, deterministic expiry, and
replacement transport restore. The replacement uses the same
`restore_reticulum_path_table_report` method wired by `reticulumd` bootstrap.
Before any new announce, the expired path is absent and its cached destination
identity is unavailable. A fresh announce for the same destination then passes
through production `handle_announce` and restores both route availability and
identity. The test expires both the persisted timestamp and deadline, avoiding
wall-clock sleeps.

```text
cargo test -p reticulum-rs-transport --lib \
  expired_persisted_route_is_not_reused_before_fresh_announce_recovery -- --nocapture
# 1 passed
```

At pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`, startup loads
active path rows and the jobs loop culls stale paths using the persisted
timestamp plus the interface-specific timeout; fresh announces replace an
existing route after its expiry. The Rust scenario reaches the same final
expired then recovered state and additionally asserts the route is unavailable
immediately after replacement. Python's transient visibility before its next
table-cull job was not measured. This does not claim full daemon-process
restart timing or close the broader #609 row. No production behavior change
was needed.

## Underlying TCP carrier reconnect

The real-socket integration test `tcp_carrier_reconnect_resumes_bidirectional_packet_traffic_on_same_iface`
accepts and drops the first TCP stream, observes the production reconnect event
with the original interface identity, then sends a framed packet from the
manager over the replacement stream and another packet back through the
manager receive channel. This verifies bidirectional packet traffic resumes
after carrier redial, not merely that a socket reconnect event was emitted.

```text
cargo test -p reticulum-rs-transport --test tcp_client_reconnect -- --nocapture
# 1 passed; 0 failed; repeated five times; bidirectional HDLC traffic resumed
```

This is carrier-level software evidence. The bounded shared-instance relay
restart trace above separately verifies path, link, and raw packet recovery;
broader packet/proof duplicate handling and LXMF queue retry across daemon
replacement remain open.

## Multi-hop link-request-proof duplicate suppression

`python_to_python_duplicate_link_request_proof_is_filtered_by_rust_transport`
starts two pinned Python nodes on separate TCP carriers owned by one forwarding
Rust transport. A real TCP proxy duplicates the endpoint's first
`LinkRequestProof` before it reaches Rust and confirms that the same proof is
observed exactly once on the client-facing carrier. The client still completes
the link and the endpoint observes exactly one Channel delivery. This checks
the production duplicate filter and both carrier directions, not merely the
application-level Channel sequence guard.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_duplicate_link_request_proof_is_filtered_by_rust_transport \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; injected duplicate count 1; forwarded proof count 1
```

The same pinned-reference test is now part of the PR `Verify` workflow. This
closes only the link-request-proof duplicate case; other packet/proof classes
remain unverified.

## Shared-instance duplicate delegation and LinkRequest replay filtering

At the frozen Python target `99de23c040d507e3fefca19e87b182302902725d`,
`RNS/Transport.py::packet_filter` accepts packets unconditionally when this
node is attached to a shared instance; the shared owner performs duplicate
filtering. For a standalone transport, an exact repeated ordinary LinkRequest
is rejected by the packet-hash list. Rust now preserves both behaviors: it
continues packet-cache bookkeeping for receipt/routing context while attached
clients do not reject duplicates locally, and standalone duplicate
LinkRequests are filtered. The test-only filter helper delegates to the same
production policy.

Focused unit regressions cover both decisions:

```text
cargo test -p reticulum-rs-transport --lib duplicate_filter -- --nocapture
# 4 passed; includes shared-instance duplicate delegation and LinkRequest replay suppression
```

`python_to_python_duplicate_link_request_is_filtered_by_rust_transport`
starts two independent Python nodes on separate carriers through the Rust
forwarder. A TCP fault proxy duplicates the client's first LinkRequest; the
endpoint-facing proxy observes exactly one forwarded request, and the client
still completes the Channel exchange. It passed against the exact frozen
Reticulum target:

```text
RETICULUM_PY_REPO=Reticulum-parity LXMF_PYTHON_BIN=python \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_duplicate_link_request_is_filtered_by_rust_transport \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; one injected request, one forwarded request
```

The same command is now a required step in PR `Verify`. Other packet/proof
classes and LXMF queue retry after daemon replacement remain open; #609 stays
partial.

## Remaining acceptance boundary

### Stale route expiry and cached announce restore

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`,
`RNS/Transport.py::jobloop`, removes a destination path only when
`time.time() > timestamp + mode_timeout`. The deterministic
`stale_route_expiry_keeps_exact_deadline_and_removes_after_it` regression
passes an explicit monotonic clock value and checks Full, Access Point, and
Roaming paths at the exact deadline and one nanosecond past it; equality is
retained and strictly stale paths are removed.

Cached-announce restore validity is covered by
`reticulum_path_table_restore_skips_malformed_cached_announce_entry` (valid
and malformed cache entries in one restore), plus the existing missing-cache
and mismatched-destination restore cases. Pinned Python only installs a path
when its cached announce exists and successfully unpacks, and when the
receiving interface is still available. These tests cover the Rust restore
accept/reject behavior; they do not claim parity for all Python cache or
announce semantics.

The pinned startup path also recalls the cached announce identity and refuses
to restore a destination present in `Transport.blackholed_identities`
(`RNS/Transport.py` at the reference SHA above). The production-path
`reticulum_path_table_restore_skips_blackholed_cached_announce_identity`
regression saves a learned route, marks that identity blackholed on the
replacement transport, then restores through
`restore_reticulum_path_table_report`. Before the fix it failed with one route
restored; after the fix it reports `active_blackholed_identity == 1`, restores
zero active routes, and exposes neither the path nor its destination identity.
The filter uses the recovered identity in the cached announce and runs before
installing that cache entry or path.

```text
env TMPDIR=/dev/shm cargo test -p reticulum-rs-transport --lib \
  reticulum_path_table_restore_skips_blackholed_cached_announce_identity -- --nocapture
# 1 passed; 0 failed
```

This is a deterministic Rust production save/restore regression against the
pinned Python startup predicate, not a Python↔Rust daemon process trace. It
closes only this cached-route validity case; the broader #609 acceptance stays
open.

```text
cargo test -p reticulum-rs-transport --lib stale_route_expiry_keeps_exact_deadline_and_removes_after_it
cargo test -p reticulum-rs-transport --lib reticulum_path_table_restore_skips_
```

This closes only the stale-route exact-time boundary and the cited cached
announce restore cases; #609 remains partial.

## Remaining acceptance boundary

The combined evidence now proves pinned Python↔Rust local attachment, announce
fan-out, a direct application/link exchange, Rust daemon restart with identity
continuity, two-carrier multi-hop Python Channel and split Resource exchanges,
and multi-hop Channel sequence deduplication through one forwarding Rust
transport, application-link close/reconnect, one link-request-proof duplicate,
and ordinary LinkRequest duplicate suppression through that forwarding path.
Attached shared-instance mode also now accepts duplicate data for owner-side
filtering, matching the pinned reference. The two-peer shared-instance trace
also verifies path relearning, fresh links, and raw packets in both directions
after replacing the Rust daemon. It does not cover post-restart LXMF queue
retry/delivery, deeper multi-relay replacement, or broader packet/proof
duplicate classes. Those cases and the remaining transport matrix are still
required before this row can be promoted; scheduled-to-cached announce
persistence and direct carrier redial are covered by the focused transport
tests above.
Hardware and public-network evidence remain separate acceptance axes.

## Locally hosted destination is not learned or fanned out

`locally_hosted_announce_is_not_learned_or_fanned_out` creates a locally hosted
destination on a transport-enabled daemon, then feeds its valid announce back
through one shared-instance child while a sibling client is attached. The
daemon does not install a remote path, queue the announce for retransmission,
or transmit it to the sibling. This covers one locally-hosted/attached-client
combination; it does not prove the full transport-enabled/disabled and
locally-hosted/remote-destination matrix in #609.

```text
cargo test -p reticulum-rs-transport --lib locally_hosted_announce_is_not_learned_or_fanned_out -- --nocapture
# 1 passed; 0 failed
```

## Transport-disabled local LinkRequest from a shared child

`disabled_shared_daemon_delivers_local_link_request_only_to_requesting_child`
drives a LinkRequest through Rust's production inbound admission and packet
processing path. The daemon has transport disabled, the ingress is one virtual
child of a shared-instance parent, the destination is hosted locally, and a
sibling child is attached. The test observes exactly one outbound packet: a
`LinkRequestProof` addressed to the request's derived link ID and directed to
the requesting child. The destination records the inbound link, and the shared
parent's transmit queue has no second packet, so the proof is not broadcast or
transit-forwarded to the sibling.

The frozen Python reference at Reticulum
`99de23c040d507e3fefca19e87b182302902725d` has the matching control flow:
`Transport._inbound` marks a packet from `local_client_interfaces` as
`from_local_client` and admits it to general handling even when transport is
disabled (`RNS/Transport.py` lines 1965-1997). Its local LinkRequest branch
matches the destination and calls `destination.receive(packet)` rather than
selecting a remote route (lines 2540-2567). `Destination.receive` dispatches to
`incoming_link_request` (`RNS/Destination.py` lines 414-435), and
`Link.validate_request` binds the link to the packet's receiving interface
before sending its proof (`RNS/Link.py` lines 186-218). The outbound path
filters link packets to that attached interface (`RNS/Transport.py` lines
1439-1454). This is a source-level reference comparison, not a live mixed
Python/Rust socket trace.

```text
cargo test -p reticulum-rs-transport --lib \
  disabled_shared_daemon_delivers_local_link_request_only_to_requesting_child -- --nocapture
# 1 passed; 0 failed
```

This proves one transport-disabled/shared-child/locally-hosted software matrix
cell. It does not establish the wider transport matrix or promote issue #609.

## Transport-enabled local LinkRequest from a shared child

`enabled_shared_daemon_delivers_local_link_request_only_to_requesting_child`
repeats the production-path scenario with transit forwarding enabled. It
asserts exactly one outbound packet, a `LinkRequestProof` directed to the
requesting child, for the derived link ID, with no transport header. The local
destination accepts the link and no second packet is emitted for the sibling;
the delivery/forwarding counts are therefore one local response and zero
transit copies.

The frozen Python source takes the same local-destination branch independent
of transit policy: `Transport._inbound` calls `destination.receive(packet)` for
a matching local LinkRequest (lines 2540-2567), and `Link.validate_request`
binds the new link to `packet.receiving_interface` before generating its
proof (lines 186-218). This is a source-level comparison, not a live Python
socket differential. Together with the preceding disabled case, it verifies
both forwarding-policy settings for this one shared-child/locally-hosted
combination; issue #609's broader matrix remains open.

```text
cargo test -p reticulum-rs-transport --lib \
  enabled_shared_daemon_delivers_local_link_request_only_to_requesting_child -- --nocapture
# 1 passed; 0 failed
```

Both transport-mode regressions were rerun at PR #629 head
`8bff93caa992cf3694e4b3a8f1dd4597651d5754`; each passed (1 test, 0 failed).
The broader #609 acceptance row remains open.

## Transport-disabled locally hosted announce from a shared child

`disabled_shared_daemon_does_not_transit_announce_for_local_destination`
drives a valid announce for a destination hosted on the shared daemon through
the production inbound admission and packet processing path. The ingress is a
virtual local client and a sibling is attached; transport forwarding is
disabled. The announce is accepted for handling but creates neither a remote
path nor a queued/cached retransmission, and the parent emits no packet to the
sibling.

The frozen Python reference at Reticulum
`99de23c040d507e3fefca19e87b182302902725d` takes the matching local-destination
branch in `RNS/Transport.py`: announce path learning and retransmission are
inside `if local_destination == None and announce_valid`, so a valid announce
for a hosted destination is not treated as transit traffic. Rust matches this
behavior without a production change.

```text
cargo test -p reticulum-rs-transport --lib \
  disabled_shared_daemon_does_not_transit_announce_for_local_destination -- --nocapture
# 1 passed; 0 failed
```

This proves the transport-disabled shared-child announce cell for a locally
hosted destination. Combined with the existing enabled announce and enabled /
disabled LinkRequest cells, it adds one point to the local-delivery matrix;
other packet classes and the remaining #609 acceptance gates remain open.

The enabled counterpart is now also exercised through production inbound
admission (`preprocess_inbound_message` followed by
`process_inbound_message`), rather than only calling `handle_announce`
directly. `enabled_shared_daemon_does_not_transit_announce_for_local_destination`
asserts no remote path, no queued/cached retransmission, and no sibling output
for a valid announce from a shared virtual child when transit forwarding is
enabled. This matches the pinned Python `Transport._inbound` local-destination
branch: path learning/retransmission occurs only when no local destination
matches. It complements the transport-disabled production-path announce test;
it remains one matrix cell and does not promote #609.

```text
cargo test -p reticulum-rs-transport --lib \
  enabled_shared_daemon_does_not_transit_announce_for_local_destination -- --nocapture
# 1 passed; 0 failed
```

## Ordinary proof duplicate filtering

`rns_1_5_ingress_suppresses_duplicate_ordinary_proof` submits an ordinary
single-destination `Proof` (`context=None`) twice through production inbound
preprocessing: the first copy is admitted, and the exact replay is rejected by
the packet-hash cache. This is distinct from LinkRequestProof routing,
Channel-sequence deduplication, and Resource-fragment tracking.

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d` uses the generic
packet-hash-list check for this proof class and remembers the hash after
admission. The ignored Rust/Python differential passed against the exact pinned
checkout: the Python filter and Rust production-ingress path both observed
`[true, false]` for first-copy admission and exact-replay rejection.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PYTHON_BIN=python3 cargo test -p reticulum-rs-transport --lib \
  pinned_python_ordinary_proof_filter_matches_production_ingress \
  -- --ignored --nocapture --test-threads=1
# 1 passed; reference HEAD 99de23c040d507e3fefca19e87b182302902725d
```

No production mismatch was found; other duplicate classes and the broad #609
acceptance matrix remain open.

## Per-interface paced announce queue capacity

The production interface manager now uses the pinned 1.5.4 limit of 4,096
queued paced announces per interface. The ignored differential
`paced_announce_queue_matches_pinned_python_capacity` reads the exact pinned
`RNS/Reticulum.py` source at
`99de23c040d507e3fefca19e87b182302902725d`, verifies the capacity, accepts the
4,096th distinct queued announce, and rejects the next one while preserving
the first transmission.

```text
RETICULUM_PY_REPO=/path/to/Reticulum-99de23c \
LXMF_PYTHON_BIN=python3 cargo test -p reticulum-rs-transport --lib \
  paced_announce_queue_matches_pinned_python_capacity -- \
  --ignored --nocapture --test-threads=1
# 1 passed; 0 failed
```

This verifies the paced-queue capacity boundary only; other announcement,
retry, duplicate, transport-recovery, and broader #609 acceptance remain
separate.

## Mixed Python/Rust multi-hop recovery after relay restart

`python_shared_instance_two_rust_relays_recover_after_upstream_restart_e2e`
uses Python shared-instance endpoint A, two Rust transport relays in series,
and a Python endpoint B behind the second relay. Both relays run with transport
enabled. The test confirms bidirectional paths through the chain, stops the
upstream Rust relay, clears only that relay's Reticulum route database while
preserving its identity/configuration, and restarts it. It verifies the
restarted relay has no restored active paths, its shared-instance client is
attached, the downstream relay's TCP client has reconnected, and both Python
endpoints explicitly request and rediscover the remote path. Fresh bidirectional
LXMF messages are then delivered end-to-end over the recovered multi-hop path.
Each sender also waits for the corresponding outbound message to reach the
`delivered` state, verifying delivery-proof processing rather than only
receiver-side content. Each direction then opens a fresh raw Reticulum Link over
the recovered path, exchanges application bytes, and asserts both peer-side
Link states are active.

The same recovery test now sends fresh production Resources in both directions
over recovered Links: 131,101 bytes from endpoint A to B and 98,317 bytes from
B to A. Each receiving Python process verifies the exact size, SHA-256 digest,
and metadata; each sender waits for its production `RNS.Resource` callback and
requires `Resource.COMPLETE`. These are fresh post-restart Resource transfers,
not retries of a Resource interrupted while in flight.

The two-peer shared-instance restart scenario verifies one queued OPPORTUNISTIC
LXMF message after relay recovery. A separate ignored integration test,
`python_direct_resource_retry_after_upstream_restart_e2e`, covers the stronger
DIRECT Resource case without coupling it to the broader, intermittently failing
B-to-A message matrix. Its test-only Python gate records the first outbound
Resource and Link IDs, status, sent-part count, and total parts after the first
fragment batch, then blocks later requests for that Resource. The test stops
and replaces the upstream Rust relay with its route database cleared, releases
the gate, rediscovers paths, and requires the same LXMF message to be delivered
exactly once using a different Resource and Link.

A focused ignored production-path regression,
`python_shared_instance_queued_direct_recovers_after_upstream_restart_e2e`, now
isolates queued DIRECT message recovery from the flaky bidirectional mixed-shape
scenario. It uses the same Python shared-instance sender, two separate Rust
relay processes, and Python receiver; the sender first caches the receiver
identity, then queues a DIRECT message while upstream relay A is stopped. The
test clears relay A's Reticulum route database, restarts it, verifies no active
routes were restored, waits for the downstream TCP client to reconnect, and
explicitly relearns both Python routes. It requires the outbound message's
terminal state to be `delivered` and exactly one receiver inbox entry with the
exact payload.

```text
TMPDIR=/dev/shm \
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/LXMF \
LXMF_PYTHON_BIN=python3 cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_shared_instance_queued_direct_recovers_after_upstream_restart_e2e \
  -- --ignored --exact --nocapture --test-threads=1
# Reticulum 99de23c040d507e3fefca19e87b182302902725d
# LXMF 727830cefda83d9c6e3982b48675425f3f988f9c
# 2 consecutive runs passed; 0 failed (24.94s, 24.92s)
```

`TMPDIR=/dev/shm` is required in this environment because the default temporary
directory exceeded its per-user quota before the fixture could start. This
isolated pass establishes only queued DIRECT delivery after route-cleared
upstream restart; it does not close the broader #609 transport/recovery matrix.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF \
LXMF_PYTHON_BIN=python3 TMPDIR=/dev/shm \
cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_shared_instance_two_rust_relays_recover_after_upstream_restart_e2e \
  -- --ignored --nocapture --test-threads=1
# pinned Reticulum 99de23c040d507e3fefca19e87b182302902725d
# pinned LXMF 727830cefda83d9c6e3982b48675425f3f988f9c
# 2 earlier consecutive passes including the Resource assertion; each 1 passed; 0 failed
```

The initial regression revision required no production change: the first
experiment reused pre-restart Python LXMF direct Links, so the acceptance now
sends only fresh application traffic after restart. Later follow-up runs
exposed intermittent behavior that the earlier passes did not: one full
invocation failed after about 55 seconds waiting for the reverse B-to-A LXMF
delivery, with B's outbound message still at `outbound`, A's inbox empty, and no accepted inbound
delivery Link at A. A separate diagnostic attempt waited 300 seconds for Rust
relay A to learn Python A's delivery destination and timed out with that route
still unknown. Because Python A hosts that destination locally and the pinned
reference does not require it to appear in the owner's learned path table, the
route-table observation is not a valid standalone discrepancy signal. Receiver
timeout failures capture outbound status, inboxes, relay path/interface state,
and endpoint diagnostics. Several pre-fix invocations passed in about 9.4
seconds, but those passes did not resolve the earlier actual delivery failure;
discrepancy tracking remained open at that point.

This establishes the shared-instance/multi-hop discovery, delivery-proof,
raw-Link traffic, daemon-replacement, one queued OPPORTUNISTIC message,
bidirectional post-restart Resource digest/metadata/completion, and in-flight
large DIRECT LXMF Resource retry slices of #609. The DIRECT retry test requires
a fresh Resource and Link after the interrupted transfer and exactly-once
receiver delivery. It does not close the umbrella issue; deeper relay
replacement, other packet/proof duplicate classes, and remaining transport
matrix behaviors stay open.

The earlier diagnostic treated a missing route-table entry for Python A's
locally hosted delivery destination as evidence that Rust relay A had failed to
learn a remote route. That inference is invalid for the Python shared-instance
owner: pinned Reticulum 1.5.4 `Transport.has_path()` consults only
`path_table`, while `next_hop()` and `next_hop_interface()` separately resolve
destinations in `destinations_map` to the local shared-instance interface.
Consequently a locally hosted destination need not appear as a learned path.
The acceptance now relies on actual post-restart B-to-A delivery and delivered
outbound state (plus the existing relay-B route checks), rather than the
redundant relay-A path-table assertion. The earlier missing-route observation
alone therefore does not establish a transport discrepancy.

A focused Rust regression now covers the narrower relay boundary: with
transport enabled, `connected_to_shared_instance`, an empty path table, and a
distinct outgoing interface marked as the shared-instance client, an unknown
LinkRequest is emitted only on that interface as Type 1. The test also checks
that the upstream MTU is clamped and that the pending link-table entry records
the ingress interface so its matching LinkRequest proof routes back there. A
receive-only shared-instance interface, ordinary interface, and ingress do not
receive the request; an ordinary transport with no shared-instance connection
still drops an unknown request. This is handler/link-table evidence, not a
separate Python-owner delivery result. The transport library suite passed
(835 passed, 5 ignored), scoped Clippy, formatting, module-size, and diff
checks passed.

The pre-fix exact ignored mixed Python/Rust post-restart regression produced
inconsistent outcomes: an earlier replay passed in 18.30 s, a fresh replay
failed after 55.11 s waiting for B-to-A LXMF delivery, and the immediate retry
passed in 9.47 s. On the failure, B had attempted the message at progress 0.03
and had a two-hop cached path to A; relay B knew A at one hop, relay A's path
table did not contain A, and A's inbox remained empty. The missing relay-A
path-table row is expected for a locally hosted destination in the Python
shared-instance owner's `destinations_map`; the absent delivery was not.

The diagnostic trace localized the failure: Python owner A accepted the
fallback LinkRequest and returned a `LinkRequestProof`, but relay A rejected it
with `missing_destination_identity`. Commit
`ce8245bac39fd4c2aad27c837e7a36a69a775b36` records this specific fallback as a
shared-owner handoff. If the relay cannot recall the destination identity, it
forwards the proof only when it arrives on the exact shared-owner interface
recorded for that request; ordinary transit still fails closed without the
destination identity, and known identities continue through signature
validation. Focused regressions also reject a wrong-interface proof and a
LinkRequestProof with a non-Link destination type.

After the fix, the original pinned-Python two-relay post-restart regression
passed three consecutive local runs (26.65 s, 9.42 s, 9.43 s). An earlier
combined shape with queued DIRECT delivery and fresh Resources in both
directions passed once (42.42 s); the final retry acceptance is now a separate
test so its verdict does not depend on the broader reverse-delivery matrix:

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/LXMF \
cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_shared_instance_two_rust_relays_recover_after_upstream_restart_e2e \
  -- --ignored --exact --nocapture
# Reticulum 99de23c040d507e3fefca19e87b182302902725d
# LXMF 727830cefda83d9c6e3982b48675425f3f988f9c
# 3 runs passed; 0 failed (26.65 s, 9.42 s, 9.43 s)
# earlier combined queued-DIRECT plus bidirectional-Resource run: 1 passed (42.42 s)
```

Local validation on this candidate also passed: `cargo test -p
reticulum-rs-transport --lib` (837 passed, 5 ignored), focused proof tests
(29 passed, 1 ignored), scoped Clippy with warnings denied, formatting,
module-size, and diff checks. The production correction addresses the
reproduced identity-less shared-owner proof rejection. Current evidence adds
one queued OPPORTUNISTIC case, bidirectional fresh-Resource transfers, and an
isolated in-flight large DIRECT LXMF Resource retry on a new Link/Resource
after relay replacement. The standalone retry test passed twice locally (32.87
s and 32.91 s). The broad reverse-delivery test still has intermittent B-to-A
timeouts in later reruns, so broad delivery stability is not established.
Deeper relay replacement, other packet/proof duplicate classes, and the wider
#609 transport matrix remain open; issue #609 stays partial.

A fresh bounded replay on the detached #629 head
`bfcd85132a6a6e7cf4c596fc5e5732d3d0e4f561` used the exact pinned Reticulum
`99de23c040d507e3fefca19e87b182302902725d` and LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c` checkouts. Two serial runs again
timed out on post-restart B-to-A delivery (54.92 s and 54.93 s); existing
diagnostics showed two outbound attempts at progress 0.5, an empty A inbox, B's
two-hop cached route, and relay B's one-hop route to A. A third immediate replay
passed in 9.98 s. This confirms the already-known intermittency but does not
localize a new mismatch or establish stable delivery; no production change is
justified by this replay. An initial pair of runs used the non-pinned generic
Reticulum checkout and is excluded from this evidence.

## Exact packet/proof replay after standalone daemon restart

The pinned Reticulum parity target at `99de23c040d507e3fefca19e87b182302902725d`
loads `packet_hashlist.raw` at startup when transport is enabled and the node
is not attached to a shared instance (`RNS/Transport.py:339-351`). It stores
exact packet hashes and `packet_filter()` rejects a replay found in either
current or previous hash sets (`:1668-1679`); graceful/runtime persistence
writes the same raw concatenated-hash format (`save_packet_hashlist`). This
includes ordinary proof packets, unlike the separate LXMF delivered-ID cache
contract documented above.

Rust previously wrote only a MessagePack list to `packet_hashlist` and never
restored that list during daemon bootstrap. The focused fix now writes and
restores the pinned `packet_hashlist.raw` format, continues to read the legacy
Rust MessagePack file, restores the cache only for standalone transport-enabled
nodes, and does not fabricate proof return-route/interface state from hashes.
The production ingress regression accepts one proof, persists the cache,
constructs a fresh transport, restores the raw hash list, then rejects the
exact same proof. The compatibility test reads the prior Rust MessagePack
format, and a shared-instance test confirms attached clients skip owner-managed
hash restoration. This covers exact hash replay only; proof retransmission
classes, cache rotation/size behavior, and the broader #609 restart matrix
remain open.

## Exact proof replay across standalone daemon-process restart

The prior regression restored the hash list into a fresh `Transport` instance
inside one process. The new production-process regression exercises the
shutdown boundary itself: it starts `reticulumd` with isolated storage, sends
the same proof twice through its TCP ingress, and observes one replay-filter
counter hit. After graceful SIGINT shutdown, it verifies the raw persisted
hash list contains the proof hash, starts a second daemon against the same
storage, sends the proof again, and observes the replay-filter counter there.
This matches the pinned Python behavior of loading `packet_hashlist.raw` at
startup and saving packet hashes during graceful persistence. Hash-list and
path-table saves are attempted independently so a failure in one does not
prevent the other; errors are still surfaced. This closes only the exact
ordinary-proof process-restart case; retransmission classes, cache rotation,
and broader #609 acceptance remain open.

## Duplicate ResourceProof admission

At pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`,
`Transport.packet_filter()` returns immediately for `Packet.RESOURCE_PRF`
before consulting either packet-hash generation (`RNS/Transport.py:1630-1640`).
Thus retransmitted resource-proof packets are deliberately admitted to the
resource state machine, where the resource proof itself is validated. Rust's
production `filter_duplicate_packet()` previously recorded the first
`PacketType::Proof` / `PacketContext::ResourceProof` hash and rejected its
identical retransmission; only pending `LinkRequestProof` duplicates were
allowed. The new regression calls the production transport duplicate filter
twice with the same Link ResourceProof and confirms both admissions. Rust now
matches the pinned context exception while retaining hash filtering for
ordinary proofs and other proof contexts. This verifies one retransmission
class only; ordinary proof process-restart evidence, other proof classes,
cache rotation/size behavior, and broader #609 acceptance remain distinct and
open.

```text
cargo test -p reticulum-rs-transport duplicate_resource_proofs_reach_resource_state_machine
  PASS (1 test)
cargo fmt --all -- --check
  PASS
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
  PASS
tools/scripts/check-module-size.sh
  PASS
git diff --check
  PASS
```

```text
cargo test -p reticulumd --bin reticulumd shutdown_flush_attempts_path_table_when_packet_hashlist_persistence_fails -- --nocapture
  PASS (1 test)
cargo test -p reticulumd --test packet_replay_process_restart -- --nocapture --test-threads=1
  PASS (1 test)
cargo test -p reticulumd --tests
  PASS (all reticulumd test targets)
Formatting, strict all-targets Clippy, module-size, and diff checks passed.
```
