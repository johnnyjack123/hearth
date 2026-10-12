#!/usr/bin/env bash
# Wrapper around podman.exe for the Phase-0 spike.
#
# Why: the user's ~/.ssh/known_hosts contains lines without a host pattern,
# which makes Podman's Go SSH client abort ("knownhosts: missing host pattern")
# before it ever talks to the machine. Instead of touching that file, we point
# podman at an isolated HOME whose known_hosts only holds the machine's host key
# (created with ssh-keyscan) and pass the connection explicitly.
#
# Machines (env):
#   WSL machine harness-spike (default):  PODMAN_SSH_PORT=63481 PODMAN_SSH_USER=user
#   Hyper-V machine harness-hv:           PODMAN_SSH_PORT=64411 PODMAN_SSH_USER=core
# Container commands over SSH need no admin rights, even for Hyper-V machines.
set -euo pipefail

SPIKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${PODMAN_SSH_PORT:-63481}"
SSH_USER="${PODMAN_SSH_USER:-user}"
# One known_hosts per machine: each VM has its own host key.
FAKE_HOME="$SPIKE_DIR/work/podman-home-$PORT"

if [[ ! -s "$FAKE_HOME/.ssh/known_hosts" ]]; then
  mkdir -p "$FAKE_HOME/.ssh"
  ssh-keyscan -p "$PORT" 127.0.0.1 2>/dev/null > "$FAKE_HOME/.ssh/known_hosts"
fi

WIN_HOME="$(cygpath -w "$FAKE_HOME")"
export USERPROFILE="$WIN_HOME" HOME="$WIN_HOME"
export CONTAINER_HOST="ssh://$SSH_USER@127.0.0.1:$PORT/run/user/1000/podman/podman.sock"
export CONTAINER_SSHKEY='C:\Users\<user>\.local\share\containers\podman\machine\machine'
export MSYS_NO_PATHCONV=1

exec "/c/Program Files/RedHat/Podman/podman.exe" "$@"
