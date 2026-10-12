#!/usr/bin/env bash
# Network reachability checks, executed INSIDE a client container.
# Usage: net-tests.sh <label> [proxy-url]
# Prints: NET;<label>;<test>;<result>
set -u
LABEL="$1"; PROXY="${2:-}"

out() { printf 'NET;%s;%s;%s\n' "$LABEL" "$1" "$2"; }

tcp() { # name host port
  if timeout 4 bash -c "exec 3<>/dev/tcp/$2/$3" 2>/dev/null; then out "$1 tcp $2:$3" "OPEN"; else out "$1 tcp $2:$3" "blocked/unreachable (rc=$?)"; fi
}

http() { # name url [extra curl args]
  local name="$1" url="$2"; shift 2
  local r
  r=$(curl -sS -o /dev/null -m 8 -w '%{http_code}' "$@" "$url" 2>&1 | tr '\n' ' ')
  out "$name $url" "$r"
}

out "hosts host.containers.internal" "$(getent hosts host.containers.internal | tr -s ' ' | tr '\n' ' ')"
out "resolv.conf" "$(grep nameserver /etc/resolv.conf | tr '\n' ' ')"
out "dns github.com" "$(getent hosts github.com | head -1 | tr -s ' ' || true)"
out "dns example.com" "$(getent hosts example.com | head -1 | tr -s ' ' || true)"
out "default route" "$(ip route 2>/dev/null | head -2 | tr '\n' ' ' || cat /proc/net/route | sed -n 2p)"

http "direct" https://github.com
http "direct-ip" https://1.1.1.1

if [[ -n "$PROXY" ]]; then
  P="${PROXY#http://}"
  out "dns proxy" "$(getent hosts "${P%%:*}" | tr -s ' ' || true)"
  http "proxy allowed" https://registry.npmjs.org/react -x "$PROXY"
  http "proxy allowed" https://github.com -x "$PROXY"
  http "proxy denied" https://example.com -x "$PROXY"
  http "proxy denied (subdomain)" https://api.github.com -x "$PROXY"
  http "proxy denied (plain http)" http://example.com -x "$PROXY"
  http "proxy denied (ip)" https://1.1.1.1 -x "$PROXY"
  http "proxy -> windows host" http://172.17.0.1:1234 -x "$PROXY"
  http "proxy -> LAN router" http://192.168.1.1 -x "$PROXY"
  v=$(HTTPS_PROXY="$PROXY" HTTP_PROXY="$PROXY" npm_config_update_notifier=false timeout 30 npm view react version --cache /tmp/npmc 2>&1 | tail -1)
  out "npm view react (via env proxy)" "$v"
  g=$(HTTPS_PROXY="$PROXY" timeout 30 git ls-remote https://github.com/vitejs/vite HEAD 2>&1 | cut -c1-60 | tail -1)
  out "git ls-remote github (via env proxy)" "$g"
  g=$(HTTPS_PROXY="$PROXY" timeout 30 git ls-remote https://gitlab.com/gitlab-org/gitlab-foss.git HEAD 2>&1 | cut -c1-80 | tail -1)
  out "git ls-remote gitlab (via env proxy, denied)" "$g"
fi

HOSTIP=$(getent hosts host.containers.internal | awk '{print $1}')
[[ -n "$HOSTIP" ]] && tcp "host.containers.internal(sshd)" "$HOSTIP" 22
tcp "windows host (WSL vEthernet) RPC" 172.17.0.1 135
tcp "windows host (WSL vEthernet) NetBIOS" 172.17.0.1 139
tcp "windows host (WSL vEthernet) SMB" 172.17.0.1 445
tcp "windows host (WSL vEthernet) node 0.0.0.0" 172.17.0.1 1234
tcp "windows host via LAN IP SMB" 192.168.1.43 445
tcp "VM eth0 sshd" 172.17.10.117 22
tcp "WSL DNS" 10.255.255.254 53
tcp "LAN router http" 192.168.1.1 80
tcp "LAN router dns" 192.168.1.1 53
tcp "tailscale MagicDNS" 100.100.100.100 53
tcp "cloud metadata" 169.254.169.254 80
tcp "internet 1.1.1.1" 1.1.1.1 443
