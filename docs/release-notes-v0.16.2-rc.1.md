# LXMF-rs v0.16.2-rc.1

This maintenance candidate follows v0.16.1. It aligns the daemon, SDK and
project crates at 0.16.2-rc.1 and requires Rust 1.89 for source builds.

## Changes

- Retire negotiated SDK sessions after five idle minutes while protecting active
  requests and preserving durable identities, custody, checkpoints and operations.
  Aggregate current and expired session counts are available in daemon diagnostics.
- Upgrade bundled SQLite and rustix past the addressed security advisories.
- Correct scoped AutoInterface discovery binding and local link-request MTU
  behavior; carry per-packet RNode signal measurements without cross-packet reuse.
- Reduce propagation retention/purge query work and checkpoint allocations, and
  finish persistence before interface retirement during shutdown.
- Preserve explicit loopback address families and cover delivery proofs for
  restored service identities.

## Upgrade and qualification

Stop writers and take consistent backups of configuration, identities, databases
and consumer state before upgrading. Restore a compatible backup for rollback;
do not open migrated databases with older binaries. Session reclamation removes
only ephemeral state. An expired durable client must renegotiate and restore its
identity through the existing SDK recovery path.

Release package checksums, source provenance and install/upgrade/restart/delivery
results must be verified before publication. Existing local session-expiry and
RCH custody checks do not qualify production, physical radios or long soaks.
This is a prerelease and does not promote the stable/latest release channel.

The GitHub prerelease includes binary archives, platform packages, and crate
SBOMs. This promotion does not publish an OCI image or crates.io versions.
SHA-256 checksums accompany the packages. This candidate path does not produce a
cosign checksum bundle, and Windows and macOS signing depends on available
signing credentials.
