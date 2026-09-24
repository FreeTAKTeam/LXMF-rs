# Issue #608 — IFAC carrier wiring evidence

## Malformed IFAC startup status

`bootstrap_reports_invalid_ifac_config_without_creating_interface` verifies that
an enabled UDP interface with `ifac_size` but no usable IFAC credential is not
started, while the existing `list_interfaces` management RPC reports it once
with `_runtime.startup_status = "failed"` and a fixed `startup_error`. The
diagnostic contains no raw configuration values or credentials. This is a
software regression. The companion
`bootstrap_reports_invalid_ifac_size_without_exposing_credentials` case verifies
that a config rejected during deserialization for a seven-bit IFAC tag receives
the same safe management diagnostic even when a credential is present. The
reported message is static and excludes the supplied credential. Other startup
parse failures and the broader #608 acceptance remain open.

```text
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  bootstrap_reports_invalid_ifac_config_without_creating_interface -- --nocapture
  1 passed; 0 failed
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  bootstrap_reports_invalid_ifac_size_without_exposing_credentials -- --nocapture
  1 passed; 0 failed
```

Status: **authenticated TCP/UDP daemon paths and shared-instance/virtual-child
IFAC policy evidenced; serial and KISS stream runtime paths have deterministic
software regressions; outbound I2P fake-SAM stream IFAC rejection,
authenticated ingress/egress, and live parent-state rotation on a virtual peer
are covered; Meshtastic tunnel, Weave stream, and incoming I2P accepted-stream
workers now also have deterministic wrong-key rejection and authenticated
ingress/egress regressions; AutoInterface peer-data, LoRa, RNode bearer, and
RNodeMulti KISS-vport paths now have software wrong-key rejection and
authenticated ingress/egress regressions; invalid live IFAC reconfiguration
returns a structured RPC error without disabling the active authenticated
configuration; pinned-Python Backbone daemon traffic now verifies bidirectional
IFAC delivery and wrong-key rejection after restart; full interface-family
acceptance remains open**.

## RNode BLE production KISS-worker IFAC fault injection

The feature-gated regression
`rnode_ble_kiss_worker_authenticates_ifac_egress_and_admission` invokes the
actual `NativeRnodeBleKissInterface` worker with a private backend factory and
an in-memory fake BLE backend. It verifies that wrong-key KISS notification
data increments the worker's IFAC violation counter and is not routed, matching
key data is admitted, and a packet sent through the interface manager is
written as KISS data that authenticates with the configured IFAC context. This
is deterministic software fault-injection evidence only; it does not verify a
physical BLE adapter, RNode radio, or over-the-air behavior.

The processing order matches frozen Reticulum
`99de23c040d507e3fefca19e87b182302902725d`: Android `RNodeInterface.process_incoming`
hands received packet bytes to `owner.inbound`, while `process_outgoing`
frames packets before writing; the Rust worker authenticates before enqueueing
received packets and before KISS transmission.

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  rnode_ble_kiss_worker_authenticates_ifac_egress_and_admission -- --nocapture
# 1 passed; wrong key rejected/counted, matching key routed, egress authenticated
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  rnode_ble_virtual_child_uses_inherited_ifac_for_ingress_and_egress -- --nocapture
# virtual child shares host IFAC state; wrong-key ingress is not delivered and child-routed egress authenticates
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  rnode_ble_worker_cleans_up_failed_startup_before_retry_and_stop -- --nocapture
# failed startup cleanup precedes retry; active retry backend is cleaned on stop
```

The separate lifecycle regression injects a BLE connect failure, checks backend
cleanup before a retry starts, then cancels the successfully retried worker and
checks cleanup on stop. This is software-only fault injection, not physical
BLE/RNode or radio acceptance.

This closes one mocked BLE worker case only. Other carrier-family/startup
acceptance and physical/public-network evidence remain open; #608 and #605
remain partial.

The additional regression
`rnode_ble_startup_retry_keeps_ifac_required_and_counts_plaintext_rejection`
injects a failed first BLE connection followed by a successful retry through
the production KISS worker. It verifies the failed backend is cleaned, a
plaintext KISS packet is rejected and increments the interface IFAC-violation
counter, no plaintext packet reaches transport routing, and a matching-key
packet is admitted after the retry. This is deterministic fake-backend
software evidence for one BLE startup-retry path; it does not verify runtime
error-status reporting, other startup/stop/restart paths, other carrier
families, physical BLE/RNode behavior, or public-network behavior. Acceptance
item 4 and issue #608 therefore remain open.

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib rnode_ble_ -- --nocapture
  5 passed; 0 failed (all current RNode BLE unit regressions)
cargo fmt --all -- --check
  passed
git diff --check
  passed
```

The virtual-child regression creates a production `InterfaceManager` virtual
peer on the BLE host, verifies that it inherits the host configuration and
shares its live IFAC state, then sends wrong-key and matching-key KISS ingress
through the fake BLE worker. The wrong-key packet increments the violation
counter and never reaches transport delivery. A direct send addressed to the
virtual child is routed through the host worker and its emitted KISS packet
authenticates under the inherited child policy. This is software-only coverage;
it does not claim physical BLE or radio acceptance.

