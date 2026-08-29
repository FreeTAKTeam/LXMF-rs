# LXMF-rs Release Strategy

This document describes the release pipeline implemented in
`.github/workflows/release.yml`. The older `release-bundles.yml` workflow is
retained as a manual-only legacy workflow and does not trigger on release tags.

## What every release ships

| Deliverable | Artifacts | Produced by |
|---|---|---|
| Linux static binaries (musl) | `lxmf-rs_<ver>_linux-x86_64.tar.gz`, `…_linux-aarch64.tar.gz`, `…_linux-armv7.tar.gz` | `build` job |
| Raspberry Pi | aarch64 (Pi 4/5, 64-bit OS) and armv7/armhf (32-bit OS) static builds above | `build` job |
| Debian / RPM packages | `lxmf-rs_<ver>_{amd64,arm64,armhf}.{deb,rpm}` (nfpm) | `package-linux` job |
| Windows | `lxmf-rs_<ver>_windows-x86_64.zip` (signed exes) + `lxmf-rs_<ver>_x64.msi` (signed MSI) | `package-windows` job |
| macOS universal | `lxmf-rs_<ver>_macos-universal.tar.gz` (lipo universal2, optional codesign; notarization is not wired) | `macos-universal` job |
| OCI container | `ghcr.io/freetakteam/lxmf-rs:<ver>` (and `:latest` for stable releases only), multi-arch (amd64 + arm64) | `container` job |
| SBOM | CycloneDX per app crate (`sbom-reticulumd.cdx.json`, `sbom-lxmf-cli.cdx.json`, `sbom-rns-tools.cdx.json`) + `sbom-container.cdx.json` | `sbom` and `container` jobs |
| Checksums | `SHA256SUMS.txt` + `SHA256SUMS.txt.cosign.bundle` (keyless sigstore signature) | `release` job |
| Provenance | GitHub build-provenance attestations for every release file and for the container image | `release` and `container` jobs |
| Homebrew | Stable-release formula update pushed to `FreeTAKTeam/homebrew-tap` | `homebrew` job |

All Linux targets are built against musl with `+crt-static`, so the shipped
binaries are fully static (SQLite is bundled via rusqlite's `bundled` feature,
and BLE/dbus support is behind opt-in cargo features, so nothing links against
glibc or system libraries). The same static binaries back the `.deb`/`.rpm`
packages and the distroless container image, so every channel ships bit-for-bit
identical tools.

## Pipeline overview

```
prepare ─► build (matrix: linux-musl x3, windows, macos x2)
    │         ├─► macos-universal (lipo, optional codesign)
    │         ├─► package-linux (nfpm → .deb/.rpm x3 arches)
    │         ├─► package-windows (sign → ZIP + WiX MSI → sign)
    │         ├─► sbom (cargo-sbom per app crate)
    │         └─► container (buildx amd64+arm64 → ghcr, cosign sign, attest, syft SBOM)
    └─► release (SHA256SUMS → cosign sign-blob → attest-build-provenance → gh release)
              └─► homebrew (render formula with sha256 → push to homebrew-tap)
```

## Cutting a release

1. Bump `VERSION` and the crate versions (`cargo xtask` release helpers already
   exist for crates.io publishing).
2. For this release, tag the reviewed commit: `git tag -a v0.10.1 -m "LXMF-rs v0.10.1" && git push origin v0.10.1`.
3. The workflow runs end to end. A manual dry run is available via
   **Actions → Release → Run workflow** (set `publish: false` to build and
   smoke-test everything without publishing).
4. Promote only the same immutable commit to the stable `v0.10.1` release after
   the release evidence ledger recommends publication.

Pre-release tags containing `-rc`, `-alpha`, `-beta`, or `preview` are
published as GitHub pre-releases automatically.

## Secrets to configure

| Secret | Purpose | Required? |
|---|---|---|
| `GITHUB_TOKEN` | Release upload, ghcr push, attestations | Automatic |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TS_ENDPOINT`, `AZURE_TS_ACCOUNT_NAME`, `AZURE_TS_CERT_PROFILE` | Windows/MSI signing via Azure Trusted Signing | Optional — builds are unsigned if absent |
| `APPLE_CERTIFICATE_BASE64` (p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID` | macOS codesign (notarization is not currently wired) | Optional — builds are unsigned if absent |
| `HOMEBREW_TAP_TOKEN` | PAT with write access to `FreeTAKTeam/homebrew-tap` | Optional — tap update skipped if absent |

Every optional integration degrades gracefully: the workflow notices in the log
and continues, so a fork or a fresh setup still produces a complete unsigned
release.

## Verifying a release

```sh
# 1. Download the artifact set plus checksums and signature bundle, then:
sha256sum -c SHA256SUMS.txt

# 2. Verify the checksum signature (keyless, against the CI identity):
cosign verify-blob \
  --bundle SHA256SUMS.txt.cosign.bundle \
  --certificate-identity-regexp "https://github.com/FreeTAKTeam/LXMF-rs/.github/workflows/release.yml.*" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS.txt

# 3. Verify build provenance of any file:
  gh attestation verify lxmf-rs_0.10.1_linux-x86_64.tar.gz --owner FreeTAKTeam

# 4. Verify the container image:
  cosign verify ghcr.io/freetakteam/lxmf-rs:0.10.1 \
  --certificate-identity-regexp "https://github.com/FreeTAKTeam/LXMF-rs/.*" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
  gh attestation verify oci://ghcr.io/freetakteam/lxmf-rs:0.10.1 --owner FreeTAKTeam
```

## Consumer usage

```sh
# Container (works on Raspberry Pi OS 64-bit too):
# The image's anonymous volume is writable by UID 65532. For a bind mount,
# prepare the host directory for the image's nonroot user first:
mkdir -p data && sudo chown 65532:65532 data
docker run --rm -v $PWD/data:/data ghcr.io/freetakteam/lxmf-rs:latest

# Homebrew:
brew tap freetakteam/tap && brew install lxmf-rs

# Debian/Ubuntu/Raspberry Pi OS:
sudo dpkg -i lxmf-rs_0.10.1_arm64.deb     # or amd64 / armhf

# Fedora/RHEL/openSUSE:
sudo rpm -i lxmf-rs-0.10.1-1.aarch64.rpm  # or x86_64
```

## Design notes

- **Why musl static builds?** One binary per architecture runs on every Linux
  distro regardless of glibc version, and the same binaries can go straight
  into a `distroless/static` image with no libc at all.
- **Why nfpm instead of cargo-deb/cargo-generate-rpm?** nfpm builds both .deb
  and .rpm from a single config and does not require editing crate manifests.
- **Why Azure Trusted Signing?** It is the current Microsoft-recommended path
  for OSS code signing and needs no hardware token or certificate files in CI.
  A classic PFX + `signtool` flow can replace that step if the project prefers.
- **Why GitHub artifact attestations?** `actions/attest-build-provenance`
  provides SLSA-style provenance backed by sigstore with no key management;
  `gh attestation verify` is the consumer-side check. The cosign-signed
  checksums cover consumers outside the GitHub ecosystem.
- **Raspberry Pi coverage** is the stock aarch64 and armv7 musl targets; they
  run on Pi 3/4/5 and Zero 2 W under both 64-bit and 32-bit Raspberry Pi OS.
