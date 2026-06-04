#!/usr/bin/with-contenv sh
# s6-overlay strips env vars from services by default; `with-contenv`
# re-injects the container env so we get SUPERVISOR_TOKEN and friends.
# Mirrors the Python add-on's launcher.
set -e

cd /opt/app

# RUST_LOG defaults to "info" so we get the same level of operational
# detail as the Python addon's logging.basicConfig(level=INFO). Override
# via add-on options or the supervisor's environment if you need debug.
: "${RUST_LOG:=info}"
export RUST_LOG

exec /opt/app/epaper-dashboard-rust