The implementation is based on the frozen Reticulum `1.5.4-dev` reference at
`99de23c040d507e3fefca19e87b182302902725d`. It wires the existing Rust
`IfacContext` through configured carrier ingress and egress, keeps the live
configuration shared with virtual/accepted children, applies carrier-specific
default tag sizes, and rejects missing, unexpected, malformed, or invalid IFAC
frames before packet admission. Invalid-frame counters are shared with the
interface runtime status path and rejection logs do not include wire payloads.

## Executed software evidence

The following checks passed on the isolated `codex/issue-605-parity` checkout:

```text
cargo test -p reticulum-rs-transport --lib
  791 passed; 0 failed
cargo test -p reticulum-rs-transport --all-features --lib
  805 passed; 0 failed
cargo test -p reticulum-rs-transport --lib memory_tests::
  12 passed; 0 failed
cargo test -p reticulum-rs-transport --lib iface::ifac_wire_tests
  5 passed; 0 failed
candidate: 6e5b1a4865594432a7fd0405fbaf800e71481cc1
cargo test -p reticulumd --test config
  122 passed; 0 failed
cargo test -p reticulumd --bin reticulumd interface_hot_apply
  35 passed; 0 failed
cargo test -p reticulumd --bin reticulumd --all-features
  466 passed; 0 failed
cargo test -p reticulumd --test transport_policy_evidence
  8 passed; 0 failed
cargo test -p reticulumd --test code_quality_issue_369
  1 passed; 0 failed
cargo test -p reticulumd --test python_channel_interop ifac -- --ignored --nocapture --test-threads=1
  4 passed; 0 failed
  candidate: 6e5b1a4865594432a7fd0405fbaf800e71481cc1
  Python Reticulum: 99de23c040d507e3fefca19e87b182302902725d
  cases: TCP Channel and Resource in both Rust->Python and Python->Rust directions
cargo test -p lxmf-cli --test python_lxmd_remote_relay ifac -- --ignored --nocapture --test-threads=1
  2 passed; 0 failed (serial process execution required)
  candidate: 6e5b1a4865594432a7fd0405fbaf800e71481cc1
  Python Reticulum: 99de23c040d507e3fefca19e87b182302902725d
  Python LXMF: 727830cefda83d9c6e3982b48675425f3f988f9c
  production path: Rust lxmd/reticulumd TCP server plus Python LXMF TCP client
  cases: Rust->Python and Python->Rust delivery; Python link status active; wrong Python credentials rejected before routing with a live IFAC violation counter; stop/restart with a wrong Rust credential rejected, then correct configuration restored with stable identity and fresh bidirectional delivery
```

The focused HDLC carrier tests cover valid authenticated ingress, plaintext,
tampered-tag, wrong-key, and truncated-frame rejection, plus authentication
before HDLC egress framing. Configuration tests cover bit-to-byte IFAC sizing,
credential aliases, incomplete configurations, hot-apply queueing, live
reconfiguration, virtual-interface inheritance, and invalid reconfiguration
rollback.

The mixed-peer tests use the same non-secret test credentials on both sides and
exercise the configured TCP carrier rather than a disconnected helper. The
daemon cases also verify that `lxmd` preserves `ifac_size`, `network_name`,
and `passphrase` when generating the `reticulumd` configuration, rejects a
wrong credential before routing, and does not bypass IFAC across a stopped,
reconfigured, and restarted daemon. The two daemon processes must run
serially because they use the shared Reticulum local-instance socket.

## Evidence boundary

This artifact does not promote the #605 behavioral row or child #608 to
complete. The daemon evidence covers authenticated TCP with wrong-credential
rejection and stop/reconfigure/restart, plus a separate-process Python/Rust UDP
IFAC path through `lxmd` and `reticulumd` that delivers direct LXMF messages in
both directions, observes an active Python link, and records zero live IFAC
violations. A second daemon trace verifies wrong-passphrase rejection before
peer or message admission. A third daemon trace rejects plaintext, invalid-tag,
and truncated UDP datagrams and confirms the violations are counted without
peer or message admission. The initial UDP daemon traces did not exercise a
valid authenticated frame tampered in transit or credential rotation/restart;
later PR #628 follow-ups cover both. A further PR #628 trace covers an
IFAC-protected UDP owner shared with an attached Rust client and a separate
pinned-Python peer, plus the virtual-child inherited IFAC admission policy.
Other carrier families through production paths remain open. Attached serial,
RNode, BLE, KISS, LoRa, Meshtastic, Weave, and public-network evidence is
outside this local software run. Those rows remain `partial / unverified` or
`hardware-unverified` in the forward ledger.

## Mainline follow-up: UDP software coverage

The following additional software checks were added on top of `main` at
`a649f51e9671007c08aeff469877038e2db7a716`:

```text
cargo fmt --all -- --check
  passed
cargo test -p reticulum-rs-transport --lib iface::ifac_wire_tests
  6 passed; 0 failed
cargo test -p reticulumd --test python_channel_interop \
  udp_ifac_ingress_counts_and_rejects_malformed_frames_before_admission -- --nocapture
  1 passed; 0 failed; missing-flag, tampered, wrong-key, and truncated UDP frames rejected,
  counted as decode errors and IFAC violations, with no packet admission
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  ifac_udp -- --ignored --nocapture --test-threads=1
  2 passed; 0 failed; Python/Rust authenticated UDP Channel request/reply and
  Resource payload/metadata transfers in both directions on a Rust-initiated Link
```

