# Issue #608 — IFAC carrier wiring evidence

Status: **authenticated TCP/UDP daemon paths and shared-instance/virtual-child IFAC policy evidenced; full interface-family acceptance remains open**.

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
