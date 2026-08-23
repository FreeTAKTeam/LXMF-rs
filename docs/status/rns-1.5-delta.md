# RNS 1.5.0 Alignment Ledger

Last reassessed: 2026-08-23

This ledger classifies every item in the upstream Reticulum 1.5.0 `Changes`
section. The authority is tag `1.5.0`, peeled to
`e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`; the previous active reference was
`1.4.2`. The source comparison is `1.4.2..1.5.0` (88 commits, 65 changed
files). Callable classification is independently generated in
`python-surface-parity.json`; this file covers release-note behavior that an
AST inventory cannot prove.

`complete` means the applicable software behavior is implemented. `equivalent`
means the Rust architecture avoids the Python-specific failure mechanism while
preserving the observable contract. `hardware-unverified` and
`platform-unverified` constrain evidence, not implementation.

## Added and improved behavior

| # | Upstream change | Rust disposition | Focused evidence | Remaining boundary |
|---:|---|---|---|---|
| 1 | Operator LXMF address in discovery information | complete: optional address is encoded, decoded, retained, and published by the daemon for supported interface types, including TCP clients; encrypted publication uses one explicitly configured shared network identity across nodes | `cargo test -p reticulum-rs-transport rns_1_5_operator_lxmf_address_roundtrips`; `cargo test -p reticulumd rns_1_5_encrypted_discovery`; `cargo test -p reticulumd rns_1_5_tcp_client_interface_is_publishable_and_wire_valid` | Physical discovery carriers remain hardware-unverified. |
| 2 | Prioritized transport ingress | complete: four strict-priority FIFO queues replace the undifferentiated drain | `cargo test -p reticulum-rs-transport inbound_queues` | Strict priority intentionally permits lower-class starvation, matching RNS. |
| 3 | Configurable data/announce/path-request/ingress-limited queue lengths | complete: defaults 4096/256/256/128 and positive-value validation flow from daemon config to transport | `cargo test -p reticulumd rns_1_5_queue_lengths`; `cargo test -p reticulum-rs-transport rns_1_5_queue_limits` | None on the software axis. |
| 4 | Early ingress filtering and filtering efficiency | complete: hop, destination-type, transported Plain/Group, IFAC flag-policy, and exact destination/tag duplicate checks run before routing and cryptographic work | `cargo test -p reticulum-rs-transport rns_1_5_ingress`; issue-369 scanner | Raw Reticulum IFAC authentication is not implemented; daemon IFAC configuration fails closed. |
| 5 | Per-interface protocol violation tracking | complete: the live prequeue path records protocol, IFAC flag-policy, and packet-filter failures in typed counters | `cargo test -p reticulum-rs-transport rns_1_5_ingress`; `cargo test -p reticulum-rs-transport rns_1_5_interface_traffic` | Physical IFAC fault injection remains hardware-unverified. |
| 6 | In-flight path-request tracking | complete: destination-keyed records retain expiry, outbound interface, and all requesters | `cargo test -p reticulum-rs-transport rns_1_5_path_request_batch` | None. |
| 7 | In-flight path-request request/response batching | complete: duplicates coalesce and one matching announce answers every retained ingress interface | `cargo test -p reticulum-rs-transport rns_1_5_path_request_batch`; `matching_announce_consumes_waiting_discovery_requesters` | Multi-radio soak is separate evidence. |
| 8 | Blackholed result from announce validation | complete at the Rust policy boundary: transport maintains the daemon blackhole set, filters both normal and ingress-limited announces, and synchronizes persisted state when the bridge attaches | `cargo test -p reticulum-rs-transport blackholed_identity_path_eviction`; `cargo test -p reticulum-rs-rpc blackhole` | Core signature validation stays policy-free by design. |
| 9 | Full link MDU for Channel and Buffer | complete: originated/retried link requests clamp signalling to the next-hop MTU, link encryption accepts the negotiated cleartext MDU, Channel rejects payloads beyond its two-byte length envelope, and stream payload is Channel MDU minus the two-byte stream header | `cargo test -p reticulum-rs-transport rns_1_5_originated_link_request_signals_next_hop_interface_mtu`; `rns_1_5_channel_packet_uses_negotiated`; pinned-Python Backbone `rust_to_python_channel_buffer_roundtrip` with an incompressible 600-byte payload | None on software-supported link MTUs. |
| 10 | Queue pressure/drop statistics in `rnstatus` | complete in typed status, JSON, and human output | `cargo test -p reticulumd rns_1_5_transport_status`; `cargo test -p rns-tools rns_1_5_human_status`; exact-head loaded-daemon capture in the candidate ledger | None on the software axis. |
| 11 | Detailed announce/path-request traffic flow per interface | complete: bytes, rates, and class composition are tracked from ingress and egress | `cargo test -p reticulum-rs-transport rns_1_5_interface_traffic` | Hardware traffic-rate calibration is separate. |
| 12 | Total announce/path-request count and frequency per interface | complete | same counter test and daemon status bridge test | None on deterministic software input. |
| 13 | Data-flow speed and composition | complete: aggregate and per-interface RX/TX bytes and speeds are exposed | same counter and status tests | Long-duration production sampling remains operational evidence. |
| 14 | Protocol-violation statistics in `rnstatus` | complete | live ingress counter and `rnstatus` rendering tests | Full raw IFAC authentication and physical IFAC evidence remain separate. |
| 15 | Active-link statistics in `rnstatus` | complete: total and validated-active counts are separate accessors | `cargo test -p reticulum-rs-transport rns_1_5_runtime_accessors`; status tests | None. |
| 16 | Blocked-IP listings in `rnstatus` | preserved complete: sorted live Backbone listener state is rendered | `cargo test -p rns-tools rnstatus`; `fast_flapping_blocks_after_grace_and_reports_live_ip_state` | Public listener soak is separate. |
| 17 | Medium-bitrate timeout helpers and RPC | complete: the slowest interface considers only live online, non-stopped carriers; the exact MTU round-trip formula flows through legacy RPC, typed bridge, JSON, and human status | `cargo test -p reticulum-rs-transport rns_1_5_lowest_interface_bitrate_ignores_offline_interfaces`; `cargo test -p reticulum-rs-rpc path_rpc` | None. |
| 18 | Extra timeout for discovery requests on slow interfaces | complete: in-flight expiry takes the maximum configured/medium timeout | `cargo test -p reticulum-rs-transport rns_1_5_discovery_timeout` | Physical slow-link timing remains hardware-unverified. |
| 19 | Adaptive `rncp`, `rnpath`, `rnprobe`, and `rnx` timeout | complete where network discovery exists: `rnpath` queries medium timeout and `rnprobe` delegates to it; `rnx` Reticulum path scenarios invoke `rnpath`; the repository's root-scoped deterministic `rncp` has no network/link timeout | `cargo test -p rns-tools adaptive_timeout`; `cargo test -p rns-tools --bin rnprobe`; `cargo test -p rns-tools --bin rncp` | Rust `rncp` is deliberately a local deterministic workflow, not a live RNS file-transfer client. |
| 20 | Adaptive timeout calculation in `rngit` | complete for the transport-neutral client: `ReticulumGitClient::connect_remote` applies a caller-injected typed medium timeout before remote work | `cargo test -p rns-tools rns_1_5_rngit_adaptive_timeout` | The shipped CLI remains a local Git workflow; live RNS remote timing is a separate network scenario. |
| 21 | Faster overall inbound processing | complete structurally: one fast ingestor performs bounded early classification and one drainer owns routing | ingress tests; module-size and architecture checks | Performance claims require a fresh versioned benchmark dataset. |
| 22 | Significantly improved path-request handling | complete through batching, scoped duplicate keys, retained requesters, and adaptive expiry | `cargo test -p reticulum-rs-transport path_request` | Public-network convergence remains separate. |
| 23 | Improved path-request ingress limiting/accounting | complete through bounded class queue, per-interface counters, and destination batching | queue, path-request, and traffic-counter tests | Hardware bursts remain separate. |
| 24 | Responsive egress limiting under high path-request load | complete: ingress work no longer blocks interface egress and request emission is coalesced | inbound queue starvation/order tests; path batching tests | Long-running high-load soak is separate. |
| 25 | Improved transport background jobs | complete: ingress and drain workers are independently supervised and cancellation-safe | transport worker-supervision tests; `cargo test -p reticulumd --test code_quality_issue_369` | None. |
| 26 | Early rejection of excessive hop counts | complete before routing/crypto with violation accounting | `cargo test -p reticulum-rs-transport rns_1_5_ingress` | None. |