Short and invalid-length authenticated frames now map to `InvalidTag`, so the
existing IFAC violation counter classifies them consistently with bad tags.
The raw UDP regression also proves that a frame encoded with a different
passphrase is rejected and counted before packet admission. Plaintext remains
accepted when authentication is not configured, and the regression test covers
that behavior. The Python/Rust UDP round-trip is also
registered in `.github/workflows/verify.yml` against the pinned parity checkout
so it runs in PR CI. This follow-up adds UDP evidence; it does not close the
remaining carrier-family, frozen support-matrix, or physical-device evidence
gaps and does not mark #608 or #605 complete.

## PR #628 follow-up: UDP through the `lxmd` daemon path

The UDP success-path implementation is recorded at
`83d3d885e775f84b415b6f41d003cae8251c12e4`; the daemon wrong-credential and
malformed-frame regressions are at candidate commit
`7c3f715ee8c13d21dd6ad42ba4bd5029469ffb2d`, based on the exact Python
references above. The regression starts separate Rust `lxmd`/`reticulumd` and
Python LXMF/Reticulum processes over authenticated UDP and verifies successful
direct-message delivery both ways, an active Python delivery link, and zero
live Rust IFAC violations. The failure that motivated it exposed dropped UDP
forwarding endpoints in `lxmd`'s generated `reticulumd` configuration; the fix
preserves the endpoints and parses Python `UDPInterface` listen/forward aliases
and its shared-port form. A diagnostic regression also ensures nested
passphrase, IFAC key, secret, and token fields are redacted in failure snapshots.
A second UDP daemon trace uses a Python peer with a wrong passphrase and verifies
that the IFAC violation is counted while peer and message counts remain zero.
A third daemon trace sends plaintext, invalid-tag, and truncated UDP datagrams
and verifies the violation counter advances while peer and message counts stay
zero.

```text
candidate: 7c3f715ee8c13d21dd6ad42ba4bd5029469ffb2d
Python Reticulum: 99de23c040d507e3fefca19e87b182302902725d
Python LXMF: 727830cefda83d9c6e3982b48675425f3f988f9c
cargo fmt --all -- --check
  passed
cargo test -p lxmf-cli --bin lxmd
  16 passed; 0 failed
cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  rpc_diagnostics_redact_ifac_credentials_recursively -- --nocapture
  1 passed; 0 failed
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p lxmf-cli --test python_lxmd_remote_relay ifac -- \
  --ignored --nocapture --test-threads=1
  5 passed; 0 failed (serial process execution required)
cargo clippy -p lxmf-cli --all-targets --all-features --no-deps -- -D warnings
  passed
bash tools/scripts/check-module-size.sh
  passed
.github/workflows/verify.yml YAML parse
  passed
```

The Verify PR job now runs the ignored IFAC daemon scenarios against the
pinned Python checkouts. At PR #628 head
`657f9bde0aae5bc0cdcc0f26e4101b359af8d9d0`, two additional production-path UDP
checks passed locally against Reticulum
`99de23c040d507e3fefca19e87b182302902725d` and LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c`:

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p lxmf-cli --test python_lxmd_remote_relay \
python_rust_lxmd_ifac_udp_valid_frame_tampering_is_rejected_before_admission \
-- --ignored --nocapture --test-threads=1
# 1 passed; tampered valid IFAC frame counted and rejected before routing/delivery

RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p lxmf-cli --test python_lxmd_remote_relay \
python_rust_lxmd_ifac_udp_credential_rotation_and_restart_e2e \
-- --ignored --nocapture --test-threads=1
# 1 passed; live credential rotation, old-key rejection, restart, stable identity,
# and successful delivery under the rotated credentials
```

These traces close the UDP-tamper and UDP-rotation/restart software gaps for
this daemon path. The shared-instance test also passed at candidate
`9d8726d0cab8a9eff96bb3e4ece3b47641351557`, using the same pinned Python RNS
and LXMF revisions:

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p lxmf-cli --test python_lxmd_remote_relay ifac \
-- --ignored --nocapture --test-threads=1
# 8 passed; includes Python owner -> Rust local client -> Python IFAC UDP peer

cargo test -p reticulum-rs-transport --lib \
rns_1_5_virtual_ifac_child_enforces_inherited_authentication_policy -- --nocapture
# 1 passed; inherited child rejects open packet, admits authenticated form

cargo test -p reticulum-rs-transport --lib
# 815 passed; 0 failed

cargo clippy -p reticulum-rs-transport --lib --all-features --no-deps -- -D warnings
cargo clippy -p lxmf-cli --test python_lxmd_remote_relay --all-features --no-deps -- -D warnings
tools/scripts/check-boundaries.sh
tools/scripts/check-module-size.sh
# all passed; cargo fmt --all -- --check and git diff --check also passed
```

The IFAC-protected UDP owner forwarded bidirectional LXMF traffic between the
remote Python peer and the attached Rust client; the owner and Rust client both
reported zero IFAC violations. The virtual-child regression verifies inherited
policy at transport ingress. Other carrier families through production paths
remain open; this does not claim physical verification under #616.

## PR #628 follow-up: coherent packet authentication snapshot

`decode_packet_ifac` now uses one read-locked IFAC context for both frame
authentication and the packet's verified-wire provenance marker. This avoids
using different live configurations for those two decisions during
reconfiguration; the read lock is released before packet deserialization.
Packet-level tests cover authenticated decoding, plaintext rejection on an
authenticated interface, IFAC-frame rejection on a plaintext interface, and
plain packet provenance.

```text
cargo test -p reticulum-rs-transport --all-features --lib
  830 passed; 0 failed
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
  passed
