#!/usr/bin/env bash
# Build a clean tarball of the Rust addon for upload to Home Assistant.
#
# Output: dist/epaper_dashboard_rust.tar.gz
#
# The HA host fetches this tarball (e.g. via `wget` from a static
# file server) and unpacks it into /addons/epaper_dashboard_rust.
# Supervisor then notices the new `version:` in config.yaml and
# offers the update — at which point HA's add-on builder runs the
# Dockerfile, which compiles the Rust binary inside the per-arch
# Alpine image.
#
# That means the tarball must ship the *sources* (Cargo.toml +
# Cargo.lock + src/ + assets/), not a pre-built binary. Build
# artifacts in target/ are local-dev-only and would just bloat the
# tar.
#
# Usage:
#   scripts/build_addon_rust_tarball.sh
#
# Mirrors scripts/build_addon_tarball.sh (Python addon) so the
# release flow is identical apart from the slug.

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
  echo "ERROR: ${addon_dir}/Cargo.lock missing — Dockerfile uses --locked" >&2
  exit 1
fi

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
#   * target/        — local cargo build cache, ~500 MB; HA rebuilds it
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

echo "Built ${out_tar}"
echo "  version: ${version}"
echo "  size:    ${bytes} bytes"
echo
echo "Next: serve dist/ on the static file server that the HA host"
echo "downloads from (port 8765 in your case), then on the HA host:"
echo "  wget -O /tmp/epaper_rust.tar.gz http://<server>:8765/epaper_dashboard_rust.tar.gz"
echo "  rm -rf /addons/epaper_dashboard && mkdir -p /addons/epaper_dashboard"
echo "  tar -xzf /tmp/epaper_rust.tar.gz -C /addons/epaper_dashboard"
echo "  # then in HA: Supervisor → Local add-ons → Check for updates"
echo
echo "Note: the Rust addon ships under slug 'epaper_dashboard' (same as"
echo "the Python addon) so HA preserves your existing options across"
echo "the upgrade. To roll back, re-extract dist/epaper_dashboard.tar.gz"
echo "into /addons/epaper_dashboard."
