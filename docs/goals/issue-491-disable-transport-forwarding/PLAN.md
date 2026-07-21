# Disable Transit Forwarding Implementation Plan

**Intent:** Make Rust `TransportConfig` expose and enforce the Python Reticulum `enable_transport = false` contract reported in issue #491.
**Current Behavior:** `set_retransmit(false)` suppresses announce and path-request retransmission, but known-path Link Requests and established-link traffic can still cross the process between interfaces.
**Expected Outcome:** A transport-disabled instance continues to serve local destinations and locally owned links but never relays traffic for remote destinations or transit links.
**Target-Perspective Output:** An application operator can connect the process to trusted and untrusted networks and observe that a third party cannot activate or use a link through it when transport forwarding is disabled.
**Truth Owner:** `TransportConfig::transport_enabled` in `crates/libs/rns-transport`.
**Contract Boundary:** Daemon `[reticulum] enable_transport` and public `TransportConfig` configuration enter the inbound transport routing pipeline.
**Cutover:** Replace the internal `retransmit` policy flag with `transport_enabled`; retain `set_retransmit` as a compatibility alias while directing new callers to `set_transport_enabled`.
**Displaced Path:** Remove ungated remote forwarding from Link Request, proof, data, keepalive, and resource handlers.
**Value Density:** One policy flag closes the security/correctness gap across every transit packet family without changing local application traffic.
**Acceptance Evidence:** A two-interface regression recreates the issue #491 attacker/app/host boundary and proves no packet reaches the host-facing interface while local Link Requests still work; focused crate and daemon policy tests pass.
**Evidence Lane:** Unit and simulated transport-boundary tests, followed by formatting, clippy, and workspace tests.
**Kill Criteria:** No internal forwarding decision may use the old `retransmit` field or bypass the authoritative transit-policy check.
**Architecture Slice:** Configuration in `transport/{mod,config,core}.rs`; inbound transit decisions in `transport/{path,wire,resource_wire}.rs`; daemon mapping in `bootstrap_transport.rs`; policy evidence in `transport_policy_evidence.rs`; parity status in `docs/status`.
**Plan Review Gate:** Requires PRE review before execution.

## Task 1: Establish the configuration contract

- Files: `crates/libs/rns-transport/src/transport/mod.rs`, `config.rs`, `core.rs`, transport maintenance modules, and `crates/apps/reticulumd/src/bin/reticulumd/bootstrap_transport.rs`.
- Allowed scope: Rename the internal policy to `transport_enabled`, add its explicit setter, and preserve the old setter as an alias.
- Expected output: One source of truth for transport-instance behavior.
- Verification: Focused config and daemon bootstrap tests.
- Acceptance evidence: Both public setters produce identical transport-enabled state and daemon `enable_transport` selects it.
- Parallel: No.

## Task 2: Gate transit forwarding

- Files: `crates/libs/rns-transport/src/transport/path.rs`, `wire.rs`, and `resource_wire.rs`.
- Allowed scope: Gate only remote/transit sends; do not block local destinations, locally owned links, or their replies.
- Expected output: Disabled instances cannot create transit link-table state or forward transit packets.
- Verification: New focused transport tests plus existing routed-link/resource tests.
- Acceptance evidence: Known-path Link Requests and established-link packets do not leave the second interface when disabled.
- Parallel: No.

## Task 3: Record parity and validate

- Files: `crates/apps/reticulumd/tests/transport_policy_evidence.rs`, `docs/status/reticulum-parity-matrix.md`, and `docs/status/current-roadmap.md`.
- Allowed scope: Add issue-shaped regression evidence and correct the parity claim.
- Expected output: Repository status matches the implemented contract.
- Verification: `cargo fmt --all -- --check`, focused package tests, workspace clippy, and workspace tests.
- Acceptance evidence: The issue-shaped regression and full required checks pass on the final diff.
- Parallel: No.