## Bugfix audit

| # | Upstream fix | Rust disposition | Focused evidence | Remaining boundary |
|---:|---|---|---|---|
| 27 | Backbone EPOLL starvation and ingress timestamp | equivalent: Rust Backbone/TCP uses Tokio readiness and monotonic ingress accounting rather than Python EPOLL registration | `cargo test -p reticulum-rs-transport tcp_server`; interface traffic tests | Linux prepared-host listener soak remains separate. |
| 28 | Receipt-lock deadlock when callbacks send | equivalent: receipt map access is scoped and callbacks do not execute under its lock | receipt tests; strict Clippy and issue-369 scanner | None. |
| 29 | Path request, announce queue, and pending-link state edges | complete with explicit status predicates, batched request lifecycle, held-announce release, and pending-link rediscovery | `cargo test -p reticulum-rs-transport path_request`; `pending_out_link_rediscovery`; `held_udp_announce` | Multi-node soak is separate. |
| 30 | Link watchdog reset after receive exceptions | complete through explicit inbound/activity anchors and watchdog transitions | `cargo test -p reticulum-rs-transport watchdog` | Physical transport exceptions remain hardware-unverified. |
| 31 | Multi-segment Resource cancellation | complete: cancellation is propagated through segmented manager state and fragments are released | `cargo test -p reticulum-rs-transport resource` | Cross-implementation large-transfer cancellation remains separate. |
| 32 | Resource part-index alignment and rebinding | complete: segment-local/global indices and sender window rebinding are explicit | `cargo test -p reticulum-rs-transport resource` | Same cross-implementation boundary. |
| 33 | Stale BLE RNode device reference | complete in the software state machine: reconnect selects current peripheral state and clears failed sessions | `cargo test -p reticulum-rs-transport --test rnode_ble` | Physical BLE remains hardware-unverified. |
| 34 | Ratchet cleaning retained preservation | complete: cleaning removes expired/malformed records while enforcing the configured retained count | `cargo test -p reticulum-rs-transport ratchet` | None. |
| 35 | Invalid `rnstatus` statistics | complete: absent, non-finite, and malformed optional statistics render through bounded defaults instead of aborting | `cargo test -p rns-tools rnstatus` | None. |
| 36 | Various packet, link, and interface bugs | no standalone claim: covered only by the named packet/link/interface regressions and the full workspace suite | `cargo test --workspace --tests` | This umbrella row does not waive any named failure. |
| 37 | Per-interface burst count consistency | complete: Backbone parent snapshots aggregate child bytes, rates, violation counters, burst flags, and active announce/path-request limiter counts; global totals include only root interfaces to avoid double counting | `cargo test -p reticulum-rs-transport rns_1_5_parent_interface_reports_active_child_burst_counts`; `cargo test -p rns-tools rns_1_5_human_status` | None. |
| 38 | Windows `rngit` file resource operations | equivalent: Rust uses `PathBuf`/`fs` operations without POSIX separator assembly | `cargo test -p rns-tools rngit` | Windows hosted execution is platform-unverified in this Linux run. |
| 39 | `rnodeconf` WiFi-mode summary | complete: the selected numeric WiFi mode is preserved in command/status JSON rather than inferred from another flag | `cargo test -p rns-tools --test rnodeconf_cli` | Physical display confirmation remains hardware-unverified. |
| 40 | Speedtest abort on stale link | not applicable to a standalone shipped binary: this workspace has no Python-style speedtest example; shared Link predicates distinguish stale retry/teardown from active data exchange | link lifecycle predicate and watchdog tests | No speedtest compatibility claim is made. |
| 41 | Queue-tuning and discovery documentation | complete in active config, roadmap, parity matrix, and this ledger | documentation and pin consistency gates | Generated Python manual text is not copied into this repository. |

## Exact inventory and evidence boundary

The exact RNS 1.5.0/LXMF reference regeneration reports 1,839 entries: 1,838
complete, zero partial, zero unmapped, and one provenance-backed
not-applicable `CRNS` package. The 28 callables added since the previous pin
have explicit mapping rules; none is promoted by a wildcard alone.

Physical RNode/RNodeMulti, BLE, Weave, VR-N76, serial/radio, public-network,
and named third-party-client evidence remains separate. The release candidate
may be software-ready without claiming those axes.
