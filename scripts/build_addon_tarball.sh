#!/usr/bin/env bash
# Build a tarball of the add-on for unpacking under /addons/ on a
# Home Assistant host.
#
# Primary use: produce a *dev* variant that coexists with the
# GitHub-installed release on the same host. Overrides in dev/ are
# deep-merged on top of addon/config.yaml + addon/build.json so the
# dev tarball can have its own slug / name / host port without
# touching the canonical sources — keeping `git push` -> release
# update flow intact.
#
# Layout expected:
#   addon/                        canonical add-on (release tracks this)
#     config.yaml
#     build.json
#     ...
#   dev/                          (optional) overrides for the dev tarball
#     config-override.yaml          deep-merged onto addon/config.yaml
#     build-override.json           deep-merged onto addon/build.json
#
# If dev/ is empty or missing, a vanilla release tarball is produced
# (legacy / air-gapped install path).
#
# Output goes to dist/<merged-slug>.tar.gz so the dev and release
# tarballs never overwrite each other.
#
# Usage:
#   scripts/build_addon_tarball.sh                 # auto-detect dev/ overrides
#   scripts/build_addon_tarball.sh --no-overrides  # force vanilla release tarball
#
# After building:
#   scp dist/<slug>.tar.gz root@<ha-host>:/tmp/
#   ssh root@<ha-host> '
#       rm -rf /addons/<slug>
#       mkdir -p /addons/<slug>
#       tar -xzf /tmp/<slug>.tar.gz -C /addons/<slug>
#   '
#   # then in HA: Settings -> Add-ons -> Store -> ⋮ -> Check for updates

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

addon_dir="addon"
overrides_dir="dev"
out_dir="dist"
merge_helper="scripts/_merge_addon_config.py"

apply_overrides=1
if [[ "${1:-}" == "--no-overrides" ]]; then
  apply_overrides=0
fi

if [[ ! -f "${addon_dir}/config.yaml" ]]; then
  echo "ERROR: ${addon_dir}/config.yaml not found — run from repo root?" >&2
  exit 1
fi

config_override="${overrides_dir}/config-override.yaml"
build_override="${overrides_dir}/build-override.json"

have_config_override=0
have_build_override=0
if (( apply_overrides )); then
  [[ -f "${config_override}" ]] && have_config_override=1
  [[ -f "${build_override}"  ]] && have_build_override=1
fi

# Stage addon/ into a temp dir so we can merge overrides without
# mutating the working tree. Trap ensures it's cleaned up on exit
# even if tar / merge fail half-way.
stage_dir="$(mktemp -d -t epaper_dashboard.XXXXXX)"
trap 'rm -rf "${stage_dir}"' EXIT

# rsync would be nicer but cp -R is portable enough for a flat layout.
cp -R "${addon_dir}/." "${stage_dir}/"

# Strip dev cruft from the staged copy (don't touch the working tree).
# The tar --exclude flags below are belt-and-braces; pruning here means
# we don't waste I/O scanning a 100+ MB venv on every build.
find "${stage_dir}" -type d \
    \( -name '__pycache__' -o -name '.venv' -o -name 'venv' \
       -o -name '*.egg-info' -o -name '.pytest_cache' \
       -o -name '.mypy_cache' -o -name '.ruff_cache' \) \
    -prune -exec rm -rf {} +
find "${stage_dir}" -type f -name '*.pyc' -delete

# Apply overrides in-place on the staged copy.
if (( have_config_override )); then
  python3 "${merge_helper}" \
      "${addon_dir}/config.yaml" \
      "${config_override}" \
      "${stage_dir}/config.yaml"
fi

if (( have_build_override )); then
  if [[ -f "${addon_dir}/build.json" ]]; then
    python3 "${merge_helper}" \
        "${addon_dir}/build.json" \
        "${build_override}" \
        "${stage_dir}/build.json"
  else
    # No base build.json — drop the override straight in.
    cp "${build_override}" "${stage_dir}/build.json"
  fi
fi

# Read merged slug + version from the staged config.yaml; these drive
# the output filename and the unpack hint at the end. Use Python so we
# don't depend on YAML quoting style (PyYAML may emit `version: 1.1.0`
# without quotes — still a valid string per the spec, but awkward to
# grep for).
read_field() {
  python3 -c "
import sys, yaml
with open('${stage_dir}/config.yaml') as f:
    cfg = yaml.safe_load(f)
val = cfg.get('$1', '')
print(val if val is not None else '')
"
}

slug="$(read_field slug)"
version="$(read_field version)"

if [[ -z "${slug}" || -z "${version}" ]]; then
  echo "ERROR: could not parse slug/version from merged config.yaml" >&2
  exit 1
fi

mkdir -p "${out_dir}"
out_tar="${out_dir}/${slug}.tar.gz"

# Build the tarball. Contents are relative to the stage so unpacking
# with `tar -xzf … -C /addons/<slug>` lands files directly (no extra
# wrapper directory).
tar -czf "${out_tar}" \
    --exclude='.DS_Store' \
    --exclude='*.pyc' \
    --exclude='__pycache__' \
    --exclude='.env' \
    --exclude='.venv' \
    --exclude='venv' \
    --exclude='*.egg-info' \
    --exclude='.pytest_cache' \
    --exclude='.mypy_cache' \
    --exclude='.ruff_cache' \
    -C "${stage_dir}" .

bytes="$(wc -c < "${out_tar}" | tr -d ' ')"

echo "Built ${out_tar}"
echo "  slug:    ${slug}"
echo "  version: ${version}"
echo "  size:    ${bytes} bytes"
if (( have_config_override || have_build_override )); then
  echo "  overrides: config=${have_config_override} build=${have_build_override}"
else
  echo "  overrides: none (vanilla release tarball)"
fi

cat <<EOF

Next:
  scp ${out_tar} root@<ha-host>:/tmp/
  ssh root@<ha-host> '
      rm -rf /addons/${slug} &&
      mkdir -p /addons/${slug} &&
      tar -xzf /tmp/$(basename "${out_tar}") -C /addons/${slug}
  '
  # then in HA: Settings -> Add-ons -> Store -> ⋮ -> Check for updates
EOF
