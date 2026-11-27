# Fabric v0.17.0

Fabric is the new name for the project formerly known as LXMF-rs. The GitHub
repository (`FreeTAKTeam/LXMF-rs`), crate names, binaries (`reticulumd`,
`lxmf-cli`, `rnx`, `rnsd` and the other `rns-tools` commands) and the
`ghcr.io/freetakteam/lxmf-rs` container image are unchanged. Release archive
and package file names now use the `fabric_` prefix, for example
`fabric_0.17.0_linux-x86_64.tar.gz`; the Debian/RPM package name stays
`lxmf-rs` so existing installs upgrade in place.

This full release follows v0.16.1 and the v0.16.2 prereleases. The daemon, SDK
and public package manifests share version `0.17.0`, including the
separately published `reticulum-rs-zeromq` transport.

## Changes since v0.16.2-rc.2-linux

- Fabric project identity: purpose and boundaries, README and docs, logo and
  wordmark assets (#765).
- New `rpp-core` crate: RPP 0.3 RC1 engine and RNS Resource contract (#766,
  #767, #769), bounded decoder allocation, explicit coverage of every RC1
  negative vector (#770), and `ShareNode` for one-file publish, Offer and
  verified fetch over RNS with `prepare_file`, `allow` and
  `package_destination`.
- New `rdo-core` crate: RDO owner binding v1 with a Python/Rust network gate
  (#771); resolving an object from its already-received announce (#772).
- ZeroMQ transport: a fair-queue stream that wakes itself while pending now
  yields instead of livelocking the cooperative scheduler (#773).

Changes in v0.16.2-rc.1 and v0.16.2-rc.2-linux (MeshChat history import,
announce-retention performance, `rnsh` teardown and others) are also part of
this release; see their release notes.

## Distribution

GitHub release assets cover Linux x86_64, AArch64 and ARMv7 (static musl
archives plus deb/rpm), Windows (ZIP and MSI), macOS universal, CycloneDX
SBOMs, cosign-signed `SHA256SUMS.txt` and build provenance. Public library
crates are published to crates.io at `0.17.0` by the release workflow; the new
`rpp-core` and `rdo-core` crates are not yet part of the crates.io publish
plan.

## Upgrade and qualification

Stop writers and back up configuration, identities, databases and consumer
state before upgrading. Do not open migrated databases with older binaries.
Parity rows are unchanged by this release. Physical radios, long soaks and
production memory stability are not qualified by it.
