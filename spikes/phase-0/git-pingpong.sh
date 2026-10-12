#!/usr/bin/env bash
# Measures `git status` when Windows git and Linux git (inside the Podman VM, on the
# 9p/drvfs mount) take turns on the same working tree. Each side writes stat data the
# other side does not trust, which forces content re-hashing.
# Usage (Git Bash on the host): git-pingpong.sh <repo dir under spikes/phase-0/work>
set -uo pipefail
REPO_HOST="$1"
REPO_WIN="$(cygpath -w "$REPO_HOST")"
REPO_VM="/mnt/$(cygpath -u "$REPO_HOST" | sed -E 's#^/([a-z])/#\1/#')"

secs() { awk -v a="$1" -v b="$2" 'BEGIN{printf "%.3f", b-a}'; }

win() { # label
  local s e
  s=$(date +%s.%N); git -C "$REPO_WIN" -c safe.directory='*' status --porcelain -uno >/dev/null 2>&1; e=$(date +%s.%N)
  echo "RESULT;pingpong;$1;$(secs "$s" "$e")"
}

lin() { # label git-args...
  local label="$1"; shift
  local out
  out=$(MSYS_NO_PATHCONV=1 wsl.exe -d podman-harness-spike -u user --cd "$REPO_VM" -e \
    bash -c 's=$(date +%s.%N); git "$@" >/dev/null 2>&1; e=$(date +%s.%N); awk -v a=$s -v b=$e "BEGIN{printf \"%.3f\", b-a}"' _ "$@")
  echo "RESULT;pingpong;$label;$out"
}

STATUS=(status --porcelain -uno)
MIN=(-c core.checkStat=minimal -c core.trustctime=false)

win  "A1 windows git (baseline)"
win  "A2 windows git"
lin  "B1 linux, read-only index (no-optional-locks)" --no-optional-locks "${STATUS[@]}"
lin  "B2 linux, read-only index (no-optional-locks)" --no-optional-locks "${STATUS[@]}"
lin  "C1 linux, minimal stat, read-only index" --no-optional-locks "${MIN[@]}" "${STATUS[@]}"
lin  "C2 linux, minimal stat, read-only index" --no-optional-locks "${MIN[@]}" "${STATUS[@]}"
lin  "D1 linux, writes index" "${STATUS[@]}"
lin  "D2 linux, writes index" "${STATUS[@]}"
win  "E1 windows git after linux wrote index"
win  "E2 windows git"
lin  "F1 linux, minimal stat, writes index" "${MIN[@]}" "${STATUS[@]}"
win  "G1 windows git after linux(minimal) wrote index"
lin  "H1 linux, minimal stat, read-only index" --no-optional-locks "${MIN[@]}" "${STATUS[@]}"
