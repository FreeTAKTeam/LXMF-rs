# #606 finite support and verification matrix

Status: **declared; operational rows NOT RUN / excluded from this software
goal**. This is the finite verification inventory for acceptance item 6, not a
platform-support certification and not evidence that physical or external
client runs passed.

The forward behavior target is Reticulum-Python
`99de23c040d507e3fefca19e87b182302902725d` (1.5.4-dev). The pinned Python
checkout used for source inspection resolved to that exact revision. Platform
names below come from repository workflows and target-gated code; carrier
names come from the daemon interface inventory and configured HIL profiles;
external peer identities come from `tools/interop/independent-implementations.toml`
and `.github/workflows/independent-interop.yml`.

## Platform build and runtime boundary

These are the finite hosted OS families currently named by interface CI.
They are workflow coverage rows, not a declaration that every interface has
runtime parity on each OS. `*-latest` runner labels and the Linux
`ubuntu-24.04` runner do not pin a Rust target triple or CPU architecture;
therefore no architecture is inferred or certified here.

| Row | Scope and observable claim | Verification command / evidence | Current evidence status | Owner |
| --- | --- | --- | --- | --- |
| Linux hosted | Daemon interface targets compile on the Linux hosted runner; this does not prove carrier runtime behavior. | `.github/workflows/ci-full.yml` `interfaces-build-linux`: `cargo check -p reticulumd --all-targets`; regular CI `build-matrix` is Linux hosted. | Linux hosted checks passed on PR #627 head `9832268f9d42811027cb75dc68b1f3e4fbfd395b`; build/test only. CPU architecture is not recorded by the workflow contract. | #606 contract; interface gaps #614 |
| macOS hosted | Daemon interface targets compile on the macOS hosted runner; no live interface behavior is implied. | `.github/workflows/ci-full.yml` `interfaces-build-macos`: `cargo check -p reticulumd --all-targets`. | Workflow row exists but is conditional on full-CI dispatch/label; no macOS runtime result is recorded. Architecture is not pinned. | #606 contract; #614 |
| Windows hosted | Daemon interface targets compile on the Windows hosted runner; Windows BLE/native runtime remains distinct. | `.github/workflows/ci-full.yml` `interfaces-build-windows`: `cargo check -p reticulumd --all-targets`; Windows BLE tests are a separate CI job. | Windows BLE CI passed on PR #627 head; this does not show every daemon interface running on Windows. Architecture is not pinned. | #606 contract; #614 |
| Android | Code contains Android-targeted RNode/BLE paths and HIL defines an Android profile, but hosted checks do not establish general Android daemon support. | `cargo xtask hil run --level nightly --profile android --output target/hil/runs/android-<run-id>` on the configured physical runner; preserve target/build/run artifact. | NOT RUN / excluded; no general Android support claim made. | #614 implementation; #616 operational evidence |
| CPU architecture | The inspected hosted workflows do not declare a finite architecture set: runner labels are OS-level and build-matrix does not set `--target`. | To claim architectures, pin target triples in CI and run target-specific compile plus required runtime evidence. | NOT DECLARED / NOT VERIFIED; no guessed x86_64/aarch64 rows. | #606 contract; #614 |

## Physical-interface verification rows

These finite rows come from the physical HIL profile matrices in
`.github/workflows/hil-nightly.yml` and `hil-release.yml`. They define what a
future operational report must identify and observe. Every row is **NOT RUN /
excluded** from this goal by the explicit scope decision in `GOAL.md`, which
records the Codex task provenance and tracks this axis under #616. A CI build,
virtual HIL case, fake device, loopback test, or source inspection cannot
change these statuses. No physical result is claimed, and #616 is not
complete.

For every future row, evidence must state exact candidate SHA, HIL run URL and
artifact, host OS and architecture, device/carrier identity and firmware where
applicable, peer identity/version, observed bidirectional protocol result,
and cleanup/recovery result. The repository procedure is
`cargo xtask hil run --level nightly --profile <profile> --output
target/hil/runs/<profile>-<run-id>` on the assigned physical runner. Until an
authorized run exists, this command is a procedure, not evidence.

