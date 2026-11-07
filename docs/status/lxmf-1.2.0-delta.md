# LXMF 1.2.0 compatibility delta

Issue: [#648](https://github.com/FreeTAKTeam/LXMF-rs/issues/648).
The baseline remains LXMF `727830cefda83d9c6e3982b48675425f3f988f9c`;
the additional target is `c3ff2d6dc2f256daab896dadc044dd5a913ecbb7`.
Both use RNS `99de23c040d507e3fefca19e87b182302902725d` for these fixtures.
The [pin-to-target comparison](https://github.com/markqvist/LXMF/compare/727830cefda83d9c6e3982b48675425f3f988f9c...c3ff2d6dc2f256daab896dadc044dd5a913ecbb7)
includes accumulated changes; this table does not attribute every row to 1.2.0.

| Upstream behavior | Rust owner and disposition | Evidence and remaining boundary |
| --- | --- | --- |
| Correct opportunistic serialized content budget: 287, removing the extra timestamp | `lxmf-core` constants and delivery selection; covered locally | Eighteen exact-pin size fixtures include title, fields and stamps. Core/runtime/daemon tests compare 398/399/400 full-wire bytes and mode selection. |
| Medium-aware request debounce, path and delivery waits, attempt limits, awaiting-path wakeups | Shared `DeliveryTiming`/`DeliverySchedule`, daemon path history and retained delivery owners; implemented locally | Exact measurement/decision fixtures, 66 Opportunistic snapshots and native discovery/retry tests pass. The additional 47-case/263-row Link adapter passes against the production admitted owner. Native watchdog and physical slow-interface reliability are separate claims. |
| `PN_META_IMPL_NAME = 0xFE` and string implementation/version metadata | `lxmf-core` announce encoder/parser and daemon announce producer; locally qualified | Old/new/unknown/malformed fixtures pass. Both clean mixed-peer matrices pass: target Python's `lxmd`/`1.2.0` is ingested, baseline missing values remain absent, and both Python peers decode Rust's actual `reticulumd`/`0.14.0` wire metadata. |
| Sequential outbound processing and recovery when notification callbacks fail | Existing retained scheduler and sole receipt worker; external `EventSinkBridge` notifications remain outside committed storage locks | Canonical public SDK/ZeroMQ qualification now passes two real send failures, external sink error/panic and subsequent signed native delivery through the same client. Status, replay and persisted terminal outcomes remain coherent. The sink is enabled via public configure; startup field propagation is not claimed. |
| Peer stamp minimum changes from the erroneous zero floor to `max(0, cost-flex)` | Existing peer owner uses `saturating_sub`; covered arithmetic | Focused minimum-cost regressions and Python fixtures cover underflow and positive floors. This does not establish autonomous network offers. |
| Missing Opportunistic packet receipt no longer raises while installing a callback | Retained Opportunistic owner handles no-route explicitly; covered bounded Rust behavior | Unused correlation is removed; pending status and the existing retry budget remain owned. Rust status vocabulary and cancellation are explicitly stronger than the Python callback behavior. |
| Python lxmd's default accepted delivery size drops from 1 MB to 1 KB | Mixed-peer Resource lane explicitly configures the same 1 MB receiving policy at both pins | An initial target run correctly rejected the 16 KB Resource under the new default. Configured Resource interoperability must not be described as matching every default limit. Rust default-limit alignment is not claimed by these four slices. |
| Per-message file writes gain a lock, unique temporary file, flush, best-effort fsync, replacement and cleanup | **Confirmed file-export gap:** `WireMessage::pack_to_file`/`pack_storage_to_file` and the lxmd inbound file sink still use direct writes | Daemon SQLite transactions and preparation/custody recovery do not prove atomicity of these file APIs. No atomic file-export parity claim. |
| Router cache/statistics/ticket/peer persistence uses replacement and locked snapshots | Existing `MessagesStore` writer and transactions own database-backed values; partial evidence | Do not duplicate Python cache filenames. Restart and fault tests must be mapped per value; the native custody tests cover dispatch/custody, not every Python cache. |
| Unpacking records the source identity's blackhole state; inbound router rejects before ticket processing/storage | **Confirmed inbound gap:** signature resolution does not check the resolved source identity against the blackhole owner | Ignored destination policy and announce/path blackhole filtering do not cover existing Links or locally decrypted propagation. |
| GROUP encryption description changes from AES-128 to AES-256 | Not applicable to the inspected SINGLE/Direct metadata; native Fernet already defaults to AES-256 | SINGLE and Direct correctly remain Curve25519. AES-128 requires its explicit feature. Generic GROUP descriptor parity is not claimed; this text delta does not require changing Link ciphers. |
| An unanswered autonomous-peer path request waits 7.5 seconds, then adds 720 seconds of backoff and clears `alive` | **Confirmed autonomous-peer gap:** management maintenance is a local RPC path | Claiming a management backoff before local `peer_sync` is not this path-wait/Link transition. No production autonomous outbound `/offer` caller is present. |
| Empty, filtered autonomous offers return before sending `/offer` | **Confirmed autonomous-peer gap** | Inbound offer handling, empty local synchronization results and `/pn/peer/sync` tests cannot establish suppression of an outbound network offer. |

`lxmf_1_2_0_link_traces.json` now contains 47 controlled Python scenarios and
263 snapshots. Five additions characterize creator-bound propagation signals,
including proof-before-retirement, retired creators, replacement Links and
malformed input. The production-owner adapter consumes every snapshot with declared inputs and
explicit monotonic-cancellation projections.
Actual Rust handshakes, packet/Resource proofs and blocked SQLite commits have
separate native tests; they are not Python interoperability results.

The retained native packet observer accepts late proofs after timeout/teardown
while its original key remains owned. Terminal SQLite state remains monotonic
after cancellation. Those are deliberate stronger Rust boundaries. The signal
decoder has a 32-container safety budget; deeper extra values are ignored and
leave the observer armed. These differences must remain visible in comparison
results instead of being counted as exact Python matches.

Clean candidate `5a0a30f55eac94f6bf5dcb7963f5be28771a8abd` passes both full
30-case matrices (23 live Python/Rust, seven local policy cases each), with no
skips or ignores. Reports and exact invocation/reference/environment records are
under `target/interop/item3/5a0a30f5/{baseline,lxmf-1.2.0}/matrix.json`.
Direct, Opportunistic and propagated messages pass in both directions. Native
Resource delivery and typed propagation custody are checked separately from
stored recipient content; a later negative terminal, event-stream gap or missing
native proof fails the evidence gate. The eight predeclared native input-barrier
stress iterations also pass all 47 scenarios/263 snapshots.

These results qualify the four approved slices, not every accumulated Python
change in this ledger. The default size, file-export, source-blackhole,
persistence and autonomous-peer boundaries above remain explicit. Workspace
tests and all-feature Clippy pass. Release-check reaches the existing published
ZeroMQ/UUID conflict after its software test stages; it is not a release pass.

The [roadmap](current-roadmap.md), [parity matrix](lxmf-parity-matrix.md) and
[execution contract](../goals/open-issues-2026-10/LINK-OWNER.md) remain authoritative.
Item 3 merged in [PR #673](https://github.com/FreeTAKTeam/LXMF-rs/pull/673)
at `9c27747753488ed950c52a2c7717ddec14478786`, after final-head review and
verification of 20 successful hosted checks and one deliberate publication skip.
Hosted baseline compatibility passes 30/30 and PR HIL passes 23/23 on first
attempts. The independent peer gate retains three known peer failures and two
dependency-blocked cases alongside 79 passes; it is not full peer conformance.
