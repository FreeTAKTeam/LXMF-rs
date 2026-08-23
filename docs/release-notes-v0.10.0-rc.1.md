# LXMF-rs v0.10.0-rc.1

This is the RNS 1.5.0 alignment candidate for the Rust Reticulum and LXMF
implementations. The workspace and publishable crate version is `0.10.0`; the
`-rc.1` suffix is carried by the eventual GitHub tag and prerelease. No tag or
artifact publication is implied by this preparation PR.

## RNS 1.5.0 alignment

The candidate is compared with these exact Python references:

- Reticulum `1.5.0` at
  `e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`;
- LXMF at `727830cefda83d9c6e3982b48675425f3f988f9c`.

The strict generated inventory contains 1,839 entries: 1,838 applicable and
complete, zero partial, zero unmapped, and one provenance-backed
not-applicable `CRNS` package. The manual 41-row release-note audit is recorded
in [`docs/status/rns-1.5-delta.md`](status/rns-1.5-delta.md).

Notable changes include:

- bounded four-class priority ingress with early filtering, violation
  accounting, queue pressure, drop, and flow telemetry;
- destination-scoped path-request batching, adaptive expiry, blackhole-aware
  announce admission, and active Backbone child limiter counts;
- negotiated full-link Channel and Buffer MDU use, validated with a real
  pinned-Python Backbone transfer above the legacy packet MDU;
- medium-bitrate timeout accessors and adaptive `rnpath`/`rngit` policy;
- operator-address discovery announcements, TCP-client discovery publication,
  and encryption bound to an explicitly configured shared network identity; and
- expanded typed RPC, JSON, SDK/ZeroMQ, and human `rnstatus` visibility.

## Compatibility and evidence boundaries

The release adds public status/configuration fields and therefore advances the
pre-1.0 minor version to `0.10.0`. SDK/RPC consumers should tolerate the new
optional telemetry fields and regenerate strongly typed clients from the
candidate schema when applicable. Source-level changes and configuration
cutover steps are listed in the
[`v0.10.0 migration guide`](migrations/v0.10.0-rns-1.5.md).

Physical RNode/RNodeMulti, BLE, Weave, VR-N76, serial/radio operation, public
networks, and third-party clients remain separate evidence axes. Reticulum
IFAC authentication is not implemented; IFAC configuration now fails closed at
daemon startup instead of creating an unauthenticated carrier. RNS 1.5
flag-policy violations are still accounted for in the live transport prequeue path. The shipped
`rngit` CLI remains a local Git workflow, while its transport-neutral client
applies a caller-injected medium timeout before remote operations.

## Candidate status

The candidate becomes ready for tagging only when the exact PR head passes the
repository's complete local release gate, pinned-Python interoperability and
HIL cases, independent review, and required hosted pull-request checks. Tag
publication, release assets, attestations, signing state, OCI images, and the
versioned performance dataset are subsequent tag-triggered evidence.