cargo test -p reticulumd --bin reticulumd interface_hot_apply
  35 passed; 0 failed
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p lxmf-cli --test python_lxmd_remote_relay ifac -- \
  --ignored --nocapture --test-threads=1
  8 passed; 0 failed
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p reticulumd --test python_channel_interop ifac -- \
  --ignored --nocapture --test-threads=1
  6 passed; 0 failed
```

These checks exercise the common decoder through Rust/Python TCP and UDP
Channel, Resource, daemon, credential-rotation, tampering, and shared-instance
paths. They do not complete the remaining carrier-family or physical-support
matrix.

## PR #628 follow-up: PipeInterface IFAC runtime

The spawned Rust `PipeInterface` worker now has a Unix loopback regression
using a real `cat` subprocess. It configures IFAC without an explicit size,
checks the Python reference Pipe default of 8 bytes on the runtime channel,
then sends an authenticated packet through the production worker's IFAC and
HDLC transmit/receive path. The echoed packet is admitted with IFAC provenance,
the violation counter remains zero, and stopping the interface terminates the
child process.

```text
cargo test -p reticulum-rs-transport --lib \
  pipe_worker_roundtrips_authenticated_packet_with_reference_default_tag_size -- --nocapture
  1 passed; Unix-only subprocess loopback
```

This is carrier-runtime evidence, not Python-peer interoperability. TCP and
UDP retain the mixed Python/Rust evidence above; other carrier-family software
paths and physical-support rows remain open.

The complementary `pipe_stream_rejects_wrong_ifac_key_before_admission`
regression feeds a wrong-key authenticated HDLC packet through the production
Pipe stream worker's duplex-stream boundary. It verifies the IFAC violation
counter increments and the packet is not delivered to transport admission.
Together with the subprocess round-trip above, this covers Pipe worker
authentication in both directions at the software boundary; it does not claim
Python-peer interoperability or physical carrier acceptance.

```text
TMPDIR=/dev/shm cargo test -p reticulum-rs-transport --lib \
  pipe_stream_rejects_wrong_ifac_key_before_admission -- --nocapture
# 1 passed; wrong-key Pipe frame counted and not admitted
```

## Serial stream IFAC admission, rejection, and egress

`serial_stream_ifac_rejects_wrong_key_and_roundtrips_authenticated_packets`
exercises the production `run_serial_stream_with_ifac` worker over an in-memory
duplex serial stream. A frame authenticated with a distinct wrong-key context
increments the IFAC violation counter and never reaches transport admission; a
valid frame is admitted with IFAC provenance. A Rust-originated packet then
crosses the worker's IFAC/HDLC egress path and decodes under the configured
context. This is deterministic serial-stream software evidence only; it does
not claim a physical serial device or radio test.

```text
cargo test -p reticulum-rs-transport --lib \
  serial_stream_ifac_rejects_wrong_key_and_roundtrips_authenticated_packets -- --nocapture
# 1 passed; wrong-key rejection, authenticated ingress, and authenticated egress
```

Serial now has stream-level IFAC software evidence alongside the Pipe worker
loopback and pinned Python/Rust TCP/UDP traces. Physical serial/RNode and the
other carrier-family acceptance remain open.

## KISS TCP stream IFAC admission, rejection, and egress

Implementation and test commit: `e77285811838ee0d4efa24514fa22915e02504b8`
on the existing PR #628 branch.

`kiss_stream_ifac_rejects_wrong_key_and_authenticates_ingress_egress` exercises
the production `run_kiss_stream_with_ifac` worker over an in-memory duplex
stream using the AX.25 KISS payload adapter. With no explicit IFAC size, the
runtime uses the reference 8-byte default. A frame authenticated under a
different key increments the violation and deserialize-error counters and is
not admitted; an authenticated frame is admitted with IFAC provenance; and a
Rust-originated packet is authenticated before AX.25/KISS framing and decodes
under the configured context. Runtime packet/frame counters are asserted.
This is deterministic KISS stream software evidence, not a modem, radio, or
physical-carrier test.

```text
cargo test -p reticulum-rs-transport \
  kiss_stream_ifac_rejects_wrong_key_and_authenticates_ingress_egress -- --nocapture
# 1 passed; wrong-key rejection, authenticated ingress/egress, and runtime counters
cargo test -p reticulum-rs-transport --all-features --lib
# 833 passed; 0 failed
```

The KISS stream regression supplements serial and Pipe software evidence. It
does not establish Python-peer KISS interoperability or close the remaining
carrier-family/support-matrix requirements; issue #608 remains open.

## Outbound I2P tunneled-stream IFAC

`i2p_virtual_peer_inherits_parent_ifac_rotation_and_rejects_stale_credentials`
drives the production outbound I2P peer loop through a local fake SAM server.
The pinned Python `I2PInterface.incoming_connection` copies the parent's IFAC
size and credentials to its spawned peer. Rust shares the parent's `IfacState`
with the child; after the parent rotates credentials, the established child
rejects a wrong-key frame and a stale pre-rotation frame, admits a frame
authenticated with the rotated context, and emits egress under that new
context. This is deterministic virtual-child tunnel-stream evidence, not public
I2P connectivity, incoming-peer acceptance, or hardware evidence.

Validation passed on the updated #628 worktree:

```text
cargo test -p reticulum-rs-transport \
  i2p_virtual_peer_inherits_parent_ifac_rotation_and_rejects_stale_credentials \
  -- --nocapture
  1 passed; 0 failed