| Finite HIL row(s) | Scope / observable claim | Verification command / evidence | Current evidence status | Owner |
| --- | --- | --- | --- | --- |
| `heltec`, `tbeam`, `esp32` | Named embedded/radio HIL profiles; verify configured device/carrier exchange and recovery as defined by each profile. | Per-profile command above; Linux physical HIL workflow; retain raw artifact. | NOT RUN / excluded (#616); no device result claimed. | #616 physical; #614 software gap |
| `vr-n76`, `ble`, `spp`, `windows` | BLE / serial-port physical profiles on configured lab runners; verify real device selection and exchange, beyond backend tests. | Per-profile command above; Linux/Windows self-hosted HIL matrices. | NOT RUN / excluded (#616); Windows BLE CI is backend/software evidence only. | #616; #614 |
| `kiss`, `ax25`, `serial` | Physical serial and KISS/AX.25 carrier paths; verify framing, exchange, and reconnect on the named lab setup. | Per-profile command above; Linux physical HIL matrix. | NOT RUN / excluded (#616). | #616; #614 |
| `rnode-prepared`, `rnode-multi` | Physical RNode / multi-radio configuration and packet exchange for prepared hardware profiles. | Per-profile command above; Linux physical HIL matrix and exact profile artifact. | NOT RUN / excluded (#616). | #616; #614 |
| `tcp` | Lab TCP profile against its configured real peer/network; record both endpoints and exchange. | Per-profile command above; Linux physical HIL matrix. | NOT RUN / excluded (#616); loopback/software TCP is not this row. | #616; #614 |
| `auto-interface` | Native multicast/link-local discovery and exchange on assigned physical LAN interfaces. | Per-profile command above; Linux physical HIL matrix; record actual interfaces/network. | NOT RUN / excluded (#616); loopback lifecycle tests do not satisfy it. | #616; #614 |
| `i2p`, `i2p-pair` | I2P service and paired-node behavior as configured by the HIL profiles. | Per-profile command above; Linux physical HIL matrix. | NOT RUN / excluded (#616); local/fake SAM tests are not this row. | #616; #614 |
| `weave` | Weave carrier/profile operation against the configured lab device/network. | Per-profile command above; Linux physical HIL matrix. | NOT RUN / excluded (#616); source and fake-transport tests do not satisfy it. | #616; #614 |
| `reticulum-interface-matrix` | HIL aggregate interface-family exercise on the assigned physical lab. | Per-profile command above; preserve per-case results and hardware mapping. | NOT RUN / excluded (#616); aggregate name does not substitute for row-level results. | #616; #614 |
| `android` | Android physical runtime profile for configured device and radio/BLE path. | `cargo xtask hil run --level nightly --profile android --output target/hil/runs/android-<run-id>`; retain device/build/run evidence. | NOT RUN / excluded (#616); no Android hardware result claimed. | #616; #614 |

Scope provenance is `GOAL.md`, row
`reticulum-605-616-operational-platform-evidence`, decision
`exclude-from-software-goal`, approval reference
`codex-task:01a0bf74-9050-70e1-b8bf-0e528184c9ad`. These rows remain visible,
not applicable to software-goal completion, and hardware-unverified; they are
not passed acceptance rows.

## External reference-client rows

The client set is finite for the repository's configured independent-peer
gate, not an assertion that every public Reticulum client is supported. The
Python target is the behavioral comparator; `rns-rs` and Reticulum-Go are the
two independent implementation identities in the pinned peer manifest.
Hosted software evidence remains distinct from direct physical or external-
network validation.

| Client row | Scope / observable claim | Verification command / evidence | Current evidence status | Owner |
| --- | --- | --- | --- | --- |
| Reticulum-Python 1.5.4-dev (`99de23c040d507e3fefca19e87b182302902725d`) | Frozen forward target for differential behavior; each test/evidence row must name behavior and client role. | Use the exact test command recorded by the owning child evidence and retain transcript/reference SHA; there is no `independent_interop.py --peer python-reference` option. | Target pinned and source-inspected; focused target-specific tests exist in child evidence but do not constitute a complete client matrix. Direct physical/external-network run: NOT RUN / excluded (#616). | #606 contract; behavior owner #609–#614 |
| Reticulum-Python 1.5.2 baseline (`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`) | Existing non-physical HIL Python reference peer for the configured interop suite; this is the active baseline, not the 1.5.4-dev target. | `cargo xtask hil run --level pr --profile python-reference --output target/hil/runs/python-reference-<run-id>`; profile is declared in `tests/hil/lab.toml`. | PR-level HIL passed on PR #627 head; it does not validate physical/client-network operation or substitute for the forward-target differential tests. | #606 contract; behavior owner #609–#614 |
| rns-rs `rns-net-v0.7.0`, `6c6d79b83516feff271d15c97d39dd1de7798afe` | Independent peer for capabilities declared in `independent-implementations.toml`; only emitted per-capability evidence counts. | `python3 tools/scripts/independent_interop.py --peer rns-rs --level pr --output target/independent-rns-rs --keep`; hosted workflow `.github/workflows/independent-interop.yml`. | PR-level independent evidence passed on PR #627 head; hosted software interop only, not physical/external-network certification. | #606 contract; applicable behavior child issue |
| Reticulum-Go `v1.0.1`, `48f15178f6fbc34aeb69ad428679db9deddae7f4` | Independent peer for capabilities declared in `independent-implementations.toml`; configured in nightly/release tiers, not the PR peer set. | `python3 tools/scripts/independent_interop.py --peer reticulum-go --level nightly --output target/independent-reticulum-go --keep`; hosted workflow `.github/workflows/independent-interop.yml`. | Peer identity/capabilities pinned; no current PR-level result claimed. Direct physical/external-network run: NOT RUN / excluded (#616). | #606 contract; applicable behavior child issue |

Any wider external-client acceptance—another implementation/version,
public-network operation, or a client attached through physical carrier
hardware—requires a separate #616 scope decision and is **NOT RUN / excluded**
here. Do not treat independent-peer CI, Python differential tests, or
successful compilation as evidence for that operational row.

## Existing evidence and traceability

- Hosted checks on PR #627 head `9832268f9d42811027cb75dc68b1f3e4fbfd395b`
  passed for quality, Linux build-matrix (stable), unit tests, architecture
  checks, contracts, independent `rns-rs` evidence, and PR-level HIL. These
  checks do not imply physical lab testing; PR-level HIL is not a physical
  carrier run.
- `606-behavioral-contract.md` remains the behavioral contract evidence;
  behavioral observations and target-delta review remain open.
- `614-native-interfaces.md` records bounded native-interface software tests
  and remaining live, cross-platform, and hardware gaps.
- `615-release-acceptance.md` and `GOAL.md` preserve software/operational
  separation and #616 scope provenance.

This matrix makes the verification inventory finite; it does not check the
remaining GitHub acceptance box, close #606/#616, establish full behavioral
parity, or promote an operational row to success.
