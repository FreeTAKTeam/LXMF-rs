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
active. It does not cover reconnecting the underlying TCP carrier stream,
cached-versus-scheduled announce persistence, or broader packet/proof duplicate
handling.

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
underlying carrier-stream reconnect, announce persistence, or broader
packet/proof duplicate handling.

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
keepalives reaching terminal `Closed` state. These are current software traces;
they do not compare cached versus scheduled announce persistence, reconnect an
underlying carrier stream, establish caller-visible close-reason taxonomy, or
cover broader packet/proof duplicate handling.

## Remaining acceptance boundary

The combined evidence now proves pinned Python↔Rust local attachment, announce
fan-out, a direct application/link exchange, Rust daemon restart with identity
continuity, two-carrier multi-hop Python Channel and split Resource exchanges,
and multi-hop Channel sequence deduplication through one forwarding Rust
transport, and application-link close/reconnect over that forwarding path. It
does not yet compare cached versus scheduled announce persistence or underlying
carrier stream reconnect behavior, caller-visible close-reason taxonomy, and
broader packet/proof duplicate handling remain separate. Those traces are still
required before this row can be promoted.
Hardware and public-network evidence remain separate acceptance axes.
