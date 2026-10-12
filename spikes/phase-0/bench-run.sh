#!/usr/bin/env bash
# Phase-0 file I/O benchmark orchestrator (runs in Git Bash on the Windows host).
# Usage: bench-run.sh <location> [<location> ...]
#   locations: bind | volume | containerfs | hybrid
# Each location gets its own long-running container; every step runs via `podman exec`
# and reports its own wall-clock time from inside the container (no SSH overhead).
set -uo pipefail
export MSYS_NO_PATHCONV=1

SPIKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PM="$SPIKE_DIR/pm.sh"
WORK="$SPIKE_DIR/work"
OUT="$WORK/bench-results.txt"
IMAGE=docker.io/library/node:24-bookworm
SCRIPTS_WIN="$(cygpath -w "$SPIKE_DIR")"
BIND_WIN="$(cygpath -w "$WORK")\\bind"
HYBRID_WIN="$(cygpath -w "$WORK")\\hybrid"

# Hyper-V machines only share the work directory (mounted at $BENCH_VM_WORK in
# the VM), so mounts use VM-side paths and the scripts are copied into work/.
if [[ -n "${BENCH_VM_WORK:-}" ]]; then
  mkdir -p "$WORK/scripts"
  cp "$SPIKE_DIR/bench-step.sh" "$SPIKE_DIR/smallfiles.mjs" "$WORK/scripts/"
  SCRIPTS_WIN="$BENCH_VM_WORK/scripts"
  BIND_WIN="$BENCH_VM_WORK/bind"
  HYBRID_WIN="$BENCH_VM_WORK/hybrid"
fi

drop_vm_caches() {
  if [[ "${PODMAN_SSH_USER:-user}" == core ]]; then
    ssh -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i "$HOME/.local/share/containers/podman/machine/machine" \
      -p "${PODMAN_SSH_PORT}" core@127.0.0.1 "sudo sh -c 'sync; echo 3 > /proc/sys/vm/drop_caches'"
  else
    wsl.exe -d podman-harness-spike -u root -- sh -c 'sync; echo 3 > /proc/sys/vm/drop_caches'
  fi
}

run_location() {
  local loc="$1" ctr="bench-$1" bench_dir="/bench"
  local -a mounts=(-v bench-cache:/cache:ro -v bench-npmcache:/npmcache -v "$SCRIPTS_WIN:/scripts:ro")
  case "$loc" in
    bind)        mkdir -p "$WORK/bind"; mounts+=(-v "$BIND_WIN:/bench") ;;
    volume)      "$PM" volume rm -f bench-vol >/dev/null 2>&1; "$PM" volume create bench-vol >/dev/null
                 mounts+=(-v bench-vol:/bench:U) ;;
    containerfs) bench_dir="/home/node/bench" ;;
    hybrid)      mkdir -p "$WORK/hybrid/app/node_modules"
                 "$PM" volume rm -f bench-nm >/dev/null 2>&1; "$PM" volume create bench-nm >/dev/null
                 mounts+=(-v "$HYBRID_WIN:/bench" -v bench-nm:/bench/app/node_modules:U) ;;
    *) echo "unknown location $loc" >&2; return 1 ;;
  esac

  "$PM" rm -f "$ctr" >/dev/null 2>&1
  "$PM" run -d --name "$ctr" --userns=keep-id --user 1000:1000 -e HOME=/tmp \
    -e npm_config_update_notifier=false "${mounts[@]}" "$IMAGE" sleep infinity >/dev/null || return 1
  "$PM" exec "$ctr" mkdir -p "$bench_dir"

  step() { "$PM" exec -e BENCH_DIR="$bench_dir" -e LABEL="$loc" "$ctr" bash /scripts/bench-step.sh "$@" 2>&1 | tee -a "$OUT"; }

  echo "=== $loc ($(date -Iseconds)) ===" | tee -a "$OUT"
  if [[ "$loc" != hybrid ]]; then
    for i in 1 2 3; do step smallfiles; done
    step clone_net
    step rm_vite
    step clone_local
    step rm_vite
    step clone_local
    drop_vm_caches
    step git_status cold
    for i in 1 2 3 4 5; do step git_status warm; done
  fi
  step app_prepare
  step npm_ci 1
  step npm_ci 2
  step build 1
  step build 2
  step test 1
  step test 2
  step rm_node_modules
  "$PM" rm -f "$ctr" >/dev/null 2>&1
}

for loc in "$@"; do run_location "$loc"; done