cargo test -p reticulum-rs-transport --all-features --lib
  834 passed; 0 failed
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
  passed
cargo fmt --all -- --check
  passed
bash tools/scripts/check-boundaries.sh
  passed
bash tools/scripts/check-module-size.sh
  passed
git diff --check
  passed
```

The fake-SAM regressions add software evidence for outbound and virtual-child
IFAC behavior on one tunneled carrier path;
remaining I2P lifecycle and SAM accept-loop integration, interface-family,
support-matrix, and operational acceptance remain open.

## Additional carrier-adapter IFAC regressions

Commit `49b7999f` adds three deterministic production-worker regressions on the
existing #628 branch. The Meshtastic tunnel test uses its injected interface
handle and verifies wrong-key rejection/error evidence, authenticated ingress,
and authenticated egress after tunnel reassembly. The Weave stream test
performs its signed discovery handshake, rejects a wrong-key endpoint packet
with an IFAC-violation count, admits a valid authenticated packet, and checks
authenticated egress. The I2P incoming accepted-stream test uses a local TCP
pair to exercise the production HDLC worker, verifies wrong-key rejection
before admission and authenticated ingress/egress, and checks child cleanup.
These are software adapter tests only: they do not exercise a radio, a public
I2P router, or a full hardware/support matrix. #616 remains a separate
operational gate.

```text
cargo fmt --all -- --check
# passed
cargo test -p reticulum-rs-transport ifac -- --nocapture
# 394 passed; 0 failed
cargo test -p reticulum-rs-transport --lib
# 823 passed; 0 failed
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
# passed
bash tools/scripts/check-boundaries.sh
# passed
bash tools/scripts/check-module-size.sh
# passed
git diff --check
# passed
```

The uncovered carrier families and broad startup/configuration/error matrix
remain open, so these additions do not close #608 or promote #605.

## Meshtastic IFAC worker cancellation cleanup

`meshtastic_ifac_worker_stops_cleanly_without_clearing_authentication` starts
the production in-memory Meshtastic tunnel worker with IFAC configured, requests
interface stop, and bounds the worker join. It verifies the worker exits
normally, publishes its stopped state, and retains the same authenticated wire
context across shutdown. This complements the existing wrong-key and
authenticated ingress/egress regression; it does not cover external Meshtastic
startup failures, daemon error reporting, restart/reconfiguration, or other
carrier families.

```text
cargo test -p reticulum-rs-transport \
  meshtastic_ifac_worker_stops_cleanly_without_clearing_authentication -- --nocapture
# 1 passed; graceful worker stop; IFAC context unchanged
```

The broad #608 carrier-family and lifecycle matrix remains open.

## AutoInterface and RNode-family IFAC regressions

The follow-up software tests exercise the AutoInterface peer-data bridge and
the production LoRa stream, RNode bearer, and RNodeMulti KISS-vport workers.
Each injects a packet authenticated with a different key and verifies it is
not admitted; authenticated ingress and egress are then checked against the
configured context. The bearer test also waits for its production startup
monitor to validate before asserting packet egress. These use local sockets,
streams, and a fake bearer; they do not establish Python-peer interoperability,
radio behavior, hardware support, or the complete carrier lifecycle matrix.

```text
cargo fmt --all -- --check
# passed
cargo test -p reticulum-rs-transport ifac -- --nocapture
# 397 passed; 0 failed
cargo test -p reticulum-rs-transport --lib
# 826 passed; 0 failed
cargo test -p reticulum-rs-transport --all-features --lib
# 840 passed; 0 failed
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
# passed
bash tools/scripts/check-boundaries.sh
# passed
bash tools/scripts/check-module-size.sh
# passed
git diff --check
# passed
```

Issue #608 remains open for its broader carrier-family and
startup/configuration/child-inheritance/stop-restart/error matrix. Physical
acceptance remains outside this software evidence and is separately tracked by
#616.

## Rejected live IFAC reconfiguration and fail-closed restart

The UDP credential-rotation regression now submits a `set_interfaces` update
with an IFAC tag size but no credentials. The daemon returns a framed
`CONFIG_INVALID_IFAC` RPC error, the already-active rotated Python peer remains
usable, and after daemon restart a plaintext Python peer is still rejected by
the IFAC-protected interface. The RPC error uses static text, so credentials
and raw configuration values are not reflected to the caller. A focused RPC
test checks the typed IFAC error mapping, ensures a duplicate interface named
`ifac-duplicate` is still classified as a generic interface error, and verifies
that a stopped-worker `BrokenPipe` failure is not marked retryable or allowed
to replace stored interfaces. The daemon hot-apply tests also verify the IFAC
failure retains its typed source and does not queue the rejected update.

```text
cargo fmt --all -- --check
# passed
cargo test -p reticulum-rs-rpc
# 751 passed; 0 failed
cargo test -p reticulum-rs-rpc set_interfaces_keeps_stored_interfaces_unchanged_when_bridge_fails
# 1 passed; 0 failed
cargo test -p reticulum-rs-rpc interface_mutation_error_maps_typed_ifac_failure_without_display_matching
# 1 passed; 0 failed
cargo test -p reticulumd --bin reticulumd interface_hot_apply
# 35 passed; 0 failed
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
LXMD_TEST_LOGS=1 cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_rust_lxmd_ifac_udp_credential_rotation_and_restart_e2e \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed; 0 failed
cargo clippy -p reticulum-rs-rpc --all-targets --all-features --no-deps -- -D warnings
# passed
cargo clippy -p reticulumd --bin reticulumd --all-targets --all-features --no-deps -- -D warnings
# passed
cargo clippy -p lxmf-cli --test python_lxmd_remote_relay --all-features --no-deps -- -D warnings
# passed
```

This closes the invalid-reconfiguration error-reporting and rollback subpath;
it does not complete #608's broader carrier-family, startup/error matrix, or
physical/public-network acceptance.

## IFAC-enabled UDP startup bind-error reporting

The production daemon bootstrap regression
`bootstrap_reports_bind_failure_for_ifac_enabled_udp_interface` reserves the
configured loopback UDP port, starts an IFAC-enabled UDP interface in
best-effort mode, and reads live `list_interfaces` status until the worker
reports `bind_failed`. It asserts that daemon startup distinguishes interface
creation (`startup_status=spawned`) from worker health, exposes the socket
failure through UDP `last_error`, and does not reflect the configured
passphrase in that error.

The strict-startup counterpart,
`bootstrap_strict_startup_rejects_bind_failure_for_ifac_enabled_udp_interface`,
reserves a loopback UDP port and attempts bootstrap with strict interface
startup enabled and the same IFAC configuration. Bootstrap panics with the
strict-policy rejection, identifies exactly one failed
`ifac-strict-startup (udp)` interface, and does not include the configured
passphrase. Because bootstrap fails before returning a daemon context, the
failed interface is not available as an active daemon interface. The original
best-effort test remains unchanged. This adds one software-only UDP startup
case; it does not expand the carrier matrix or complete broader #608
acceptance.

```text
cargo fmt --all -- --check
# passed
TMPDIR=/var/tmp cargo test -p reticulumd --bin reticulumd \
  bootstrap_reports_bind_failure_for_ifac_enabled_udp_interface -- --nocapture
