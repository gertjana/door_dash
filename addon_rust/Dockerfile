# syntax=docker/dockerfile:1.6
#
# Multi-stage build:
#   1. `builder` — pulls the official Rust musl-Alpine image, builds the
#      release binary statically against musl libc.
#   2. final — uses Home Assistant's tiny Alpine base image and just
#      copies the compiled binary in. The HA add-on builder swaps
#      $BUILD_FROM per architecture (see build.json).
#
# Why multi-stage?
#   * Build-stage tooling (rustc, cargo, ~1 GB of crates index/registry)
#     never ends up in the published image — final image is ~10-15 MB.
#   * Each arch is built natively under qemu/buildx, so we get matching
#     binaries for aarch64, amd64 and armv7 without any cross-compilation
#     dance.
#
# A note on caching: Cargo's incremental store is bind-mounted via
# `--mount=type=cache,target=/usr/local/cargo/registry` so dependency
# fetches survive across rebuilds, which dramatically speeds up local
# iteration. The HA add-on builder doesn't honour this mount, so cold
# CI builds still take a few minutes — that's fine for a release flow.

ARG RUST_VERSION=1.83
FROM rust:${RUST_VERSION}-alpine AS builder

# musl-dev: needed because some -sys crates compile a C shim. Even the
# pure-Rust ones occasionally pull in a compiler check that requires
# `cc` to exist — `build-base` covers that. We strip the binary at
# build time to get release-grade size without an extra `strip` step.
RUN apk add --no-cache build-base musl-dev

WORKDIR /build

# Copy manifests first so dependency resolution caches well even if
# only source files change. Cargo's "fetch and compile dependencies as
# a separate step" trick relies on a placeholder src/main.rs.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release --locked && \
    rm -rf src target/release/deps/epaper_dashboard_rust*

# Now bring in the real sources (incl. asset directory referenced by
# include_bytes! macros) and build for real. Anything in `assets/` is
# part of the binary at compile time, so changes there trigger a
# rebuild — same as a source change.
COPY src ./src
COPY assets ./assets
RUN cargo build --release --locked && \
    strip target/release/epaper-dashboard-rust


# Runtime stage — HA's per-arch Alpine base. Provides s6-overlay so
# our run.sh can use `with-contenv` (mirrors the Python add-on's
# behaviour for SUPERVISOR_TOKEN injection).
ARG BUILD_FROM=ghcr.io/home-assistant/amd64-base:3.20
FROM ${BUILD_FROM}

ENV LANG=C.UTF-8

WORKDIR /opt/app

COPY --from=builder /build/target/release/epaper-dashboard-rust /opt/app/epaper-dashboard-rust
COPY config.yaml /opt/app/config.yaml
COPY run.sh /run.sh
RUN chmod +x /run.sh /opt/app/epaper-dashboard-rust

EXPOSE 8099
CMD ["/run.sh"]
