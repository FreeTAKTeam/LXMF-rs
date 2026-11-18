# LXMF-rs v0.16.2-rc.2-linux

This Linux-only prerelease follows v0.16.2-rc.1. The daemon, SDK and public
package manifests share version `0.16.2-rc.2-linux`. It ships Linux x86_64,
AArch64 and ARMv7 archives and packages only; it does not publish crates.io
versions or promote the stable/latest release channel.

## Changes since v0.16.2-rc.1

- MeshChat: import legacy MeshChat history (#760) and save
  `BackboneClientInterface` through the interface API (#759).
- Announce retention visits only eligible history rows and keeps a running
  payload byte total, cutting retention work on large stores (#762, #717, #663).
- `rnsh` flushes the initiator's Link teardown before exiting.
- Daemon memory bench and test/lint fixes for cargo and clippy 1.100.
- Docs: R3S/1 sensor telemetry and linked SPI extension profile (#761).

## Upgrade and qualification

Stop writers and back up configuration, identities, databases and consumer
state before upgrading. Do not open migrated databases with older binaries.
This is a prerelease for real-system testing; physical radios, long soaks and
production memory stability are not qualified by it.