# 1 passed; IFAC UDP bind failure and redacted runtime error reported
TMPDIR=/var/tmp cargo test -p reticulumd --bin reticulumd \
  bootstrap_strict_startup_rejects_bind_failure_for_ifac_enabled_udp_interface -- --nocapture
# 1 passed; strict bootstrap rejected the failed IFAC UDP interface without credential disclosure
```

This narrows one startup/error-reporting gap only. The remaining lifecycle and
carrier-family matrix remains open; #608 and #605 remain partial.

## Accepted-child credential rotation follow-up

The transport ingress suite now exercises the accepted-stream configuration
path independently of the virtual-peer path: it creates a child channel,
inherits the configured parent policy through `InterfaceManager`, rotates the
parent credentials while the child remains attached, then decodes actual IFAC
wire frames using the child's production `IfacState`. The former credential is
rejected and the rotated credential is admitted. This confirms the existing
`set_shared_config` propagation behavior; no production-code change was needed.

```text
cargo test -p reticulum-rs-transport --all-features --lib \
  rns_1_5_accepted_child_uses_parent_ifac_rotation_for_wire_admission -- --nocapture
# 1 passed; old-key frame rejected, rotated-key frame admitted
```

This adds one accepted-child software reconfiguration case only. It does not
complete the broader startup/error-reporting or carrier-family matrix, and it
is not physical-carrier evidence; #608 remains open and partial.

## TCP accepted-client policy is installed before worker startup

The TCP listener previously spawned each accepted `TcpClient` worker and only
then copied the listener's shared runtime policy into its child channel. Since
the worker can begin reading concurrently, its initial IFAC state could be the
default unauthenticated state. `InterfaceManager::spawn_inheriting` now copies
the parent's configuration and IFAC context before scheduling the child worker;
TCP acceptance uses this path and declines the client with a fixed runtime
error if inheritance cannot be completed.

`inherited_interface_worker_starts_with_parent_ifac_policy` checks policy at
the child's first worker poll and verifies that a plaintext frame is rejected.
This deterministic regression covers the TCP accepted-stream startup ordering,
not TCP credential rotation, listener restart, or every carrier family.

```text
TMPDIR=/dev/shm cargo test -p reticulum-rs-transport --lib \
  inherited_interface_worker_starts_with_parent_ifac_policy -- --nocapture
# 1 passed; IFAC policy present on first poll; plaintext rejected
TMPDIR=/dev/shm cargo test -p reticulum-rs-transport --lib \
  tcp_server_runtime_status_tracks_accepted_client -- --nocapture
