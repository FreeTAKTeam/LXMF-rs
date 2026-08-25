# LXMF-rs v0.10.0

v0.10.0 is the stable release for the Reticulum 1.5.0 alignment. It promotes
the reviewed implementation merged on `main` and keeps hardware, public-network,
and third-party-client validation as separate evidence tracks. Stable artifacts
and package publication are tied to the immutable `v0.10.0` tag.

## Reticulum 1.5.0 alignment

The release is compared with these exact Python references:

- Reticulum `1.5.0` at
  `e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`;
- LXMF at `727830cefda83d9c6e3982b48675425f3f988f9c`.

The strict generated inventory contains 1,839 entries: 1,838 applicable and
complete, zero partial, zero unmapped, and one provenance-backed
not-applicable `CRNS` package. The manual 41-row release audit is recorded in
[`docs/status/rns-1.5-delta.md`](status/rns-1.5-delta.md).

Highlights include:

- bounded four-class priority ingress with early filtering, violation
  accounting, queue pressure, drop, and flow telemetry;
- destination-scoped path-request batching, adaptive expiry, blackhole-aware
  announce admission, and Backbone child-limiter accounting;
- negotiated full-link Channel and Buffer MDU use, including pinned-Python
  transfer validation above the legacy packet MDU;
- medium-bitrate timeout accessors and adaptive `rnpath`/`rngit` policy;
- operator-address discovery announcements, TCP-client discovery publication,
  and encryption bound to an explicitly configured shared network identity; and
- expanded typed RPC, JSON, SDK/ZeroMQ, and human `rnstatus` visibility.

## Compatibility boundaries

The pre-1.0 minor release advances the workspace and publishable crates to
`0.10.0`. SDK/RPC consumers should tolerate the additive telemetry fields and
regenerate strongly typed clients from the release schemas when applicable.
Source-level changes and configuration cutover steps are listed in the
[`v0.10.0 migration guide`](migrations/v0.10.0-rns-1.5.md).

Reticulum IFAC authentication is not implemented; IFAC configuration fails
closed at daemon startup instead of creating an unauthenticated carrier.
Physical RNode/RNodeMulti, BLE, Weave, VR-N76, serial/radio operation, public
networks, and third-party clients remain separate evidence axes. Their absence
does not imply that those environments were tested by this release.

## Validation and publication

The reviewed RNS 1.5.0 implementation passed the complete local release gate,
the pinned-Python interoperability and HIL matrix, strict parity/inventory
checks, independent review, and the hosted pull-request workflows. The
tag-triggered Release, crates.io, performance, signing, provenance, and OCI
workflows published evidence for the immutable `v0.10.0` tag. Homebrew was
skipped because the tap repository/token is not configured; this does not affect
the archives, native packages, crates, or OCI publication.

The performance comparison used five interleaved comparison runs, two isolated
resource runs, and 1,000 resource iterations on the same runner. Its release
budget passed with geomean throughput `1.008x`, CPU `1.005x`, and peak RSS
`1.086x` versus the v0.9.1 baseline, with no warnings or failures. The release
attaches checksummed JSON, HTML, and raw performance evidence alongside the
standalone independent-interoperability reports.
