# syntax=docker/dockerfile:1.6
#
# Cross-compile builder Dockerfile.
#
# Used by `scripts/build_addon_rust_binaries.sh` together with
# `docker buildx --platform linux/arm64,linux/amd64 --output type=local`
# to produce statically-linked musl binaries that ship pre-compiled
# inside the addon tarball.
#
# Why a separate file from the main Dockerfile?
#   * The main Dockerfile (consumed by HA's add-on builder on the Pi)
#     just COPYs a prebuilt binary — that's the whole point of this
#     refactor: avoid making the Pi re-compile ~150 crates on every
#     update. Keeping the cross-compile path in its own file means the
#     HA addon never accidentally re-runs cargo at deploy time.
#   * `FROM scratch AS export` at the bottom is a buildx export trick:
#     when you pass `--output type=local,dest=…`, buildx writes the
#     filesystem of the *final* stage to disk. Using `scratch` means
#     just the binary lands on disk, with no junk.
#
# Inputs (build context = addon_rust/):
#   * Cargo.toml, Cargo.lock — pin dependencies
#   * src/, assets/, config.yaml — the actual source tree
#
# Outputs (one file per --platform invocation):
#   * /epaper-dashboard-rust — stripped release binary

ARG RUST_VERSION=1.90

FROM rust:${RUST_VERSION}-alpine AS builder

# musl-dev / build-base: a couple of -sys crates compile a tiny C
# shim, and even the pure-Rust ones occasionally probe for `cc`.
RUN apk add --no-cache build-base musl-dev

WORKDIR /build

# Phase 1: compile dependencies only. By copying just the manifests
# and a stub main.rs, cargo populates target/release/deps with all
# the crate compile artifacts. This layer is cached as long as
# Cargo.toml + Cargo.lock are unchanged, so iterating on src/
# afterwards skips the multi-minute dependency compile.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release --locked && \
    rm -rf src target/release/deps/epaper_dashboard_rust*

# Phase 2: real source build. assets/ contains TTFs and PNG icons
# pulled in via `include_bytes!`; config.yaml is referenced from
# `src/lib.rs` via `include_str!` to bake ADDON_VERSION into the
# binary at compile time.
COPY src ./src
COPY assets ./assets
COPY config.yaml ./config.yaml
RUN cargo build --release --locked && \
    strip target/release/epaper-dashboard-rust

# Export-only stage. `docker buildx --output type=local,dest=...`
# extracts the filesystem of the final stage to the host. Using
# scratch ensures only the single file we care about is exported.
FROM scratch AS export
COPY --from=builder /build/target/release/epaper-dashboard-rust /epaper-dashboard-rust
