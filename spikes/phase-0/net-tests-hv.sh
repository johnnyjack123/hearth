#!/usr/bin/env bash
# Extra reachability checks for the Hyper-V machine, whose network is provided
# by gvproxy (user-mode networking) instead of WSL's virtual switch.
# gvproxy conventionally exposes 192.168.127.1 (gateway/DNS) and
# 192.168.127.254 (forwarded to the Windows host's 127.0.0.1), which would let
# containers reach services the user bound to localhost only.
# Usage (inside a client container): net-tests-hv.sh <label>
set -u
LABEL="$1"
out() { printf 'NET;%s;%s;%s\n' "$LABEL" "$1" "$2"; }
tcp() {
  if timeout 4 bash -c "exec 3<>/dev/tcp/$2/$3" 2>/dev/null; then out "$1 tcp $2:$3" "OPEN"; else out "$1 tcp $2:$3" "blocked/unreachable"; fi
}

out "hosts host.containers.internal" "$(getent hosts host.containers.internal | tr -s ' ' | tr '\n' ' ')"
out "hosts host.docker.internal" "$(getent hosts host.docker.internal | tr -s ' ' | tr '\n' ' ')"
out "default route" "$(ip route 2>/dev/null | head -2 | tr '\n' ' ')"
HOSTIP=$(getent hosts host.containers.internal | awk '{print $1}')
for port in 8080 5173 55432 445; do
  [[ -n "$HOSTIP" ]] && tcp "host.containers.internal" "$HOSTIP" "$port"
  tcp "gvproxy host-loopback" 192.168.127.254 "$port"
done
tcp "gvproxy gateway dns" 192.168.127.1 53
tcp "windows host via LAN IP SMB" 192.168.1.43 445
tcp "LAN router http" 192.168.1.1 80
tcp "tailscale MagicDNS" 100.100.100.100 53
tcp "cloud metadata" 169.254.169.254 80
tcp "internet 1.1.1.1" 1.1.1.1 443
