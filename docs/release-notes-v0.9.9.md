# LXMF-rs v0.9.9

v0.9.9 promotes the RNS 1.4.2 software-parity release candidate to the stable
release channel. It includes the reviewed work merged after v0.9.8 and keeps
hardware, public-network, and third-party-client validation as separate
evidence tracks rather than folding them into the software-parity result.

## Highlights

- Completes the generated Python Reticulum 1.4.2 software inventory: 1,810
  applicable entries are complete, zero are partial or unmapped, and the absent
  `CRNS` package is the single provenance-backed not-applicable entry.
- Adds the repository-native HIL controller and pinned Python Reticulum/LXMF
  verification for virtual transport, channels, paper messages, compatibility
  matrices, and LXMD relay scenarios.
- Expands routing, resource, lifecycle, interface-policy, blocked-IP reporting,
  and `rngit` repository-service compatibility, including document ACL and
  work-item behavior.
- Publishes independent rns-rs and Reticulum-Go interoperability evidence as a
  separate release axis, including multi-hop, restart, large-resource, routing,
  and deterministic-chaos scenarios.
- Adds a five-sample release performance gate and standalone JSON, HTML, and raw
  evidence artifacts with explicit warning and hard-regression budgets.
- Extends the release pipeline with static multi-platform bundles, Debian/RPM
  packages, Windows MSI output, SBOMs, checksums, provenance attestations, OCI
  images, and stable-channel publication hooks.

## Compatibility boundary

The release targets Python Reticulum 1.4.2 at
`b48b96e61676504e0a4e527b33b9a0b4495c6872` and Python LXMF at
`727830cefda83d9c6e3982b48675425f3f988f9c`. The seven tracked LXMF software
rows and the generated RNS callable inventory are complete for their named
software scenarios.

Physical RNode/RNodeMulti, Weave, VR-N76, BLE, and serial-radio validation;
public I2P and public Reticulum soak; and Sideband, MeshChatX, Columba, and
other third-party-client validation remain explicitly separate. Their absence
does not imply evidence that those environments were tested by this release.

## Validation and publication

Stable publication is performed from the immutable `v0.9.9` tag. The
tag-triggered workflows build and attest release bundles, run the exact-tag
independent interoperability and performance gates, publish the GitHub release
and OCI image, and publish the public Rust crates in dependency order. Public
assets include checksums and standalone interoperability and performance
evidence so each claim can be verified against the release commit.
