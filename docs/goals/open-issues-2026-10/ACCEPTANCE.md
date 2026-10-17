# Open-issue acceptance reconciliation

Snapshot: 2026-10-17, source `a8e500e88610210582e8dd8869df8b510e1e75fc`.
All **22** open GitHub issues were read, including discussions. The issue set was
independently counted with the REST issues endpoint excluding pull requests.
No implementation tests were run to produce this snapshot. This document is a
dated execution input; [current-roadmap.md](../../status/current-roadmap.md) and
the parity matrices remain authoritative. Recheck source and issue changes before
closing a requirement. Historical evidence is never a fresh candidate pass.

## Classification

- **I**: existing implementation or documented historical acceptance in the cited source; retain/regress it. It does not imply the whole issue is complete.
- **M**: missing implementation identified by source inspection or the maintained gap record.
- **E**: required acceptance evidence is missing, incomplete, or must be rerun on the candidate. Source presence alone does not satisfy it. For broad unchecked criteria, inspect the exact reference branches before deciding whether code is missing.
- **D**: explicitly deferred or rejected scope. “Outside 1–6” only excludes implementation from this goal, not from the repository backlog.
- Combined codes apply to different parts of a compound criterion. A checked GitHub box is recorded as inherited **I/E**, not copied as a new PASS.

## All-issue disposition

