# LXMF-rs v0.16.1-rc.1-linux

This Linux-only prerelease packages the changes since v0.16.0 for real-system
testing. The daemon, SDK and all 21 public package manifests share version
`0.16.1-rc.1-linux`. This is a GitHub binary release; it does not publish those
packages to crates.io or promote a stable release. The Linux OCI image uses the
versioned tag without updating `latest`.

## Changes since v0.16.0

- Daemon readiness can notify systemd after configured RPC and ZeroMQ listeners
  bind. Graceful SIGTERM shutdown participates in the same service lifecycle.
- Expired remote destination/peer identity caches are pruned while preserving
  live links, local destinations and pending proof contexts. Random-blob
  capacity and duplicate-hashlist persistence avoid avoidable retained copies.
- Propagation maintenance exposes separate phase costs. Indexed peer-entry
  expiry/quota and handled-ID queries reduce scans and temporary sorts; payload
  pruning releases the writer between bounded candidate windows. Refill respects
  the global peer-entry budget instead of recreating entries removed by quota.
- Announce history is retained in bounded batches while preserving each peer's
  latest event and stamp-cost information. Durable consumers can reconcile
  bounded latest-per-peer pages so busy peers do not hide quiet ones.
- The daemon uses mimalloc with a requested-Rust-allocation gauge. Periodic
  propagation maintenance also includes the experimental, guarded SQLite
  pager-cache release from #711. Its sustained RSS/performance benefit remains
  unverified; merging it did not satisfy the memory acceptance gates.
- MeshChat API discovery includes configured interfaces and live display-name
  updates. `lxmd` inbound hooks follow the daemon event stream. Performance
  reports now distinguish matched work and withhold unsubstantiated end-to-end
  ratios.

The integrated high-priority fixes additionally include:

