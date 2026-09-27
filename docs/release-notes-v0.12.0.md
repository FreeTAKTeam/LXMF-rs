# LXMF-rs v0.12.0

v0.12.0 updates the tested RNS 1.5.2-compatible system with the feature and
behavior changes identified in the pinned Reticulum 1.5.4-dev revision
`99de23c040d507e3fefca19e87b182302902725d`. The canonical 1.5.2
reference pin and existing public wire formats remain unchanged. This is a
scoped update, not a claim of full forward Reticulum parity.

## Highlights

- HDLC framing, RNode BLE lifecycle and paired-device selection, and `rngit`
  media, page, permission, work-item, and link-cleanup behavior have focused
  regressions against the pinned Python feature delta.
- The transport library now exposes request-envelope and identify-proof
  builders, stored RNode radio configuration, improved Link close reasons,
  AutoInterface peer-data reachability, and RNode-over-TCP disconnect handling.
- LXMF wire messages expose ID, stamp, and ticket members; daemon stamp and
  ticket helpers moved into the library. The daemon and utilities also gained
  focused fixes for opportunistic delivery, propagation stamp cost, `rnsh`
  command lifetime, and `rngit` permission/work persistence.
- IFAC carrier and Resource interoperability checks were extended, including
  authenticated UDP daemon traffic and transfer-recovery regressions.

## Compatibility and remaining work

All 17 public workspace crates advance together to `0.12.0`, including their
workspace-local dependency requirements. No Reticulum packet or LXMF wire
format change is intended. Consumers should rebuild and run their integration
tests against the new library versions; the public API additions and behavioral
fixes above do not imply compatibility with every Python edge case.

The active release baseline remains pinned to Reticulum 1.5.2 at
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`. The 1.5.4-dev feature delta,
its evidence, and its broader follow-ups are described in the
[delta ledger](status/rns-1.5.4-delta.md). Issues #608–#614 remain separately
tracked parity follow-ups. Physical, client, platform, and public-network
coverage remains separately tracked under #616 and is not certified by this
release. See the [current roadmap](status/current-roadmap.md) for the latest
qualified status.

## Release verification

The release is cut from the reviewed `v0.12.0` commit only after local release
checks and exact-head CI pass. The published GitHub artifacts, checksums,
provenance, container image, and matching crates.io versions must be checked
against the immutable tag. Passing software checks does not substitute for
the outstanding physical and operational matrix.
