# LXMF-rs v0.13.0 (draft)

v0.13.0 updates the tested RNS 1.5.2-compatible system with selected changed
behavior from the exact Python Reticulum 1.5.5 tag
`7f2b3b9b524c9386316379af1313b43a5e4f7a5d`. The active 1.5.2 reference,
historical 1.5.4-dev #605 feature pin, and existing public wire formats are
unchanged. This is a scoped feature update, not full Reticulum parity.

## Changes

- Discovery announcements retain implementation/version metadata. The library
  auto-connect planner defaults to verified RNS 1.5.2-or-newer Backbone peers,
  with an explicit unverified override. Empty or literal `None` IFAC values
  are discarded; `rnstatus-rs --discovered` hides stale and unknown rows unless
  requested, while JSON retains the complete discovery set.
- `rngit` can serve Markdown downloads converted to `.mu` over an active Link
  and shows permission-filtered Active, Completed, Proposed, and All workdoc
  counts. A Python Reticulum 1.5.5 Link received one converted download and
  matching filename/bytes for the tested fixture.
- Workspace package and path-dependency versions advance together to 0.13.0.

## Compatibility and remaining work

Rust consumers that construct `DiscoveredInterface` or
`AutoconnectInterfacePolicy` with struct literals must supply the new optional
implementation/version fields or the explicit `allow_unverified` policy field.
Persisted discovery rows without those metadata fields still deserialize.

The discovery planner is not yet a production daemon auto-connect worker.
The built-in Markdown converter covers common syntax, not all Python tables or
syntax highlighting. Named `rnstatus` attach/detach/reload and its management
policy are not yet implemented; the existing `set_interfaces` RPC only queues
changes and must not be treated as equivalent. These omissions must be
reconciled with the release scope before publication. See the
[1.5.5 delta ledger](status/rns-1.5.5-delta.md) for affected-path evidence and
the [roadmap](status/current-roadmap.md) for remaining parity work.

#616 physical, client, and public-network acceptance remains separate from this
software feature update.

## Release verification

Publish only after the final candidate passes local release checks and normal
exact-head CI. Verify the immutable GitHub tag, assets, checksums, provenance,
container image, and matching crates.io versions independently after release.