| PR | Result |
| --- | --- |
| [#722](https://github.com/FreeTAKTeam/LXMF-rs/pull/722) | Propagation `/get` and control responses use the negotiated Link MDU, with Resource transfer for larger responses. Control/resource workers continue after broadcast lag. |
| [#723](https://github.com/FreeTAKTeam/LXMF-rs/pull/723) | Resident-evidence replay rejects invalid timing, overlapping captures and reset maintenance counters. This corrects validation without creating new memory-stability evidence. |
| [#724](https://github.com/FreeTAKTeam/LXMF-rs/pull/724) | An indexed retry deadline excludes future-deferred operations from empty durable-dispatch polls while preserving due-message timestamp/id ordering. |
| [#725](https://github.com/FreeTAKTeam/LXMF-rs/pull/725) | Designated protocol cores build under isolated bare-metal feature contracts. Dependency/source checks and seeded violations enforce the boundary and catch accidental `std` feature unification. |
| [#726](https://github.com/FreeTAKTeam/LXMF-rs/pull/726) | I2P startup preserves literal nonempty IFAC credential aliases, including whitespace, and falls back when canonical values are empty. Pinned TCP lifecycle and KISS PTY checks run in Verify. |
| [#727](https://github.com/FreeTAKTeam/LXMF-rs/pull/727) | Outbound hashes enter duplicate membership only after carrier acceptance, preserving ingress eligibility after rejection. Pinned duplicate/cache-generation checks and isolated Data/LinkRequest restart fixtures extend coverage. |

## Linux artifacts and source verification

Static-musl archives and Linux packages are built for these targets:

| Architecture | Rust target | Archive suffix | Package filename architecture |
| --- | --- | --- | --- |
| x86-64 | `x86_64-unknown-linux-musl` | `linux-x86_64.tar.gz` | `amd64` |
| AArch64 | `aarch64-unknown-linux-musl` | `linux-aarch64.tar.gz` | `arm64` |
| ARMv7 hard-float | `armv7-unknown-linux-musleabihf` | `linux-armv7.tar.gz` | `armhf` |

Archives use the prefix `lxmf-rs_0.16.1-rc.1-linux_`; `.deb` and `.rpm` assets
use that prefix followed by their package architecture. The versioned OCI image
is `ghcr.io/freetakteam/lxmf-rs:0.16.1-rc.1-linux`.
Package metadata normalizes the SemVer prerelease for distro ordering: Debian
uses `0.16.1~rc.1-linux`; RPM uses version `0.16.1~rc.1_linux`, release `1`.
Asset filenames retain the release version shown above.

The release record identifies the annotated tag `v0.16.1-rc.1-linux` and the
build workflow run. Attached `release-build-source.json` records `release_tag`,
`product_commit`, `workflow_commit`, `workflow_ref`, `workflow_run` and
`oci_image_digest`. Check the full product commit against the tag; a successful
earlier PR build is not a substitute for the tagged release build. Exact hashes
belong to the generated assets rather than this source document.

Download `SHA256SUMS.txt` alongside the selected assets and verify them before
installation:

```bash
sha256sum --check --ignore-missing SHA256SUMS.txt
```

The release also attaches `SHA256SUMS.txt.cosign.bundle`, the component SBOMs
`sbom-reticulumd.cdx.json`, `sbom-lxmf-cli.cdx.json` and `sbom-rns-tools.cdx.json`,
and `sbom-container.cdx.json`. Linux performance and independent-interoperability
evidence are recorded separately from the distributable binaries.

## Upgrade and operations

Rust 1.88 remains the workspace minimum for source builds; the Bluetooth leaf
retains its Rust 1.85 contract. Reference pins and the SDK contract retain their
existing compatibility boundaries.

Preserve existing identities, configuration, databases and paired consumer
state. Before replacing a running daemon, stop writers and take
SQLite-consistent backups. Follow the
[v0.14.0 migration guide](migrations/v0.14.0-zmq.md) and
[durable-broker operations](goals/zmq-rch-durable-broker/OPERATIONS.md) for
custody, checkpoints, restore and downgrade. A migrated store must not be opened
with an older binary as a rollback shortcut; use the documented consistent
backup procedure and account for already admitted work.

An existing durable-broker database receives the dispatch-index migration at
startup before traffic admission: `(state, next_attempt_at, message_id)`
replaces `(state, message_id)`. Building the replacement index takes one-time
startup work and disk/WAL space. Allow for that work when planning maintenance
and service startup; the migration is transactional and rolls back on failure.
It does not change retry deadlines, dispatch order or custody semantics.

## Evidence and remaining limits

Focused software regressions and the required CI, Verify and independent
interoperability workflows cover the integrated fixes. Release-build, package
and artifact verification results are recorded in the GitHub prerelease for the
exact tagged source. This source note does not assert that an uncompleted
workflow or an unbuilt artifact has passed.

The IFAC alias fix leaves the wider carrier-family lifecycle matrix open under
[#608](https://github.com/FreeTAKTeam/LXMF-rs/issues/608). Duplicate/cache and
restart regressions do not complete the broader routing/shared-instance and
repeated bidirectional recovery matrix under
[#609](https://github.com/FreeTAKTeam/LXMF-rs/issues/609). The `no_std` guarantee
applies to the designated cores and feature sets, not every library or daemon.
Physical hardware and public-network qualification remain separate.

[#663](https://github.com/FreeTAKTeam/LXMF-rs/issues/663) and
[#709](https://github.com/FreeTAKTeam/LXMF-rs/issues/709) retain their production
resource, sustained memory and delivery acceptance limits. Resident mapping
attribution passed on stored diagnostic evidence, but the tagged baseline
failed RSS-window stability. The SQLite cache-release candidate has no completed
stability qualification. No new synthetic one-hour soak, production deployment
or broad high-load acceptance is implied by this prerelease. See the
[resource investigation](performance/rch-resource-fixes-663.md),
[roadmap](status/current-roadmap.md) and maintained parity matrices for the
remaining work.

[Full changelog](https://github.com/FreeTAKTeam/LXMF-rs/compare/v0.16.0...v0.16.1-rc.1-linux).
