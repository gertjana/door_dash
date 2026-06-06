#!/usr/bin/env bash
# Cross-compile the Rust addon binary for every HA-supported arch.
#
# Output:
#   addon_rust/prebuilt/aarch64/epaper-dashboard-rust
#   addon_rust/prebuilt/amd64/epaper-dashboard-rust
#
# These binaries are bundled into the tarball produced by
# scripts/build_addon_rust_tarball.sh, and then COPYd by
# addon_rust/Dockerfile when HA's add-on builder runs on the Pi.
# That way the Pi never has to re-compile Rust on update.
#
# Why use docker buildx instead of `cargo build --target …`?
#   * No native cross-compile toolchain needed on macOS.
#   * Re-uses the exact same `rust:1.90-alpine` builder stage we
#     trust — the produced binary is byte-for-byte equivalent to
#     what HA would have compiled on-host.
#   * buildx caches layers, so iterating only re-runs the cargo
#     compile step (~30 s after warm cache vs. ~5 min cold).
#   * Both arches build natively under qemu — slow on amd64 (we're
#     on aarch64 silicon) but still much faster than asking the
#     Pi to do it.
#
# Usage:
#   scripts/build_addon_rust_binaries.sh                 # both arches
#   scripts/build_addon_rust_binaries.sh aarch64         # one arch
#   scripts/build_addon_rust_binaries.sh aarch64 amd64
#
# Requirements: docker (>= 20.10) with buildx plugin. On Apple
# silicon, qemu is wired up automatically by Docker Desktop.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

addon_dir="addon_rust"
out_root="${addon_dir}/prebuilt"

if ! command -v docker >/dev/null 2>&1; then
  echo "ERROR: docker not on PATH — install Docker Desktop or set up an engine" >&2
  exit 1
fi

if ! docker buildx version >/dev/null 2>&1; then
  echo "ERROR: docker buildx not available — update Docker (>= 20.10)" >&2
  exit 1
fi

# Map HA add-on `arch` names to docker buildx `--platform` values.
# HA's BUILD_ARCH for these are exactly: aarch64, amd64. Using a
# function instead of `declare -A` because macOS still ships bash
# 3.2 by default (no associative arrays).
platform_for() {
  case "$1" in
    aarch64) echo "linux/arm64" ;;
    amd64)   echo "linux/amd64" ;;
    *)       return 1 ;;
  esac
}

# Default to building everything; allow narrowing via CLI args.
if [[ $# -eq 0 ]]; then
  arches=(aarch64 amd64)
else
  arches=("$@")
fi

# Ensure a buildx builder with multi-platform support exists. The
# default `desktop-linux` builder on Docker Desktop already has it,
# but on a CI Linux box we need to create one explicitly.
if ! docker buildx inspect epdash-builder >/dev/null 2>&1; then
  docker buildx create --name epdash-builder --use --bootstrap >/dev/null
else
  docker buildx use epdash-builder >/dev/null
fi

for arch in "${arches[@]}"; do
  if ! platform=$(platform_for "$arch"); then
    echo "ERROR: unknown arch '$arch' (expected: aarch64 | amd64)" >&2
    exit 1
  fi

  out_dir="${out_root}/${arch}"
  mkdir -p "$out_dir"

  echo
  echo "=== Building ${arch} (${platform}) ==="
  docker buildx build \
    --file "${addon_dir}/Dockerfile.builder" \
    --platform "${platform}" \
    --target export \
    --output "type=local,dest=${out_dir}" \
    "${addon_dir}"

  bin="${out_dir}/epaper-dashboard-rust"
  if [[ ! -f "$bin" ]]; then
    echo "ERROR: ${bin} missing after build — check buildx output above" >&2
    exit 1
  fi

  bytes="$(wc -c < "$bin" | tr -d ' ')"
  # `file` reports the embedded ELF arch — useful sanity check that
  # the buildx --platform actually took effect.
  fileinfo="$(file "$bin" 2>/dev/null || true)"
  echo "  -> ${bin}"
  echo "     size: ${bytes} bytes"
  echo "     file: ${fileinfo#${bin}: }"
done

echo
echo "All requested architectures built. Next:"
echo "  scripts/build_addon_rust_tarball.sh    # bundles prebuilt/* into the tarball"
