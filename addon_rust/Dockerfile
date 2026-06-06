# syntax=docker/dockerfile:1.6
#
# Runtime-only Dockerfile.
#
# Ships a pre-compiled musl binary (cross-compiled on the developer
# machine via scripts/build_addon_rust_binaries.sh) inside the addon
# tarball. HA's add-on builder runs THIS file on the Pi — and all it
# does is COPY a few files into the per-arch HA Alpine base image.
# That means an `update` on the Pi takes ~30 s (just the COPY +
# layer flatten) instead of ~15 min (cargo compile of ~150 crates).
#
# How the per-arch dispatch works:
#   * HA Supervisor passes `BUILD_ARCH` automatically when it builds
#     the addon for a given target — one of: aarch64, amd64, armv7,
#     armhf, i386. We use it to pick the right prebuilt binary.
#   * BUILD_FROM is the same per-arch base image variant (set in
#     build.json). It already contains s6-overlay + with-contenv
#     so run.sh works unchanged.
#
# To bump the embedded binary, run on a dev machine:
#   scripts/build_addon_rust_binaries.sh
#
# That re-runs the cross-compile and refreshes
# addon_rust/prebuilt/{aarch64,amd64}/epaper-dashboard-rust.

ARG BUILD_FROM=ghcr.io/home-assistant/amd64-base:3.20
ARG BUILD_ARCH=amd64

FROM ${BUILD_FROM}

# `BUILD_ARCH` is declared globally above (pre-FROM) so it propagates
# into stage scope under BuildKit. We need to re-declare it here as
# well so it's visible to the COPY below — BuildKit treats global
# ARGs as "available but not bound" inside stages.
ARG BUILD_ARCH

ENV LANG=C.UTF-8

WORKDIR /opt/app

# Pull the right prebuilt binary for this build's target arch.
# Path layout matches the output of build_addon_rust_binaries.sh.
COPY prebuilt/${BUILD_ARCH}/epaper-dashboard-rust /opt/app/epaper-dashboard-rust

# config.yaml is read at runtime by the binary's `Settings::load()`
# fallback path (when /data/options.json is absent — useful for
# local dev). config.yaml is also embedded into the binary at
# compile time, so this COPY is mostly belt-and-braces.
COPY config.yaml /opt/app/config.yaml
COPY --chmod=755 run.sh /run.sh

# Defensive chmod: the prebuilt binary is +x on disk but tar/COPY
# semantics across CI environments can occasionally drop the bit.
RUN chmod +x /opt/app/epaper-dashboard-rust

EXPOSE 8099
CMD ["/run.sh"]
