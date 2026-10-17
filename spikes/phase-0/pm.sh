#!/usr/bin/env bash
# Wrapper around podman.exe for the Phase-0 spike.
#
# Why: the user's ~/.ssh/known_hosts contains lines without a host pattern,
# which makes Podman's Go SSH client abort ("knownhosts: missing host pattern")
# before it ever talks to the machine. Instead of touching that file, we point
# podman at an isolated HOME whose known_hosts only holds the machine's host key
# (created with ssh-keyscan) and pass the connection explicitly.
#
# Machine selection (env):
#   default: Hyper-V machine "harness"; port and user are read from its machine JSON
#   PODMAN_MACHINE=<name> picks another Hyper-V machine,
#   PODMAN_SSH_PORT / PODMAN_SSH_USER override both explicitly.
# Container commands over SSH need no admin rights, even for Hyper-V machines.
set -euo pipefail

SPIKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MACHINE_JSON="$USERPROFILE/.config/containers/podman/machine/hyperv/${PODMAN_MACHINE:-harness}.json"
# Once the VM service (harness vmd install) owns the machine, its JSON lives
# in the service's directory; the user may read it.
SERVICE_JSON="${SYSTEMDRIVE:-C:}/ProgramData/llm-harness/vmd/home/.config/containers/podman/machine/hyperv/${PODMAN_MACHINE:-harness}.json"
[[ -f "$SERVICE_JSON" ]] && MACHINE_JSON="$SERVICE_JSON"
if [[ -z "${PODMAN_SSH_PORT:-}" && -f "$MACHINE_JSON" ]]; then
  # The port changes with every `machine init`, so never hard-code it.
  read -r PODMAN_SSH_PORT PODMAN_SSH_USER < <(node -e "const c=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));console.log(c.SSH.Port, c.SSH.RemoteUsername)" "$MACHINE_JSON")
fi
PORT="${PODMAN_SSH_PORT:?no machine config found; set PODMAN_SSH_PORT}"
SSH_USER="${PODMAN_SSH_USER:-core}"
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
