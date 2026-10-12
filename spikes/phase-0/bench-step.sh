#!/usr/bin/env bash
# One benchmark step, executed INSIDE the benchmark container.
# Usage: bench-step.sh <step> [args]
# Env:   BENCH_DIR (target directory, e.g. /bench), LABEL (location label)
# Prints: RESULT;<label>;<step>;<seconds>;<extra>
set -uo pipefail

: "${BENCH_DIR:?BENCH_DIR not set}"
: "${LABEL:=unknown}"
STEP="$1"; shift || true

now() { date +%s.%N; }
report() { # step seconds extra
  printf 'RESULT;%s;%s;%.3f;%s\n' "$LABEL" "$1" "$2" "${3:-}"
}
timed() { # name cmd...
  local name="$1"; shift
  local t0 t1 rc
  t0=$(now)
  "$@" >/tmp/step.out 2>&1; rc=$?
  t1=$(now)
  if [[ $rc -ne 0 ]]; then
    echo "--- step $name failed (rc=$rc):" >&2
    tail -n 20 /tmp/step.out >&2
  fi
  report "$name" "$(awk -v a="$t0" -v b="$t1" 'BEGIN{print b-a}')" "rc=$rc"
  return $rc
}

case "$STEP" in
  smallfiles)
    # 5,000 files of 1 KiB in 50 directories: create, read, stat, delete (see smallfiles.mjs).
    node /scripts/smallfiles.mjs "$BENCH_DIR" "$LABEL"
    ;;

  clone_net)
    rm -rf "$BENCH_DIR/vite"
    timed clone_net git clone -q --depth 1 https://github.com/vitejs/vite "$BENCH_DIR/vite"
    ;;

  clone_local)
    # Clone from a local bare mirror (on a volume) to isolate checkout I/O from network.
    rm -rf "$BENCH_DIR/vite"
    timed clone_local git clone -q --depth 1 file:///cache/vite.git "$BENCH_DIR/vite"
    n=$(git -C "$BENCH_DIR/vite" ls-files | wc -l)
    echo "INFO;$LABEL;vite tracked files=$n"
    ;;

  git_status)
    # $1 = label suffix (cold|warm)
    kind="${1:-warm}"
    timed "git_status_$kind" git -C "$BENCH_DIR/vite" status --porcelain
    ;;

  rm_vite)
    timed rm_vite rm -rf "$BENCH_DIR/vite"
    ;;

  app_prepare)
    # Copy the create-vite react-ts template + a prepared lockfile into $BENCH_DIR/app.
    APP="$BENCH_DIR/app"
    mkdir -p "$APP"
    find "$APP" -mindepth 1 -maxdepth 1 ! -name node_modules -exec rm -rf {} +
    cp -r /cache/app-template/. "$APP/"
    rm -rf "$APP/node_modules/"* "$APP/node_modules/".[!.]* 2>/dev/null || true
    echo "INFO;$LABEL;app prepared"
    ;;

  npm_ci)
    # Warm npm cache on /npmcache: measures extraction/linking I/O, not network.
    cd "$BENCH_DIR/app" || exit 1
    rm -rf node_modules/* node_modules/.[!.]* 2>/dev/null || true
    timed "npm_ci_${1:-1}" npm ci --prefer-offline --no-audit --no-fund --loglevel=error --cache /npmcache
    n=$(find node_modules -type f 2>/dev/null | wc -l)
    echo "INFO;$LABEL;node_modules files=$n"
    ;;

  build)
    cd "$BENCH_DIR/app" || exit 1
    timed "build_${1:-1}" npm run build
    ;;

  test)
    cd "$BENCH_DIR/app" || exit 1
    timed "test_${1:-1}" npx vitest run
    ;;

  rm_node_modules)
    cd "$BENCH_DIR/app" || exit 1
    rmnm() { rm -rf node_modules/* node_modules/.[!.]* 2>/dev/null; true; }
    timed rm_node_modules rmnm
    ;;

  *)
    echo "unknown step $STEP" >&2; exit 2 ;;
esac
