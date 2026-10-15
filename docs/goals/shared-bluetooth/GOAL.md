# Progressive shared Bluetooth implementation

Approved contract: user-supplied final revised plan, 2026-10-15.

Goal: reusable Bluetooth mechanics without moving device protocols, authorization,
or host map ownership. Implementation proceeds in the priority order below.
Truth owners: native backend/session for transport; existing mobile SDK for its
serialized contract; consumers for device policy and protocol; REM for trusted
Binder grants, observer facts, drafts and map storage.

Cutover: additive infrastructure, then build-time REM pilots with old defaults.
Delete a displaced backend only after exact-device acceptance. Rollback restores
the tested release and compatible checksummed Rust/AAR pair. Never fall back
after a failed write or open two implementations against one device.

Forbidden: protocol dependencies in the leaf, automatic bearer switching,
reconnect write replay, implicit queue advancement, adapter reset/global unpair,
map clear-all, automatic goTenna/LXMF forwarding, fabricated support evidence,
private captures/identities/locations or proprietary SDK vendoring.

## Ordered task board

Main agent owns implementation; reviewers are read-only. Work is sequential.

| Task | Input / allowed files | Output / evidence | Dependency / status |
|---|---|---|---|
| 0a Baselines | repository metadata and reviewed first-party goTenna sources | exact refs, isolated worktrees, allowlisted local goTenna Git snapshot | active |
| 0b Leaf and checks | LXMF leaf, CI, policy and publication files | edition 2024, Rust 1.85, package reticulum-rs-bluetooth; isolated package/consumer locks | active |
| 0c SDK compatibility | opt-in lxmf-sdk adapter and tests | unchanged mobile DTOs/validators; Android/iOS fixture checks | 0b, pending |
| 1 REM RNode BLE | Android companion and REM JNI facade/build | pilot; protocol detection/readiness/settings stay in REM; real messages/receipts and cancellation/restart | 0, pending |
| 2 REM RNode Classic | companion stream and REM facade | EOF/idle/cancellation/mode-switch; stage 1 regression | 1, pending |
| 3 Direct Reticulum BLE #650 | daemon-private runtime extraction, Linux bluer client/server, RCH pins, Android roles | two peers, all roles, IFAC, Resources, isolated REM/RCH BLE and active RNode | 2, pending |
| 4 Binox plugin | ATN leaf adoption, transport; additive REM Binder map/draft API | trusted drafts/confirmation, observer grants, authorized device writes and display evidence | 3, pending |
| 5 goTenna plugin | audited new Git baseline, own codecs, REM SDK plugin | durable receive/advancement evidence, explicit text/location, coexistence | 4, pending |

Forbidden files: unrelated local changes in every original checkout; existing
btleplug implementations through stage 2; generated bindings and third-party
sources. No stage is parallel-safe for implementation. Later stages rerun prior
accepted cases. Software may progress without hardware, but promotion cannot.

## Package and platform contract

Directory/alias rns-bluetooth; package reticulum-rs-bluetooth; import rns_bluetooth.
Explicit edition 2024 and MSRV 1.85; host workspaces retain their own requirements.
Optional Linux bluer =0.17.4 coexists with btleplug 0.13. Protocol libraries consume
neutral interfaces; daemon/Android composition enables backends. Exact allowlist
edges accompany actual dependencies. Publish first in dependency order, using
LXMF-rs synchronized versions and licensing/package rules.

GattClient, GattServer and ClassicStream remain distinct. Preserve endpoint
identity, owner/session generation, native deadlines, bounded queues, idle/EOF
and ATT/characteristic/application capacity distinctions. Close wakes waiters;
late-open resources are cleaned; stale callbacks cannot mutate new sessions.
Dropping Rust futures does not cancel blocking JNI work.

Android companion is first-party, paired with Rust by version and SHA-256 before
Bluetooth I/O; no duplicate native libraries. New production Rust modules and
companion files/classes have a 500-line limit without new exemptions.

MobileBleCapabilities, MobileBleEvent, requests/results, validators and trait
signatures remain unchanged. Opt-in mobile-ble-adapter projects backend state,
selected characteristic pair, monotonic events and operation IDs into the old
contract. New native role information does not extend the strict SDK envelope.
RNode readiness thresholds remain outside the generic transport and #650 codec.

## Reference and adoption rules

#650 reference repository: https://github.com/torlando-tech/ble-reticulum
Pinned commit: 271b4ab8ba45a8c6a7864aef5919e77f07227fe7
Earlier Rust codec: 07d941304c9a1dc3a8e58087b3b974ff3d229e56
Deliberately behind main. Harness verifies the full commit and records Python
RNS/LXMF revisions. The LXMF-rs maintainer responsible for #650 owns a reviewed
advance: diff, fixtures, regressions, interop reacceptance and refreshed support
evidence. Dependency automation never advances the reference alone.

RCH updates its source/lock/CI/runtime baseline/packaged daemon together and
preserves ZeroMQ. ATN's first LXMF dependency is ONLY the leaf, full tested Git
rev, explicit features, defaults disabled, committed lock and matching AAR hash.
No sibling-path override or moving branch in consumer production manifests.

ATN keeps codecs, physical-semantic gates and authorization. Unknown units,
north/datum, observer relationship or firmware blocks conversion/writes.
Host-retained drafts, payload-bound idempotency, negotiated grants and scoped
subscriptions precede host-confirmed map publication. Existing immediate marker
creation cannot serve as draft submission. Reconnect grants no write authority.

goTenna preserves Python web server, Rust backend, frontend/API. Save reception
and advancement evidence durably before advancing; storage failure stops it.
No automatic advancement retry on reconnect; process death requires explicit
recovery, missing acknowledgements stay unconfirmed, duplicates retain evidence.

## Acceptance and kill criteria

Required software: strict fmt/Clippy/tests, boundary/feature/package/license/order
and source-size checks; normalized packaged crate + independent consumer on
1.85.0 with independent maintained locks; Android bridge target; mobile contract
and Android/iOS fixtures on PRs and full CI; Android unit/JNI/lint/build, Binder
instrumentation; Linux dependency-unification and supported packaging builds.
Desktop #614 regressions are required when that implementation changes.

Physical: exact phone/OS/firmware combination, real messages/receipts, partial
setup/permission/cancellation/service restart; #650 central/peripheral/dual roles,
two peers, announces/Links/Resources/small capacities/address rotation/adapter
loss/restart and isolated BLE-only REM/RCH; one RNode plus one plugin, separately
for each plugin and RNode mode, including plugin failure and process death.
Shared code does not promise controller-wide capacity across separate APKs.

Stop promotion on ownership drift, byte loss/replay, false completion, altered
SDK envelopes, missing reference verification, failed regressions or absent
hardware acceptance. Record implemented-but-unproven work in STATUS.md. Crate
completion does not close #614 or prove #650 interoperability.
