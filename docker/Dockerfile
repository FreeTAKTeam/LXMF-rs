# syntax=docker/dockerfile:1
# LXMF-rs runtime image (distroless, fully static musl binaries).
#
# The release workflow (.github/workflows/release.yml) stages the prebuilt
# static binaries into docker/context/<TARGETARCH>/ before building, so no
# compilation happens inside the container build:
#
#   docker/context/amd64/   <- linux-x86_64 musl build
#   docker/context/arm64/   <- linux-aarch64 musl build
FROM gcr.io/distroless/static-debian12:nonroot

ARG TARGETARCH
COPY docker/context/${TARGETARCH}/ /usr/local/bin/
COPY --chown=65532:65532 docker/data/.gitkeep /data/.gitkeep

USER nonroot
VOLUME ["/data"]
WORKDIR /data

# Default to the daemon; every shipped tool is on PATH for `docker run --entrypoint`.
ENTRYPOINT ["/usr/local/bin/reticulumd"]
