# Issue #608 — IFAC carrier wiring evidence

Status: **implemented but unproven for forward-parity acceptance**.

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
  4 passed; 0 failed
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
```

The focused HDLC carrier tests cover valid authenticated ingress, plaintext,
tampered-tag, wrong-key, and truncated-frame rejection, plus authentication
before HDLC egress framing. Configuration tests cover bit-to-byte IFAC sizing,
credential aliases, incomplete configurations, hot-apply queueing, live
reconfiguration, virtual-interface inheritance, and invalid reconfiguration
rollback.

## Evidence boundary

This artifact does not promote the #605 behavioral row to verified. A pinned
Python↔Rust daemon pair has not yet been run through real UDP/TCP carrier
traffic with bidirectional announces, packets, proofs, links, and Resources.
Attached serial, RNode, BLE, KISS, LoRa, Meshtastic, Weave, and public-network
evidence is also outside this local software run. Those rows remain
`partial / unverified` or `hardware-unverified` in the forward ledger.