# 1 passed; accepted-client listener runtime regression
```

This closes one accepted-TCP child startup-ordering gap only; broad #608
startup/configuration/error-reporting and carrier-family acceptance remain open.

## Backbone daemon IFAC ingress and egress

The ignored pinned-Python daemon regression
`python_rust_lxmd_backbone_ifac_bidirectional_daemon_e2e` now selects the
production Rust `backbone` adapter rather than the generic `tcp_server` kind.
Against frozen Reticulum
`99de23c040d507e3fefca19e87b182302902725d`, a Python
`TCPClientInterface` and Rust Backbone listener exchange LXMF messages in
both directions under matching IFAC credentials. The test restarts the Rust
daemon with a wrong credential, verifies the Python announce is counted as an
IFAC violation before peer admission, then restores the matching credentials
and verifies bidirectional delivery again. This covers the Backbone listener
and its accepted-client IFAC inheritance through the production adapter; it
does not add new physical-device evidence or complete the remaining
carrier-family/startup matrix.

```text
TMPDIR=/dev/shm TMP=/dev/shm \
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_rust_lxmd_backbone_ifac_bidirectional_daemon_e2e \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed; bidirectional IFAC delivery, wrong-credential rejection, restart recovery
```

The baseline generic TCP daemon test also passed once with `/dev/shm` temporary
storage before applying this Backbone-only adaptation. #608 remains open and
partial.

## Active-carrier IFAC rotation stays fail-closed

The production `InterfaceManager::set_shared_config` path validates the new
IFAC context before touching a live interface and replaces the shared context
under one write lock. A concurrent ingress regression exercises that path on
an active interface channel while rotating between two valid credentials. It
synchronizes a plaintext-ingress probe after the first live credential
replacement, keeps the reader active through the remaining updates, and
verifies no plaintext frame is admitted across 2,000 credential updates. No
production fix was indicated. The common carrier decoder takes the matching
read lock, so ingress observes the previous or replacement authenticated
context, never an intermediate plaintext state.

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d` has no live
credential hot-apply path: `RNS/Reticulum.py::_add_interface` assigns
`ifac_size`, `ifac_netname`, and `ifac_netkey`, derives the IFAC identity, and
only then registers the interface with `RNS.Transport` and calls
`interface.final_init()`. The Rust runtime reconfiguration regression
therefore checks a stronger live-update invariant than Python's startup-only
ordering. This does not claim that every carrier family or physical interface
has been exercised.

```text
cargo test -p reticulum-rs-transport live_ifac_rotation_never_admits_plaintext_during_reconfiguration -- --nocapture
# 1 passed; synchronized ingress remained fail-closed across 2,000 live credential updates
```

## Invalid IFAC startup configuration is reported without admission

The production daemon bootstrap regression supplies a UDP interface with an
IFAC size but no credentials, then queries `list_interfaces` through the
daemon RPC API. The invalid configuration is rejected by `DaemonConfig` and
the interface is not started, but the existing `list_interfaces` response
contains one sanitized record with `_runtime.startup_status = "failed"` and a
fixed IFAC validation message. The record contains no raw config values or
credentials. Config loading continues to log its parse error; no interface
transport is created for the rejected entry.

```text
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  bootstrap_reports_invalid_ifac_config_without_creating_interface -- --nocapture
# 1 passed; malformed config reported through list_interfaces, no UDP interface started
```

This verifies reporting for the missing-credential IFAC configuration case. It
does not complete other parse/startup reporting, carrier-family,
physical-device, or broader #608 acceptance.

The bootstrap regression `bootstrap_reports_nonnumeric_ifac_size_without_exposing_credentials`
supplies a nonnumeric `ifac_size` string while providing a passphrase. Pinned
Python RNS 1.5.4's `ConfigObj.as_int` rejects this value with `ValueError`; the
production Rust bootstrap retains one failed `list_interfaces` diagnostic and
returns its fixed IFAC validation message without reflecting the passphrase.
This verifies the Rust startup/error-reporting path for this malformed input;
it does not assert identical error text or cover other parse failures or the
broader acceptance row.

```text
cargo test -p reticulumd --bin reticulumd \
  bootstrap_reports_nonnumeric_ifac_size_without_exposing_credentials -- --nocapture
# 1 passed; 0 failed
```

## Conflicting IFAC credential aliases follow pinned startup precedence

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`,
`RNS/Reticulum.py::_add_interface`, assigns `networkname` before
`network_name`, and `passphrase` before `pass_phrase`; when both forms are
configured, the latter spelling in each pair wins. Rust's configuration
resolver had the `passphrase` precedence reversed. The daemon-bootstrap
regression `bootstrap_prefers_pinned_python_ifac_credential_alias_on_conflict`
starts a real IFAC-enabled UDP interface with both spellings set to distinct
values and UDP port zero for a genuinely available ephemeral bind. It checks
that the selected network name and passphrase match the pinned precedence and
polls the production UDP runtime with a two-second bound until it reports
`bound` (a merely `spawned` interface does not pass). The resolver now follows
the pinned Python ordering. This is an automated configuration/startup
regression; it does not complete the broader lifecycle or carrier-family
acceptance.

```text
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  bootstrap_prefers_pinned_python_ifac_credential_alias_on_conflict -- --nocapture
# 1 passed; both conflicting aliases selected according to pinned Reticulum
```

## Rejected live IFAC update remains fail-closed on the restarted UDP daemon

The Python/Rust UDP reconfiguration E2E now attempts an invalid live
`set_interfaces` update after the Rust daemon has restarted with its valid
rotated IFAC credentials, while a separate Python UDP peer continuously sends
plaintext announces. The update must return the typed IFAC configuration RPC
error; the daemon's IFAC-violation counter must increase, and `path_status` for
the plaintext peer's destination must remain unknown. This ties rejection,
continued authentication, and externally visible routing state to the same
running daemon/carrier session rather than inferring rollback from helper
results. It supplements the earlier authenticated-peer continuity case and
does not close the remaining startup/configuration or carrier-family matrix.

```text
TMPDIR=/dev/shm cargo test -p lxmf-cli --test python_lxmd_remote_relay \
  python_rust_lxmd_ifac_udp_credential_rotation_and_restart_e2e -- --ignored --nocapture
