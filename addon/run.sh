#!/usr/bin/with-contenv sh
# s6-overlay (built into HA base images) strips env vars from services by
# default; `with-contenv` re-injects the container env so we get
# SUPERVISOR_TOKEN and friends.
set -e
cd /opt/app
exec python -m uvicorn app.main:app --host 0.0.0.0 --port 8099
