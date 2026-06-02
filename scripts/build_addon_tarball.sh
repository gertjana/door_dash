#!/usr/bin/env bash
# Build a clean tarball of the addon for upload to Home Assistant.
#
# Output: dist/epaper_dashboard.tar.gz
#
# The HA host fetches this tarball (e.g. via `wget` from a static file
# server) and unpacks it into /addons/epaper_dashboard. Supervisor then
# notices the new `version:` in config.yaml and offers the update.
#
# Usage:
#   scripts/build_addon_tarball.sh
#
# The script intentionally:
#   - excludes Python bytecode caches (smaller tar, no stale .pyc)
#   - excludes the dev .env and any local data
#   - prints the version embedded in the produced tar so you can sanity-
#     check it before uploading (this is the bit that bit us last time:
#     the served tarball lagged the source)

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

addon_dir="addon"
out_dir="dist"
out_tar="${out_dir}/epaper_dashboard.tar.gz"

if [[ ! -f "${addon_dir}/config.yaml" ]]; then
  echo "ERROR: ${addon_dir}/config.yaml not found — run from repo root?" >&2
  exit 1
fi

mkdir -p "${out_dir}"

# Strip caches from the working tree so we don't ship them. Safe: these
# are generated artifacts that Python will recreate.
find "${addon_dir}" -type d -name '__pycache__' -prune -exec rm -rf {} +
find "${addon_dir}" -type f -name '*.pyc' -delete

# Build the tarball. Contents are relative to addon/ so unpacking with
# `tar -xzf … -C /addons/epaper_dashboard` lands files directly (no
# extra `addon/` wrapper directory).
tar -czf "${out_tar}" \
    --exclude='.DS_Store' \
    --exclude='*.pyc' \
    --exclude='__pycache__' \
    --exclude='.env' \
    -C "${addon_dir}" .

# Verify by extracting just config.yaml from the produced tar and
# reading the version line out of it. This catches the case where the
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
echo "  wget -O /tmp/epaper.tar.gz http://<server>:8765/epaper_dashboard.tar.gz"
echo "  rm -rf /addons/epaper_dashboard && mkdir -p /addons/epaper_dashboard"
echo "  tar -xzf /tmp/epaper.tar.gz -C /addons/epaper_dashboard"
echo "  # then in HA: Supervisor → Local add-ons → Check for updates"