# 1 passed; pinned Python plaintext announces remained rejected across failed update
```

## IFAC-configured TCP listener bind failure reporting

At pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`,
`TCPServerInterface.__init__` resolves and binds its listener before starting
`serve_forever`; `incoming_connection` is reached only for connections accepted
by that listener. The Rust production-daemon regression occupies the configured
TCP port, starts an IFAC-configured `tcp_server`, and polls `list_interfaces`
until the worker reports `listener_state = "bind_error"`. It verifies the
runtime error contains neither the configured network name nor passphrase and
that the failed listener reports zero accepted connections and no admitted
client interface. A strict-startup companion uses the same occupied-port
fixture and verifies that `bootstrap()` waits for the TCP worker's initial bind
result and rejects startup with a credential-free policy error. The listener
worker continues its existing retry behavior after reporting that first result;
best-effort startup still returns and exposes the deferred runtime diagnostic.
This supplements the existing UDP bind-failure coverage; it does not exercise
TCP accepted-stream authentication or prove the broader startup/configuration
matrix.

```text
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  bootstrap_reports_sanitized_bind_failure_for_ifac_enabled_tcp_listener -- --nocapture
# 1 passed; failed IFAC TCP listener reported sanitized bind_error and admitted no clients
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  tests::bootstrap_strict_startup_rejects_bind_failure_for_ifac_enabled_tcp_listener -- --exact
# 1 passed; strict startup rejected the initial worker bind result without exposing credentials
cargo test -p reticulumd --bin reticulumd
# 476 passed; 0 failed
```

The regression exposed that startup records lacked the `tcp.listener_status`
object required by the existing runtime refresher. Bootstrap now seeds the
configured listener status so the worker's bind failure reaches `list_interfaces`.
Strict bootstrap additionally receives the worker's initial bind outcome over
an optional one-shot result channel; best-effort startup does not wait on it.
This is software-only local evidence; no physical/HIL test was run.

## Unix shared-instance clients keep the reference plaintext exception

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d` creates attached
`LocalClientInterface` objects in `RNS/Interfaces/LocalInterface.py` without
copying the `LocalServerInterface` IFAC identity or key (`incoming_connection`,
lines 419-462). In contrast, `TCPServerInterface.incoming_connection` explicitly
copies and derives IFAC state for accepted TCP clients (lines 616-634). The Rust
Unix shared-instance accept path previously inherited the parent IFAC state,
which made attached local clients reject plaintext frames and authenticate their
egress—different from the pinned shared-instance behavior.

`LocalUnixServer` now spawns accepted clients after inheriting routing/runtime
settings but deliberately omits IFAC credentials. Later parent configuration
updates continue to propagate non-IFAC settings without turning local IPC into
an authenticated carrier. The regression uses a real Unix socket, rotates the
parent credentials after client attachment, then verifies plaintext ingress is
admitted and direct egress remains untagged. This is software-only shared-instance
compatibility evidence; it does not promote the broad #608 acceptance checkbox.

```text
TMPDIR=/dev/shm cargo test -p reticulum-rs-transport --lib \
  local_unix_shared_clients_keep_plaintext_wire_policy_with_parent_ifac -- --nocapture
# 1 passed; real Unix socket ingress/egress remains plaintext across parent IFAC rotation
```

## PR #628 hosted PR-HIL fixture follow-up

Run `35907636125` failed one virtual `python-channel-interop` case,
`python_to_python_resource_roundtrip_through_rust_transport`. Its uploaded
`hil-pr` artifact records the client receiving
`resource-sha256-metadata:1048832:3a4739461afba12d31aaceffc313cac937feb465456d0cc483b55b2b6c319271:1048847:python-meta`
and then timing out while awaiting the endpoint callback. The endpoint had
delivered its acknowledgment; the client fixture was still matching the old
metadata-free `resource-sha256` format. PR #638's metadata-aware expectation
confirms the same fixture mismatch. This #628 follow-up updates only that
Python test-client expectation; it does not change Resource or IFAC production
behavior. No physical device was used, and local HIL was not run.

## Pinned Python/Rust KISS serial IFAC interoperability over software PTYs

Against pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`, the
Python `KISSInterface` and Rust production `KissInterface` run as separate
nodes on opposite sides of a raw two-PTY software relay. Both use the same
128-bit configured IFAC credentials. The Python node announces a Channel
destination; Rust establishes a Link, sends a Channel message, receives the
Python reply, and observes the delivery proof. The Rust interface traffic
snapshot records receive and transmit bytes with zero IFAC violations. This
adds one pinned-Python serial/KISS software path; it does not exercise a modem,
radio, physical serial adapter, or broader platform/carrier matrix, so #608
remains open.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF LXMF_PYTHON_BIN=python3 \
cargo test -p reticulumd --test python_channel_interop \
  python_rust_ifac_kiss_serial_channel_roundtrip_over_pty \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed; Python/Rust KISS Channel request/reply and proof over PTYs, 0 IFAC violations
```

| Carrier | Software regression | Evidence boundary |
| --- | --- | --- |
| Meshtastic | `meshtastic_ifac_worker_rotates_credentials_without_plaintext_fallback` rotates IFAC through `InterfaceManager::set_shared_config` while the production worker runs; old-key and plaintext ingress are rejected and counted, replacement-key ingress is admitted, and egress authenticates with an independently derived replacement key. | In-memory tunnel/worker only; no physical Meshtastic device or full #608 acceptance. |
