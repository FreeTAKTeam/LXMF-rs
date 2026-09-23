# Issue #609: transport, local-client, and shared-instance behavior

Status: implemented but unproven. This is a forward-candidate software slice,
not a claim of mixed-peer or hardware acceptance.

## Reference and scope

- Target Reticulum revision: `99de23c040d507e3fefca19e87b182302902725d`.
- Reference surfaces: `RNS/Transport.py`, `RNS/Reticulum.py`, and
  `RNS/Interfaces/LocalInterface.py`.
- Rust owners: `rns-transport` interface-manager, announce table, and
  announce processing; `reticulumd` local TCP/Unix startup.
- Current mixed-peer evidence candidate: `789774bd` on
  `codex/issue-605-parity`.

The implementation now classifies a local client from its parent relationship
and the parent's shared-instance marker. Active local TCP and Unix listeners
are marked as shared-instance parents; accepted children inherit the runtime
configuration and remain distinguishable from the parent. An announce from a
local client is queued with an immediate timeout and the retry limit already
consumed, producing one immediate transport retransmit before the entry moves
to the bounded cache. Accepted announces are sent directly to sibling local
clients as Type-2 transport announces, excluding the receiving child, so this
fan-out does not become accidental network broadcast/transit.

## Local software evidence

- `cargo test -p reticulum-rs-transport --lib` — 794 passed.
- `cargo test -p reticulumd --bin reticulumd` — 465 passed.
- Focused regressions cover parent/child classification, immediate single
  retransmit, sibling direct fan-out, passive transport admission, and the
  existing announce-table response/cache behavior.
- `cargo clippy -p reticulum-rs-transport --lib --all-features --no-deps -- -D warnings`
  — passed.
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
The two-peer shared-instance daemon-replacement trace below was added afterward;
broader packet/proof duplicate handling and LXMF queue recovery across daemon
replacement remain unverified.

## Two-peer shared-instance recovery after Rust daemon replacement

`python_shared_instance_two_peer_relay_recovers_after_daemon_restart_e2e`
starts two independent pinned-Python shared-instance peers and attaches both as
local clients to one transport-enabled Rust relay. LXMF messages pass in both
directions before restart. The Rust daemon is stopped while both Python peers
remain alive; peer A then queues a short opportunistic LXMF message without
waiting for a path, and the test confirms it has not reached a terminal state
while the relay is down. The Rust daemon is replaced using the same
configuration and state. After both peers re-announce and the relay relearns
their paths, peer B receives that same queued message and peer A observes the
message reach `DELIVERED`. Fresh Python RNS links and raw link packets then pass
in both directions.

```text
RETICULUM_PY_REPO=Reticulum-target-99de23c0 LXMF_PY_REPO=LXMF LXMF_PYTHON_BIN=python3 \
  cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_shared_instance_two_peer_relay_recovers_after_daemon_restart_e2e \
  -- --ignored --nocapture --test-threads=1
# 2 consecutive passes; each passed with 0 failures; under 5s per run
```

Verified on Rust source commit `f99165ccf3542d343646165aaf9697f6e906fc40`,
Reticulum `99de23c040d507e3fefca19e87b182302902725d`, and LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c`. This is bounded shared-instance
path/link/packet recovery through one Rust relay and one short opportunistic
LXMF queue item. It does not prove persistence across a Python LXMRouter
process restart, direct/resource retry modes, deeper multi-relay replacement,
or broader packet/proof duplicate classes; the #609 row stays partial.

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