| Issue | Approved item | State | Reconciliation |
| --- | --- | --- | --- |
| [#244](https://github.com/FreeTAKTeam/LXMF-rs/issues/244) | 2 (subset) | I/M/E/D | Existing templates/checks provide parts of the proposal. Adopt #617/#618 lightweight subset; decline mandatory review tiers, size approval blocks and process overhead. |
| [#521](https://github.com/FreeTAKTeam/LXMF-rs/issues/521) | 5 | I/E | Source inspection shows constructor/verify/decrypt do not consume RNG. Finish a bounded audit/regression; no confirmed vulnerability. |
| [#522](https://github.com/FreeTAKTeam/LXMF-rs/issues/522) | 5 | I/M/E | TcpTxBuffers wire storage is already outside the loop; raw reuse is test-only and production encode_packet_ifac allocates. Measure and fix the real path. |
| [#523](https://github.com/FreeTAKTeam/LXMF-rs/issues/523) | 5 | M/E | Misspelled acquisition methods remain. Add compatible correctly spelled entry points and a deprecation path. |
| [#551](https://github.com/FreeTAKTeam/LXMF-rs/issues/551) | Outside 1–6 | D | Experimental optical QR remains a separate feature program, including external REM/Android work and two-device acceptance. |
| [#608](https://github.com/FreeTAKTeam/LXMF-rs/issues/608) | 4 | I/E | IFAC is active on production carriers; reconcile startup/rotation/inheritance/restart cells, not a new authentication stack. |
| [#609](https://github.com/FreeTAKTeam/LXMF-rs/issues/609) | 4 | I/M/E | Original local timing/parent classification and selected proof/restart cases are implemented. Deeper repeated delivery and remaining cache/context matrix stay open. |
| [#611](https://github.com/FreeTAKTeam/LXMF-rs/issues/611) | 6 | I/M/E | Real utility workflows exist. Remote rnstatus service/client is missing; complete all applicable frozen utility options, not only that first slice. |
| [#612](https://github.com/FreeTAKTeam/LXMF-rs/issues/612) | 6 | I/M/E | Resolver, storage, permission and selected work/network behavior exists; operation/role/scope/error/side-effect coverage is incomplete. |
| [#613](https://github.com/FreeTAKTeam/LXMF-rs/issues/613) | 6 | I/M/E | Pagination and substantial page/media cleanup exist. Remaining renderer/real-converter/real-peer-exit acceptance is not proven. |
| [#614](https://github.com/FreeTAKTeam/LXMF-rs/issues/614) | Outside 1–6 | I/M/E | Windows pairing and many native/software lifecycle cells exist. Remaining per-family/software-platform coverage belongs to original item 7. |
| [#616](https://github.com/FreeTAKTeam/LXMF-rs/issues/616) | Outside 1–6 | I/E | Retained REM/Sideband/Columba/RCH field evidence is bounded. Complete operational matrix remains separate; no hardware acceptance is newly claimed. |
| [#617](https://github.com/FreeTAKTeam/LXMF-rs/issues/617) | 2 | M/E | Live main protection returned 404 and repository rulesets were empty. Implement a small check/apply policy after the final CI gate exists. |
| [#618](https://github.com/FreeTAKTeam/LXMF-rs/issues/618) | 2 | I/M/E | Security/integration stages exist, but Full CI is dispatch-only; selected essential CI still depends on labels. Add automatic fail-closed selection. |
| [#620](https://github.com/FreeTAKTeam/LXMF-rs/issues/620) | Outside 1–6 | M/E | No mutation runner found in inspected tooling. Weekly bounded advisory automation belongs to original item 9. |
| [#624](https://github.com/FreeTAKTeam/LXMF-rs/issues/624) | Outside 1–6 | I/E | Nightly/release/lab-health workflows and HIL controller exist; no repository runner was reported. Permanent two-radio unattended operation remains unproven. |
| [#625](https://github.com/FreeTAKTeam/LXMF-rs/issues/625) | Outside 1–6 | I/M/E/D | Boundary checks exist. Host alloc-feature checks do not prove no_std. Designate a narrow real core and isolate bare-metal builds; broad lxmf-wire conversion remains deferred under #646. |
| [#646](https://github.com/FreeTAKTeam/LXMF-rs/issues/646) | Deferred | D | Maintainer's 2026-09-28 comment explicitly defers full no_std conversion. Current support declaration correctly states std is required. |
| [#648](https://github.com/FreeTAKTeam/LXMF-rs/issues/648) | 3 | I/M/E | Exact tag resolves to c3ff2d6. Extra timestamp remains in Rust payload budget; implementation-name metadata is absent. Scheduling/recovery need production-path comparison. |
| [#649](https://github.com/FreeTAKTeam/LXMF-rs/issues/649) | 4; rendering 6 | I/M/E | The qualified discovery planner, named management and bounded converted downloads already shipped. Daemon auto-connect, request-receipt states and live child-policy propagation remain missing. |
| [#650](https://github.com/FreeTAKTeam/LXMF-rs/issues/650) | Outside 1–6 | I/M/E | Codec/peer plumbing exists; native runtime still reports pending. Shared Bluetooth Stage 0 is not a native peer backend or consumer acceptance. |
| [#656](https://github.com/FreeTAKTeam/LXMF-rs/issues/656) | Outside 1–6 | I/M/E | RawKiss and ATT chunking exist. Standard service/profile discovery and characteristic handling plus hardware evidence remain; preserve Benshi default until promotion. |

## Criterion traceability

Identifiers `issue.ordinal` refer to checkbox order in the live issue body at
this snapshot, including its additional acceptance sections. Text preserves the
requirement before historical evidence annotations. Criteria without source
checkboxes are explicitly summarized. #244's sample PR-template checkboxes are
not accepted policy; its actual proposal is dispositioned as three groups.
Each section's evidence link applies to its rows. The original issue body and
source reference remain the full contract, including implementation paragraphs,
non-goals and details following the first sentence.

### #244: Proposal: PR & Issue Automation, Review Policy, and Contribution Guidelines

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/244) (updated 2026-09-21T12:48:12Z); [implementation/evidence](../../../.github/pull_request_template.md).
Owner paths: `.github templates/workflows; repository contribution documentation`. Approved item: **2 (subset)**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 244.1 | E | Adopt automatic relevant checks and native branch protection under #618/#617. |
| 244.2 | E | Keep useful contribution/issue templates and traceability; document the narrow adopted scope. |
| 244.3 | D | Do not adopt mandatory independent reviews, contributor tiers, size-L approval blocks or recurring manual management. Embedded template checkboxes are examples, not independently accepted requirements. |

### #521: Audit unnecessary OsRng use in private-key decryption

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/521) (updated 2026-09-21T12:48:14Z); [implementation/evidence](../../../crates/libs/rns-core/src/crypt/fernet.rs).
Owner paths: `crates/libs/rns-core/src/ratchets.rs; crypt/fernet.rs`. Approved item: **5**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 521.1 | I/E | Trace constructor, authentication and decrypt RNG behavior; preserve encryption randomness. |
| 521.2 | I/E | Simplify only if justified or document the unused generic RNG; regression proves decryption does not consume entropy. |

### #522: Reuse TCP transmit buffers across TX-loop iterations

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/522) (updated 2026-09-21T12:48:16Z); [implementation/evidence](../../../crates/libs/rns-transport/src/iface/tcp_client.rs).
Owner paths: `TcpTxBuffers; iface_parts/ifac_wire.rs; allocation/connection tests`. Approved item: **5**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 522.1 | I/M/E | Reuse TX storage in the actual production encoding path; preserve framing/authentication semantics. |
| 522.2 | I/M/E | Measure allocations and verify long/short/escaped frames and no stale bytes; distinguish existing test-only raw-buffer reuse. |

### #523: Correct misspelled PacketDataBuffer acquisition API

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/523) (updated 2026-09-21T12:48:18Z); [implementation/evidence](../../../crates/libs/rns-transport/src/packet.rs).
Owner paths: `PacketDataBuffer and all callers`. Approved item: **5**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 523.1 | M/E | Add acquire_buf/acquire_buf_max without breaking accuire_buf/accuire_buf_max callers. |
| 523.2 | M/E | Document deprecation/removal path; verify old/new behavior and length bounds. |

### #551: Add an Android optical QR Reticulum interface with full LXMF support

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/551) (updated 2026-09-21T12:48:22Z); [implementation/evidence](../../../crates/libs/rns-transport/src/iface.rs).
Owner paths: `optional optical bearer in rns-transport; REM owns Android camera/JNI/UI`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 551.1 | D | Opaque RNS packets over optional optical-qr; no optical HELLO/ACK/alternate LXMF protocol. |
| 551.2 | D | RQR1/RaptorQ/CRC/hash envelope, 500-byte MTU, bounded 8 incomplete transmissions and five-second expiry; duplicate/retransmission distinction. |
| 551.3 | D | Android CameraX/ZXing/JNI lifecycle and existing REM UI/API integration; no unsolicited cross-repository implementation. |
| 551.4 | D | Golden/invalid/lossy codec tests and bidirectional two-device announce/Link/LXMF/Resource/recovery acceptance; separate future implementation. |

### #608: Wire IFAC authentication into production carrier ingress and egress

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/608) (updated 2026-09-23T16:08:44Z); [implementation/evidence](../reticulum-reference-parity-605/evidence/608-ifac.md).
Owner paths: `crates/libs/rns-transport/src/iface_parts/ifac_wire.rs; crates/apps/reticulumd/src/bin/reticulumd/interface_hot_apply_parts`. Approved item: **4**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 608.1 | I/E | Two independently started Python/Rust nodes with matching credentials exchange announces, authenticated packets, proofs/links, and Resources in both directions through real software carriers. |
| 608.2 | I/E | Wrong credentials, tampered/truncated IFACs, invalid lengths and missing/unexpected authentication flags are rejected before routing; no plaintext fallback occurs on an authenticated interface. |
| 608.3 | I/E | Ordinary unauthenticated interfaces and shared-instance behavior retain their specified reference semantics. |
| 608.4 | E | Startup, configuration changes, child-interface inheritance, stop/restart and error reporting are covered. Reconfiguration never creates a transient authentication bypass. |
| 608.5 | I/E | Unit vectors are supplemented by daemon-level and mixed-peer regressions using the real production path, with counter/status assertions and bounded failure behavior. |
| 608.6 | I/E | Matrices and runtime capability reporting no longer claim completion before these scenarios pass; credentials remain redacted from logs and artifacts. |

### #609: Close transport, local-client announcement and shared-instance behavior gaps

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/609) (updated 2026-09-25T07:33:53Z); [implementation/evidence](../reticulum-reference-parity-605/evidence/609-transport-local-shared.md).
Owner paths: `crates/libs/rns-transport/src/transport; crates/apps/lxmf-cli/tests/python_lxmd_remote_relay_parts`. Approved item: **4**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 609.1 | I/E | Failing differential tests reproduce local-client timing and parent/child classification differences; fixes assert exact packet counts, routing destinations and bounded timing against the frozen Python behavior. |
| 609.2 | I/E | Exercise local client, shared daemon, parent/virtual child, transport-enabled/disabled and locally hosted destination combinations. Local delivery must not accidentally become transit forwarding. |
| 609.3 | I/E | Preserve PATH_RESPONSE ordering, cached versus scheduled announcements, duplicate suppression, held announcements, interface-mode policy, batching, throttle scopes and queue limits. |
| 609.4 | I/E | Verify startup/restart persistence, stale route expiry, cached-announce validity, link-establishment timeout and caller-visible close reasons where required by the reference. |
| 609.5 | I/E | Mixed Python/Rust shared-instance and multi-hop tests prove discovery, packets/proofs, link traffic and recovery after daemon replacement through production paths. |
| 609.6 | I/E | Every confirmed discrepancy discovered here has a regression and linked work item; no umbrella `complete` row overrides an unresolved behavior test. |
| 609.7 | E | Establish stable repeated bidirectional post-restart delivery, including reverse B-to-A DIRECT Resource delivery through deeper relay replacement. The current evidence records intermittent timeouts and explicitly does not claim broad delivery stability: [#609 evidence, lines 1004-1023](https://github.com/FreeTAKTeam/LXMF-rs/blob/28587bf46f12fa00c3ca3f0c76fba5925123f4f0/docs/goals/reticulum-reference-parity-605/evidence/609-transport-local-shared.md#L1004-L1023). |
| 609.8 | E | Complete the remaining packet/proof duplicate-filter and packet-hash cache matrix, including other retransmission classes and cache rotation/size behavior; current evidence covers only selected classes: [#609 evidence, lines 928-935](https://github.com/FreeTAKTeam/LXMF-rs/blob/28587bf46f12fa00c3ca3f0c76fba5925123f4f0/docs/goals/reticulum-reference-parity-605/evidence/609-transport-local-shared.md#L928-L935). |
| 609.9 | E | Complete the remaining routing, discovery, restart/recovery and shared-instance transport matrix with pinned-reference regressions. The existing PR evidence expressly records broader matrix behavior as open: [#609 evidence, lines 1008-1011](https://github.com/FreeTAKTeam/LXMF-rs/blob/28587bf46f12fa00c3ca3f0c76fba5925123f4f0/docs/goals/reticulum-reference-parity-605/evidence/609-transport-local-shared.md#L1008-L1011). |

### #611: Implement real network workflows for all reference Reticulum utilities

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/611) (updated 2026-09-25T07:32:00Z); [implementation/evidence](../reticulum-reference-parity-605/evidence/611-utilities.md).
Owner paths: `crates/apps/rns-tools/src/bin; crates/apps/reticulumd/src/bin/reticulumd`. Approved item: **6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 611.1 | E | Each utility has a behavior/option matrix and an implementation-backed test. No-op options, parser-only commands and unreachable helper APIs remain partial. |
| 611.2 | E | Rust client to Python service and Python client to Rust service work wherever the reference supports those roles, including binary-safe file transfers and real Git fetch/push/bundle workflows. |
| 611.3 | M/E | Implement the frozen Python rnstatus -R remote-management workflow over an authenticated Reticulum Link: a distinct rnstransport.remote.management destination, enable_remote_management gate, remote_management_allowed identity ACL, and compatible /status response fields. |
| 611.4 | I/E | Use at least two separate processes and isolated storage roots; local attached-node tests alone are insufficient. |
| 611.5 | E | Missing destination, invalid identity, denied operation, timeout, interrupted link, cancellation and disk errors produce accurate failure output and status rather than apparent success. |
| 611.6 | E | Slow-interface adaptive timeouts, shared-instance operation and restart behavior use the production transport. |
| 611.7 | E | Add migration/usage documentation and CLI transcript evidence for the frozen target; retain existing Rust conveniences without substituting them for reference functionality. |

### #612: Complete rngit permission resolvers, work-item operations and storage compatibility

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/612) (updated 2026-09-24T23:30:05Z); [implementation/evidence](../reticulum-reference-parity-605/evidence/612-rngit.md).
Owner paths: `crates/apps/rns-tools/src/bin/rngit_parts; crates/apps/rns-tools/tests/rngit_python_interop`. Approved item: **6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 612.1 | I/E | Node-owned executable `.allowed` resolvers are loaded and evaluated with reference-compatible output/permission semantics; remote clients cannot replace executable resolvers. Bound execution and output, preserve failures and document any intentional safety difference rather than silently calling it exact parity. |
| 612.2 | I/E | Group, repository and document permissions, configured-access merging, administrator inheritance, revocation, blocked identities and immediate permission refresh match tested reference behavior. |
| 612.3 | I/E | Concurrent updates have an explicit atomicity contract. Failed reads, execution or replacement do not silently broaden access or leave mismatched disk/in-memory state. |
| 612.4 | E | Create/propose/list/view/edit/comment/delete/complete/activate/permission operations match request shapes, authorization, scopes, metadata, error responses and side effects. Test denied, missing and malformed requests as well as success. |
| 612.5 | I/E | MessagePack fixtures preserve binary identity hashes, integer IDs, timestamps and other wire types. Read existing Rust representations only through a documented compatibility/migration path. |
| 612.6 | I/E | Canonical `.git.allowed`, `.git.work` and `.git.releases` naming works without collisions or unapproved migration. Test existing dotted-name data and ambiguous legacy sidecars. |
| 612.7 | I/E | Exercise Python-produced requests/data against Rust and vice versa where those formats are part of the compatibility contract, including restart and round-trip persistence. |

### #613: Implement rngit NomadNet pages, media delivery and link-scoped cleanup

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/613) (updated 2026-09-25T07:21:37Z); [implementation/evidence](../reticulum-reference-parity-605/evidence/613-rngit-pages.md).
Owner paths: `crates/apps/rns-tools/src/bin/rngit_parts; crates/apps/rns-tools/tests/rngit_python_interop`. Approved item: **6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 613.1 | I/E | NomadNet node setup, announcements, request-handler registration and page/file endpoints are reachable over real Reticulum links and enforce repository/document access. |
| 613.2 | I/E | Unidentified blocked clients receive the reference's `no_ident` behavior rather than private repository information; template customization and ordinary not-found/error behavior are covered. |
| 613.3 | I/E | `/media` request validation, repository/ref resolution, URL decoding, binary content and response filename metadata match reference fixtures. Cover missing keys, malformed paths, denied access, absent blobs and invalid refs. |
| 613.4 | I/E | Image markup and optional WebP conversion honor configuration and backend selection. Validate supported backend behavior against the pinned helper; use argument-safe process invocation, bounded work and accurate failures. |
| 613.5 | I/E | Disabled or unavailable conversion follows the specified raw-content behavior; already-compressed media is not unnecessarily auto-compressed by the Resource handler. |
| 613.6 | I/E | Active-link tracking, periodic stale-link cleanup and disconnect/error handling release temporary files and conversion processes without corrupting an active response. |
| 613.7 | I/E | Run a real pinned Python/NomadNet-compatible client against the Rust service. Record requests, metadata/content checksums and rendered-page fixtures where deterministic. |
| 613.8 | E | Complete reference page/rendering behavior: full Markdown highlighting, pagination, diff rendering, signed work-document presentation, and remaining template details. |
| 613.9 | E | Complete remote Link lifecycle and filesystem-failure coverage: verify timing over an actual Reticulum transport after a silently exited remote process, the remaining filesystem-failure paths, and full media lifecycle/cancellation behavior. |
| 613.10 | E | Complete declared-converter and visual conformance: verify real avconv encoding behavior, compare rendered output with the pinned reference, and exercise the required image corpus. A stubbed invocation/argv test is not real encoder evidence. |

### #614: Complete native interface runtimes and Windows BLE pairing/reconnect parity

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/614) (updated 2026-09-24T12:10:22Z); [implementation/evidence](../reticulum-reference-parity-605/evidence/614-native-interfaces.md).
Owner paths: `crates/libs/rns-transport/src/iface; crates/apps/reticulumd/src/bin/reticulumd/interfaces`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 614.1 | I/E | Compare the reference's Windows paired-device/address acquisition with the selected Rust BLE backend. Implement the missing behavior or prove equivalent behavior using hosted/native tests; do not claim equivalence solely from normalized address strings. |
| 614.2 | I/E | Verify pairing selection, stale device references, partial connect/service discovery, EOF versus idle, detection timeout, bounded cleanup, cancellation and reconnect; retain platform-owned mobile bearer semantics from #604. |
| 614.3 | E | Enumerate all reference interface families. For each, trace config -> live carrier -> packet receive/transmit -> status -> teardown; record and implement every missing software step. |
| 614.4 | I/E | Test AutoInterface library and daemon startup, real link-local enumeration, socket reuse, per-device carrier loss and stop/restart socket release. Integrate existing PR #603 only through normal review. |
| 614.5 | E | Cover TCP/Backbone, local TCP/Unix shared instances, UDP, Pipe, serial/KISS/AX.25, RNode/RNodeMulti, Weave, I2P and any other family actually present in the frozen reference. Rust-only extensions remain outside the reference requirement set. |
| 614.6 | E | Run feature/platform combinations on Linux, Windows and macOS plus the declared mobile targets where applicable; unavailable platform tests are reported as missing evidence, not passed. |
| 614.7 | E | Telemetry, queue bounds, recovery and cleanup are reachable through production library/daemon surfaces. Unknown/unsupported kinds fail explicitly rather than being silently dropped. |

### #616: Complete physical-carrier, platform, client and soak evidence matrix

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/616) (updated 2026-09-26T19:37:15Z); [implementation/evidence](../../status/real-world-interoperability.md).
Owner paths: `tests/hil; xtask/src/hil; existing prepared-host tooling`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 616.1 | I/E | **Physical RNode/LoRa:** REM 1.4 used LXMF-rs revision `56ea9f06c474426b2245739e9cb5e2c325cdb1e2` on POCO + Pixel at 915 MHz and demonstrated bidirectional announces, Links, small LXMF, a 1 KiB Resource, mission events, telemetry, and SOS activation/cancellation. |
| 616.2 | I/E | **Physical two-phone TCP:** REM 1.2.7 RC1 recorded mutual announce/discovery, connection, bidirectional acknowledged chat, application replication, telemetry, SOS, and restart recovery on Pixel 7 + Samsung S8. |
| 616.3 | I/E | **Sideband real-app TCP HIL:** commit `cc756672e008d24390047cec02a41196f2c8effe` records two Sideband phones reaching `Delivered` in both directions through `reticulumd`; later RCH/Sideband HIL exposed and fixed a real Resource wire-compatibility defect. |
| 616.4 | I/E | **Columba direct LXMF:** commit `cb3865378ff77fc81a33280ac6423d8905b497ef` records the external-client gate passing in both directions against clean Columba `v2.0.9-beta` (`3738bd7834128023db491cca0876585b799de942`). |
| 616.5 | I/E | **RCH live path:** commit `133dd162da59ddf191c8dee7bb2d8ae9e344c649` and the RCH live stress report record real RCH/reticulumd/REM traffic. Some RCH/Pixel 8a paths were only partially verified, so this is not a blanket pass for every RCH workflow. |
| 616.6 | E | Exercise paired RNode serial, TCP/Wi-Fi and BLE carriers with recorded model, firmware, MCU, OS/adapter, endpoint configuration and exact Rust/Python commits. Include both traffic directions, discovery, proofs, links, small/large payloads, reconnect and safe management. |
| 616.7 | E | Run reference-supported RNodeMulti/vport and Weave workflows on actual required hardware, preserving device-specific evidence rather than generalizing from one radio. |
| 616.8 | E | Confirm native Linux/Windows/macOS and declared mobile behavior on the appropriate platform; Windows pairing evidence cannot be inferred from a Linux address-normalization test. |
| 616.9 | E | Validate required real I2P/network scenarios separately from fake-SAM evidence. Define bounded soak duration, topology, offered load, recovery criteria and resource-use limits before each test. |
| 616.10 | E | Exercise the named external clients selected by #606, pinning their versions and testing only clearly identified RNS/LXMF integration workflows. Do not claim all third-party clients from one successful application. |
| 616.11 | E | Collect machine-readable reports, checksummed raw logs/captures, timestamps, topology, identity/device provenance and exact scenario outcomes. Redact credentials and personal/private traffic. |
| 616.12 | E | Missing equipment, disconnected peers, unavailable runners or incomplete manifests produce blocked/unverified results, never a pass. A changed candidate requires rerunning affected evidence. |
| 616.13 | E | Publish an operational support statement whose scope exactly matches verified rows. Keep software parity and operational validation separately visible in the roadmap and release evidence. |

### #617: Automate lightweight main-branch protection without mandatory human approvals

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/617) (updated 2026-09-26T19:37:21Z); [implementation/evidence](../../../.github/workflows/ci.yml).
Owner paths: `GitHub native branch protection; .github/workflows/ci.yml; existing repository tooling`. Approved item: **2**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 617.1 | M/E | Configuration verification distinguishes correct, missing, conflicting and inaccessible settings; `apply` is explicit and repeatable without duplicate rules. |
| 617.2 | M/E | Automated tests prove that failing/cancelled/missing required checks prevent a successful gate, while a passing relevant test set is merge-eligible without an additional human approval requirement. |
| 617.3 | M/E | Fork PRs, docs-only PRs and superseded runs do not become permanently pending or accidentally green. |
| 617.4 | M/E | Existing maintainer recovery access is documented in a short setup note; GitHub's native audit history is sufficient. No recurring manual audit/checklist is introduced. |
| 617.5 | M/E | Enabling this does not change merge strategy, require every optional HIL/nightly job, enable auto-merge globally, or rewrite unrelated organization settings. |

### #618: Run security and relevant integration checks automatically with bounded CI cost

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/618) (updated 2026-09-26T19:37:37Z); [implementation/evidence](../../../.github/workflows/ci-full.yml).
Owner paths: `xtask/src/main_parts/run_ci_stage.rs; .github/workflows/ci.yml; .github/workflows/ci-full.yml`. Approved item: **2**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 618.1 | M/E | Selector tests cover core/transport, SDK, CLI, shared fixtures, manifests, workflow changes, docs-only, renamed/deleted files and unavailable diff data. Uncertainty broadens tests instead of silently skipping them. |
| 618.2 | M/E | Required-check tests prove a failed or missing security/integration run cannot become a green summary. Infrastructure failures are reported as failures, not compliance successes. |
| 618.3 | M/E | Security and selected integration jobs start without a maintainer label or checklist. Duplicate runs across workflows are removed or reused. |
| 618.4 | M/E | Use existing caching, concurrency cancellation and explicit timeouts. Report observed runtime; aim for a cached ordinary PR path of approximately 15 minutes, but do not remove required tests to meet the target. |
| 618.5 | I/E | Full multi-platform, large-transfer, long-soak and hardware lanes remain on existing appropriate nightly/release/explicit triggers. No HIL requirement on every PR. |
| 618.6 | M/E | Results and logs are GitHub job summaries/artifacts; no new dashboard service, recurring manual report, or repeated bot comments/issues is introduced. |

### #620: Add bounded, automated mutation testing for critical protocol paths

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/620) (updated 2026-09-21T12:49:00Z); [implementation/evidence](../../../xtask/src/main_parts/run_ci_stage.rs).
Owner paths: `xtask; .github/workflows`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 620.1 | M/E | The runner demonstrably detects a seeded incorrect validation/state-transition change in a test fixture; a broken harness or zero executed tests cannot appear successful. |
| 620.2 | M/E | Results separately identify killed, surviving, unviable, timed-out, skipped and not-executed mutations. Timeouts are not counted as proof that tests detected a bug. |
| 620.3 | M/E | GitHub summaries and downloadable JSON include exact source/toolchain/tool versions, selected scope, commands, elapsed time and sample sizes. Any score states its denominator and exclusions. |
| 620.4 | M/E | Exceeding the budget stops cleanly and reports incomplete evidence without weakening the ordinary test/release gates. |
| 620.5 | M/E | The scheduled run and local reproduction need no manually maintained per-PR baseline, external service, new dashboard or special maintainer label. |
| 620.6 | M/E | Actionable survivors become ordinary test improvements; equivalent mutations are not automatically declared defects. No issue/comment is created for every survivor or every repeated run. |
| 620.7 | M/E | Reuse existing measured coverage reports when available; do not substitute test counts or mutation sample counts for line/branch coverage. |

### #624: Automate permanent RNode/LoRa HIL from existing field evidence

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/624) (updated 2026-09-26T19:37:17Z); [implementation/evidence](../../../.github/workflows/hil-nightly.yml).
Owner paths: `xtask/src/hil; tests/hil; .github/workflows/hil-nightly.yml; .github/workflows/hil-release.yml`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 624.1 | E | A permanently connected two-RNode topology runs unattended. |
| 624.2 | E | Hardware absence/disconnection reports BLOCKED/INFRA FAILURE, never PASS. |
| 624.3 | E | Rust↔Rust and Python↔Rust physical exchanges are represented where applicable. |
| 624.4 | E | >=3-node topology automatically enables multi-hop/contention tests when available. |
| 624.5 | E | Test manifests capture exact software, firmware, device and RF configuration. |
| 624.6 | E | A failed scenario can be reproduced locally from the emitted command/configuration. |
| 624.7 | E | Results are retained as artifacts and consumed by #616 operational evidence. |
| 624.8 | E | Runner locking prevents simultaneous jobs from corrupting results. |
| 624.9 | E | No unattended flashing, erase or destructive hardware management is introduced. |

### #625: Enforce a clean no_std protocol core with automated dependency-boundary checks

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/625) (updated 2026-09-21T12:49:06Z); [implementation/evidence](../../../xtask/src/main_parts/run_embedded_link_check.rs).
Owner paths: `tools/scripts/check-boundaries.sh; Cargo.toml; embedded crate feature graph`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 625.1 | E | Designated core crates build with `--no-default-features` or the repository's explicit no_std feature contract. |
| 625.2 | E | CI fails if a designated core crate gains an unauthorized std-only/application/runtime dependency. |
| 625.3 | E | CI fails on a seeded architecture-boundary violation, proving the checker is effective. |
| 625.4 | E | Dependency checks account for optional/dev/build dependencies and Cargo feature unification. |
| 625.5 | E | Protocol APIs do not expose daemon, CLI, SDK transport, TAK/CoT, ZeroMQ or Tokio-specific types. |
| 625.6 | E | Time/entropy/storage/I/O dependencies needed by core logic are supplied through narrow abstractions where necessary. |
| 625.7 | E | Embedded builds do not require application configuration or host filesystem/network services. |
| 625.8 | E | Architecture documentation identifies the authoritative core boundary and dependency direction. |
| 625.9 | E | Existing desktop/server/application capabilities continue to work through outer adapters. |

### #646: `lxmf-wire` pulls in `std` transitively via its dependencies

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/646) (updated 2026-09-28T23:33:23Z); [implementation/evidence](../../contracts/sdk-v2-feature-matrix.md).
Owner paths: `crates/libs/lxmf-core/Cargo.toml; crates/libs/lxmf-core/src/lib.rs`. Approved item: **Deferred**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 646.1 | D | Keep the std requirement and alloc compatibility flag truthful; no-default-features is not a bare-metal claim. |
| 646.2 | D | Full conversion requires compatible MessagePack values/serialization, transitive std removal, supplied secure entropy, isolated target CI and wire compatibility; explicitly deferred. |

### #648: Assess and integrate upstream LXMF 1.2.0 changes

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/648) (updated 2026-09-30T13:48:14Z); [implementation/evidence](../../../crates/libs/lxmf-core/src/constants.rs).
Owner paths: `crates/libs/lxmf-core/src/message/delivery.rs; crates/libs/lxmf-core/src/announce.rs; daemon delivery/router workers`. Approved item: **3**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 648.1 | E | Resolve tag 1.2.0 to an immutable commit and record a delta table: upstream change, Rust owner, already covered / confirmed gap / not applicable, and test evidence. Include accumulated pin-to-release changes without claiming all were introduced in 1.2.0. |
| 648.2 | M/E | Trace all consumers of the opportunistic size constants, correct the extra timestamp where applicable, and compare Python/Rust packing and mode selection at the exact effective limit, one byte below and one byte above. Include title/fields/stamp overhead; do not equate a raw text length with total serialized content. |
| 648.3 | E | Compare medium-aware scheduling against the pinned release. Test slow and normal links, absent and recovered paths, repeated path requests, retry exhaustion, and direct/opportunistic/propagated delivery. Verify timely announce wakeups without duplicate sends or request storms. |
| 648.4 | M/E | Add the implementation-name constant and production announce metadata where missing. Emit the actual Rust implementation/version, not Python's identity. Add Python-generated fixtures for string metadata, older metadata, unknown keys and malformed values; exercise propagation announce exchange. |
| 648.5 | E | Exercise simultaneous outbound triggers, a failing callback, a failed send and later successful delivery. Verify processing locks/queues recover and terminal callbacks are not duplicated. |
| 648.6 | E | Run focused tests plus Python↔Rust direct, opportunistic and propagated regression scenarios on exact recorded commits. Update the parity/delta documentation with measured results and remaining gaps. |

### #649: Assess and integrate upstream Reticulum 1.5.5 changes

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/649) (updated 2026-10-03T12:56:54Z); [implementation/evidence](../../status/rns-1.5.5-delta.md).
Owner paths: `crates/libs/rns-transport/src/discovery; daemon interface lifecycle; transport Link; rngit rendering`. Approved item: **4; rendering 6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 649.1 | I/E | Map the operations onto existing daemon/RPC/CLI and transport lifecycle surfaces; identify actual gaps before adding parallel APIs. Implemented on existing paths in [#651](https://github.com/FreeTAKTeam/LXMF-rs/pull/651); remaining carrier and monitoring gaps are recorded in the [delta ledger](https://github.com/FreeTAKTeam/LXMF-rs/blob/main/docs/status/rns-1.5.5-delta.md). |
| 649.2 | E | Test attach, repeated detach, reload, unknown interface, failed configuration and reconnection. Verify workers/sockets are cleaned up, unrelated traffic continues, and automatic monitoring does not resurrect a manually detached interface. |
| 649.3 | E | Check configuration inheritance, including `internal_forced`, across reload and spawned interfaces. |
| 649.4 | E | Compare discovery decoding, persisted metadata, CLI display and production admission policy with the exact release. Test accepted/rejected/missing/malformed versions and implementation names, stale entries, duplicate names and the explicit override. |
| 649.5 | E | Test both discovery directions with a Python 1.5.5 peer. Record whether Python auto-connects to Rust's honestly advertised implementation/version under default and override settings; do not impersonate Python to pass its filter. |
| 649.6 | E | Verify Windows/macOS carrier selection, manual-detach coordination, empty/non-string IFAC fields, previously persisted `"None"` values and I2P snippet suffix handling. Keep hardware evidence separate. |
| 649.7 | M/E | Trace the corresponding Rust production paths and add focused regressions for timeout and response rejection before/after delivery acknowledgement, pending-request removal and one terminal callback. |
| 649.8 | E | Add boundary tests for ingress-control watermarks, announce-cap percentage-to-fraction conversion and inherited child properties. |
| 649.9 | E | Exercise immediate incoming traffic during shared/local-client initialization and reconnect, checking ownership and callback readiness. Reuse relevant #608–#610 evidence rather than reopening completed cells. |
| 649.10 | E | Compare current `rns-tools` rngit page generation and response paths. Add exact-reference tests for converted download content/type, relevant filter counts and empty/error cases. Link residual work to #612/#613 where it overlaps. |
| 649.11 | E | Record immutable release commit, Rust owner, disposition and evidence for each delta. Changes already covered by #605 need links, not duplicate implementation. |
| 649.12 | E | Run focused regressions and exact-reference Python↔Rust request/response, discovery, interface lifecycle and affected LXMF delivery checks. Record commands, commits and platform coverage. |
| 649.13 | E | Update the delta/parity documentation with measured scope and unresolved items. Promotion of the canonical release pin requires the existing release gates; physical/platform acceptance remains separately bounded under #616. |

### #650: Complete native BLE peer interface for ble-reticulum interoperability

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/650) (updated 2026-09-30T14:05:08Z); [implementation/evidence](../shared-bluetooth/STATUS.md).
Owner paths: `crates/libs/rns-bluetooth; crates/apps/reticulumd/src/bin/reticulumd/interfaces/reticulum_ble_parts`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 650.1 | I/M/E | Connect the existing adapter to real Linux/BlueZ scanning, service discovery, GATT client writes/subscriptions, advertising, and GATT server reads/writes/notifications. Support configurable central-only, peripheral-only and dual-role operation. |
| 650.2 | I/M/E | Preserve the v2.2 GATT shape: service `37145b00-442d-4a94-917f-8f42c5da28e3`, TX READ/NOTIFY ending `28e4`, RX WRITE/WRITE_WITHOUT_RESPONSE ending `28e5`, and identity READ ending `28e6`. Discover by service UUID, without requiring an advertised identity/name. Read the peripheral's 16-byte identity and send the central's 16-byte identity as the initial RX write. |
| 650.3 | I/M/E | Reconcile role selection against the pinned implementation. The Rust core currently resolves duplicate roles by stable identity ordering, while the Python interface contains MAC-based initiation policy and MAC-rotation recovery. Test simultaneous connection attempts, asymmetric discovery, rotated addresses, stale/duplicate identities, and MTU/identity callback races instead of assuming those policies are equivalent. |
| 650.4 | I/M/E | Wire complete inbound packets into transport and outbound packets to the correct peer sessions, preserving IFAC and peer lifecycle behavior. Use per-peer negotiated write/notification limits; separate Reticulum packet MTU, ATT MTU, and characteristic payload capacity. A small BLE transfer limit must trigger fragmentation rather than reject a valid 500-byte Reticulum packet. |
| 650.5 | I/M/E | Reuse and verify the five-byte big-endian `type:u8, sequence:u16, total:u16` codec (START/CONTINUE/END = 1/2/3), including single-fragment packets. Bound queues, packet/reassembly sizes and timeouts; prevent fragments from concurrent packets on one peer from interleaving, since this header has no packet ID. |
| 650.6 | I/M/E | Implement bounded connection attempts, backoff, keepalive/control-byte handling, periodic stale-buffer cleanup, disconnect recovery, adapter loss and cancellation. Honor connection limits and RSSI/scan configuration, and release advertising, subscriptions, services and workers on stop/restart. |
| 650.7 | I/M/E | Report actual backend/peer state, send failures and successful radio submissions accurately. Unsupported platforms and unavailable adapters must be explicit; counted fragments alone must not imply transmitted traffic. |
| 650.8 | I/M/E | Document a working configuration, Linux dependencies/permissions, pairing behavior and troubleshooting. Treat identity exchange as peer identification rather than authentication. Preserve Reticulum security and avoid automatically weakening global Bluetooth security settings. |
| 650.9 | E | Deterministic tests cover both directions, handshake/control/data separation, MTU boundaries, full-size Reticulum packets, malformed/truncated/oversized/conflicting fragments, timeout cleanup, queue pressure and reconnect/teardown. Compare encoded bytes with fixtures generated from the pinned Python codec. |
| 650.10 | E | Backend tests exercise production RX/TX paths and multi-peer isolation, not only the standalone codec. |
| 650.11 | E | On authorized physical BLE devices, demonstrate Rust↔Python discovery, announces, Link establishment, bidirectional LXMF messages/receipts and a Resource transfer large enough to require multiple packets. Exercise Rust as central and peripheral, then dual-role operation and reconnect after interruption. |
| 650.12 | E | Exercise at least two concurrent peers for the supported multi-peer claim, retaining per-peer delivery and cleanup evidence. |
| 650.13 | E | Retain exact Rust/Python revisions, dependency versions, OS/BlueZ/adapter details, configuration, scenario results and redacted logs through the existing HIL/evidence tooling. Missing hardware is reported as blocked/unverified, never passed. |
| 650.14 | E | Update the runbook/support matrix with the precise verified platform and role scope, and run regressions for the existing BLE interface families. |

### #656: VR-N76: use direct BLE-KISS as the Reticulum data plane

Source: [issue and discussion](https://github.com/FreeTAKTeam/LXMF-rs/issues/656) (updated 2026-10-06T12:49:41Z); [implementation/evidence](../../runbooks/reticulumd-vrn76-kiss-ble.md).
Owner paths: `crates/libs/rns-transport/src/iface/vrn76_kiss_ble_parts; existing KISS codec`. Approved item: **Outside 1–6**.

| Criterion | State | Required behavior/evidence |
| --- | --- | --- |
| 656.1 | I/M/E | Reuse the existing KISS codec. Do not duplicate KISS framing logic. |
| 656.2 | I/M/E | Add a direct standard BLE-KISS frame mode/profile to the existing VR-N76 BLE interface. |
| 656.3 | I/M/E | Discover the standard BLE-KISS service by UUID. |
| 656.4 | I/M/E | Subscribe to the radio→host notification characteristic before declaring the interface ready. |
| 656.5 | I/M/E | Send outbound KISS bytes directly to the host→radio write characteristic. |
| 656.6 | I/M/E | Preserve KISS stream state across BLE notification/write boundaries. A KISS frame may span several BLE transfers, and one BLE transfer may contain multiple KISS frames. |
| 656.7 | I/M/E | Size each BLE write from the negotiated ATT payload instead of assuming one fixed write size. |
| 656.8 | I/M/E | Keep Reticulum packets opaque. Do not parse or create AX.25 frames in the Reticulum transport. |
| 656.9 | I/M/E | Preserve existing reconnect, timeout, queue and runtime-status behavior where applicable. |
| 656.10 | I/M/E | Keep the existing Benshi path as an explicit compatibility/fallback mode until direct BLE-KISS has real hardware evidence. |
| 656.11 | I/M/E | Do not require Benshi merely to carry Reticulum packet data when the standard BLE-KISS service is available. |
| 656.12 | I/M/E | Reticulum packet → KISS → direct BLE write, with no Benshi bytes present. |
| 656.13 | I/M/E | BLE notification → KISS decoder → identical Reticulum packet. |
| 656.14 | E | KISS frame split across multiple notifications. |
| 656.15 | E | Multiple KISS frames in one notification. |
| 656.16 | E | Escaped `FEND` and `FESC` across BLE transfer boundaries. |
| 656.17 | E | Negotiated ATT payload smaller than the encoded KISS frame. |
| 656.18 | E | Disconnect/reconnect clears stale partial-frame state. |
| 656.19 | E | Fake backend verifies service/characteristic discovery and subscription ordering. |
| 656.20 | E | Existing Benshi tests continue to pass. |
| 656.21 | E | Confirm the standard BLE-KISS service is exposed by the tested firmware. |
| 656.22 | E | Record firmware version, OS, BLE adapter and actual GATT characteristic properties. |
| 656.23 | E | Confirm the host→radio and radio→host characteristic directions on hardware. |
| 656.24 | E | Exchange Reticulum announces in both directions. |
| 656.25 | E | Run `rnprobe` in both directions. |
| 656.26 | E | Establish a Reticulum Link. |
| 656.27 | E | Exchange bidirectional LXMF messages. |
| 656.28 | E | Transfer a Resource large enough to exercise multiple KISS/BLE chunks. |
| 656.29 | E | Interrupt Bluetooth and verify reconnect and subsequent traffic. |
| 656.30 | E | Compare direct BLE-KISS against the current Benshi path for reliability and host-side overhead. Do not claim higher RF bitrate unless measured over the air. |

## Finite closure inventory for in-scope software work

The broad issue headings must be evaluated against the following enumerated
axes. For each applicable reference branch, record an existing or new test and
its level; a cell may be not applicable only with a pinned-source reason. A
single successful case does not discharge the other values. Detailed runnable
case manifests belong beside the existing tests in the owning implementation PR,
not in a second independent status database.

| Item | Finite axes to account for | Completion boundary |
| --- | --- | --- |
| 2 / CI | core/transport, SDK, CLI, shared fixture, Cargo manifest/lock, workflow, contract-doc, prose-doc, rename, delete, unclassified, missing diff; fork/main PR/push/schedule; success/failure/cancelled/missing/skipped/superseded | Required final result always reports accurately; verified native protection after explicit apply. |
| 3 / LXMF | below/at/above serialized opportunistic limit; title/fields/stamp overhead; old/new/unknown/malformed metadata; slow/normal bearer; path absent/recovered; simultaneous send/callback failure/send failure; direct/opportunistic/propagated | Exact LXMF 1.2.0 fixtures and production mixed-peer flows, preserving baseline regressions. |
| 4 / IFAC | TCP/Backbone including accepted children, UDP, local TCP/Unix exceptions, Pipe, serial/KISS/AX.25, RNode/RNodeMulti, Weave, I2P and other applicable frozen-reference families; startup/config/rotation/child/restart/failure; authenticated/plain/mismatched/tampered/truncated | Software carriers/fakes/PTY plus mixed-peer applicable cases. Physical equipment remains #616. Existing narrower proofs are reused explicitly. |
| 4 / transport | every frozen packet type/context and proof duplicate branch; shared owner/client/virtual child/ordinary interface; transport enabled/disabled; cached/scheduled/held announce; cache capacity/rotation/persistence; fresh/stale/malformed/blackholed routes; relay replacement and both directions | Exact reference branch inventory and production ingress tests; predeclared repeated DIRECT/Resource recovery with retained failed runs. |
| 4 / RNS 1.5.5 | attach/detach/reload/error/rollback/reconnect; initial/live inherited policy; accepted/rejected/missing/malformed implementation/version; duplicate names; persisted legacy metadata; default/override; manual detach; request SENT/DELIVERED timeout/rejection/removal/one terminal result | Real daemon auto-connect and peer request flows, not library planner or queue admission alone; platform-specific software checks remain explicit. |
| 6 / utilities | rncp, rnpath, rnprobe, rnstatus, rnsd, rnid, rnir, rnodeconf, rnpkg, rnsh, rnx, rngit; all frozen options and observable branches; both supported peer directions; missing/invalid/denied/timeout/interruption/cancel/disk/restart; slow software and shared instances | Existing utility table in evidence/611-utilities.md is the starting inventory. Exact reference no-ops remain no-ops; physical radio operations are not falsely passed. |
| 6 / work | create/propose/list/view/edit/comment/delete/complete/activate/perms; anonymous/blocked/reader/writer/interactor/author/document-admin/repository-admin; group/repository/document; active/proposed/completed/all; absent/malformed/zero/valid ID; absent/corrupt/valid storage | Applicable combinations follow pinned dispatch gates, including wire types, side effects, persistence, rollback and real Links; undefined same-document cross-process races are not invented reference guarantees. |
| 6 / pages/media | front/repository/tree/commit/diff/history/work/template/media/download; 1000-entry tree and 100-commit pagination boundaries; highlighting/signed work; readable/denied/missing/malformed; converter disabled/missing/failing/working; ffmpeg/avconv/convert/magick/gm; source/output/temp filesystem faults; active/cancel/disconnect/silent process exit | Reference output comparisons, real declared converters and corpus, live cleanup timing. Initial corpus must cover static/animated, transparent/opaque, small/large, supported/unsupported/corrupt media with pinned expected output or documented reference fallback. |

## Evidence precedence and drift

- #608's original “unwired” gap is obsolete; do not rebuild IFAC.
- #649's old permissive-planner description is obsolete; the daemon worker is the missing connection to production.
- #613 pagination is implemented. Historical converter prose contains older “unverified” statements beside later dated successes; use exact dated artifacts and current code, preserving the original evidence scope.
- #612 retains explicit security differences: fail-closed invalid permissions, resolver execution/output bounds and no document-ID-zero read bypass. Completion never requires copying an authorization weakness.
- #611 includes more than remote status. Check the frozen resolver/package utility before inventing work; no-op reference behavior is not a missing runtime.
- #522 production IFAC encoding returns owned buffers despite reuse in the test helper; benchmark the actual TX path.
- Current callable inventory completeness is scoped to its pin and cannot override unresolved forward behavioral requirements.
