# Issue #608 — IFAC carrier wiring evidence

Status: **mixed software path evidenced; forward-parity acceptance remains open**.

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
complete. The daemon evidence covers one explicit authenticated TCP server and
one Python LXMF client, including wrong-credential rejection and a
stop/reconfigure/restart path; it does not cover UDP, raw tampered or
truncated frames through the daemon process, or every carrier family.
Attached serial, RNode, BLE, KISS, LoRa, Meshtastic, Weave, and public-network
evidence is also outside this local software run. Those rows remain
`partial / unverified` or `hardware-unverified` in the forward ledger.

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
  python_rust_ifac_udp_channel_roundtrip -- --ignored --nocapture --test-threads=1
  1 passed; 0 failed; Python/Rust authenticated UDP channel request and reply
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
