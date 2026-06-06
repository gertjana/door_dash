#!/usr/bin/env bash
# Build a clean tarball of the Rust addon for upload to Home Assistant.
#
# Output: dist/epaper_dashboard_rust.tar.gz
#
# Since v1.0.5 the addon ships a *pre-compiled* musl binary inside
# the tarball. The HA host's add-on builder no longer runs cargo;
# it just COPYs the binary into a per-arch HA Alpine base image.
# That cuts on-Pi `update` time from ~15 min to ~30 s.
#
# This script therefore does two things in order:
#   1. Cross-compile the binaries for every supported arch
#      (scripts/build_addon_rust_binaries.sh — uses docker buildx).
#   2. Bundle the addon source + prebuilt/ + the new minimal
#      Dockerfile + run.sh + config.yaml into a tarball.
#
# Why ship the source as well? Because:
#   * Cargo.toml + Cargo.lock are needed inside the build context if
#     anyone ever wants to rebuild from scratch on the Pi (set
#     `BUILD_FROM` to a rust-alpine image and skip the prebuilt
#     COPY) — useful for security audits / debugging.
#   * `git diff` of /addons/epaper_dashboard against this repo stays
#     meaningful when the user inspects the deployed addon.
#
# Tarball grows from ~1 MB (sources only) to ~12 MB (sources + 2
# binaries). Still well under the 100 MB HA addon-store limit.
#
# Usage:
#   scripts/build_addon_rust_tarball.sh           # rebuilds binaries first
#   SKIP_BINARY_BUILD=1 scripts/build_addon_rust_tarball.sh
#                                                 # reuses existing prebuilt/

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

addon_dir="addon_rust"
out_dir="dist"
out_tar="${out_dir}/epaper_dashboard_rust.tar.gz"

if [[ ! -f "${addon_dir}/config.yaml" ]]; then
  echo "ERROR: ${addon_dir}/config.yaml not found — run from repo root?" >&2
  exit 1
fi

if [[ ! -f "${addon_dir}/Cargo.lock" ]]; then
  echo "ERROR: ${addon_dir}/Cargo.lock missing — Dockerfile.builder uses --locked" >&2
  exit 1
fi

# Step 1: produce prebuilt binaries unless explicitly skipped.
# SKIP_BINARY_BUILD is the iteration knob: if you've just changed
# config.yaml or run.sh and the binary hasn't been touched, no
# point in re-doing the multi-minute cross-compile.
if [[ "${SKIP_BINARY_BUILD:-0}" == "1" ]]; then
  echo "SKIP_BINARY_BUILD=1 — reusing existing ${addon_dir}/prebuilt/"
else
  echo "=== Cross-compiling addon binaries ==="
  "${repo_root}/scripts/build_addon_rust_binaries.sh"
fi

# Sanity: every arch listed in config.yaml must have a prebuilt
# binary on disk, otherwise the tarball would ship a non-functional
# image for that arch.
arches=$(awk '
  /^arch:/ { in_arch = 1; next }
  in_arch && /^[a-zA-Z]/ { in_arch = 0 }
  in_arch && /^[[:space:]]*-[[:space:]]/ { gsub(/^[[:space:]]*-[[:space:]]*/, ""); print }
' "${addon_dir}/config.yaml")

for arch in $arches; do
  bin="${addon_dir}/prebuilt/${arch}/epaper-dashboard-rust"
  if [[ ! -f "$bin" ]]; then
    echo "ERROR: ${bin} missing — listed in config.yaml's arch: but not built" >&2
    echo "Hint: scripts/build_addon_rust_binaries.sh ${arch}" >&2
    exit 1
  fi
done

mkdir -p "${out_dir}"

# Build the tarball. Contents are relative to addon_rust/ so
# unpacking with `tar -xzf … -C /addons/epaper_dashboard` lands
# files directly (no extra wrapper directory).
#
# COPYFILE_DISABLE=1 stops macOS BSD tar from embedding AppleDouble
# metadata files (`._foo`) for every entry. Without it, those
# resource-fork stubs extract as visible files on Linux and break
# HA's addon parser, which globs the dir for config.yaml/build.json
# and tries to parse the binary stubs as YAML/JSON.
#
# Excludes:
#   * target/        — local cargo build cache, ~500 MB; HA never reads it
#   * .DS_Store      — macOS Finder metadata
#   * .env           — dev-only secrets if present
#   * ._*            — belt-and-suspenders against AppleDouble in
#                      case anything sneaks past COPYFILE_DISABLE
COPYFILE_DISABLE=1 tar -czf "${out_tar}" \
    --exclude='.DS_Store' \
    --exclude='.env' \
    --exclude='target' \
    --exclude='._*' \
    -C "${addon_dir}" .

# Verify by extracting just config.yaml from the produced tar and
# reading the version line out of it. Catches the case where the
# source was bumped but a stale tar got cached somewhere.
version="$(tar -xzOf "${out_tar}" config.yaml \
            | awk -F'"' '/^version:/ {print $2}')"

bytes="$(wc -c < "${out_tar}" | tr -d ' ')"

echo
echo "Built ${out_tar}"
echo "  version: ${version}"
echo "  size:    ${bytes} bytes"
echo
echo "Next: serve dist/ on the static file server that the HA host"
echo "downloads from (port 8765 in your case), then on the HA host:"
echo "  ha apps stop local_epaper_dashboard"
echo "  wget -O /tmp/epaper_rust.tar.gz http://<server>:8765/epaper_dashboard_rust.tar.gz"
echo "  tar -xzf /tmp/epaper_rust.tar.gz -C /addons/epaper_dashboard --overwrite"
echo "  ha supervisor reload"
echo "  ha apps update local_epaper_dashboard"
echo
echo "Note: from v1.0.5 onwards the tarball ships a pre-compiled"
echo "binary, so on-Pi update time is ~30 s instead of ~15 min."
echo
echo "To roll back, re-extract dist/epaper_dashboard.tar.gz"
echo "into /addons/epaper_dashboard."
