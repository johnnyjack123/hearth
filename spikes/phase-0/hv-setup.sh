#!/usr/bin/env bash
# Prepares the Hyper-V machine (harness-hv) for bench-run.sh: fills the
# bench-cache volume (vite bare mirror + app template) and warms the npm cache,
# mirroring what was set up by hand on the WSL machine.
# Usage: PODMAN_SSH_PORT=64411 PODMAN_SSH_USER=core ./hv-setup.sh
set -euo pipefail
export MSYS_NO_PATHCONV=1

SPIKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PM="$SPIKE_DIR/pm.sh"
IMAGE=docker.io/library/node:24-bookworm
VM_WORK="${BENCH_VM_WORK:-/mnt/work}"

"$PM" pull -q "$IMAGE" >/dev/null
for volume in bench-cache bench-npmcache; do
  "$PM" volume rm -f "$volume" >/dev/null 2>&1 || true
  "$PM" volume create "$volume" >/dev/null
done

# Copy inputs from the 9p share onto VM volumes, so the benchmark measures the
# location under test and not the share it was seeded from.
"$PM" run --rm --userns=keep-id --user 1000:1000 \
  -v "$VM_WORK:/src:ro" -v bench-cache:/cache:U "$IMAGE" sh -c '
    cp -r /src/vite-mirror.git /cache/vite.git &&
    mkdir -p /cache/app-template &&
    cd /src/native/app &&
    tar --exclude=node_modules --exclude=dist -cf - . | tar -xf - -C /cache/app-template &&
    echo "cache: $(du -sh /cache | cut -f1)"'

# Warm npm cache: npm ci once in a throwaway directory.
"$PM" run --rm --userns=keep-id --user 1000:1000 -e HOME=/tmp -e npm_config_update_notifier=false \
  -v bench-cache:/cache:ro -v bench-npmcache:/npmcache:U "$IMAGE" sh -c '
    cp -r /cache/app-template /tmp/app && cd /tmp/app &&
    npm ci --no-audit --no-fund --loglevel=error --cache /npmcache &&
    echo "npm cache warmed: $(du -sh /npmcache | cut -f1)"'
