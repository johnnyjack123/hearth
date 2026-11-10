#!/usr/bin/env bash
# Smoke test of Phase 2 against the real VM, from Git Bash:
#
#   throwaway server + mock LLM -> hearth login (approved over the API)
#   -> hearth run pi --print (answer through llm.hearth) -> a commit and
#   the gateway's rules in hearth shell -> hearth sync + merge
#   -> hearth logout -> hearth run removes pi's wiring again
#
# Needs the running VM, both images (`hearth image build`), `cargo build`
# and `pnpm install`. Uses ports 8093/8094 and a temporary HEARTH_HOME, so
# the real ~/.hearth stays untouched. pi is installed into the shared tools
# volume if it is missing there. Everything the test project created in the
# VM is removed at the end.
set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
H="$REPO/target/debug/hearth"
PM="$REPO/spikes/phase-0/pm.sh"
SERVER_PORT=8093
MOCK_PORT=8094
URL="http://127.0.0.1:$SERVER_PORT"
WORK="$(mktemp -d)"
NAME="smoke-$$"
export HEARTH_HOME="$WORK/home"
# The checks below look for German output, whatever language Windows shows.
export HEARTH_LANG=de

failures=0
check() { # check <label> <command…>
  local label="$1"
  shift
  if "$@"; then
    echo "  ✓ $label"
  else
    echo "  ✗ $label"
    failures=$((failures + 1))
  fi
}

cleanup() {
  (cd "$WORK/$NAME" 2>/dev/null && "$H" stop > /dev/null 2>&1)
  "$PM" rm -f "hearth-$NAME" "hearth-gw-$NAME" > /dev/null 2>&1
  "$PM" volume rm "hearth-ws-$NAME" "hearth-home-$NAME" > /dev/null 2>&1
  "$PM" network rm "hearth-net-$NAME" > /dev/null 2>&1
  rm -rf "$LOCALAPPDATA/hearth/exchange/$NAME"
  powershell.exe -NoProfile -Command "foreach (\$p in $SERVER_PORT,$MOCK_PORT) { Get-NetTCPConnection -LocalPort \$p -State Listen -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id \$_.OwningProcess -Force } }" > /dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "== Server und Mock-LLM"
(cd "$REPO/apps/web" && MOCK_PORT=$MOCK_PORT node e2e/mock-llm.mjs > "$WORK/mock.log" 2>&1) &
(cd "$REPO/apps/server" && DATA_DIR="$WORK/server-data" PORT=$SERVER_PORT PUBLIC_URL="$URL" \
  node_modules/.bin/tsx src/index.ts > "$WORK/server.log" 2>&1) &
for _ in $(seq 60); do curl -s "$URL/api/health" > /dev/null && break; sleep 0.5; done
J='Content-Type: application/json'
O="Origin: $URL"
curl -s -c "$WORK/cookies" -H "$J" -H "$O" \
  -d '{"email":"smoke@example.com","name":"Smoke","password":"correct-horse-battery"}' \
  "$URL/api/auth/sign-up/email" > /dev/null
curl -s -b "$WORK/cookies" -H "$J" -H "$O" \
  -d "{\"kind\":\"openai-compatible\",\"label\":\"Mock\",\"baseUrl\":\"http://127.0.0.1:$MOCK_PORT/v1\",\"apiKey\":\"sk-mock-key\"}" \
  "$URL/api/providers" > /dev/null

echo "== hearth login"
"$H" login "$URL" --no-browser > "$WORK/login.log" 2>&1 &
LOGIN=$!
for _ in $(seq 100); do grep -qE '[A-Z2-9]{4}-[A-Z2-9]{4}' "$WORK/login.log" 2>/dev/null && break; sleep 0.3; done
CODE=$(grep -oE '[A-Z2-9]{4}-[A-Z2-9]{4}' "$WORK/login.log" | head -1)
curl -s -b "$WORK/cookies" -H "$O" "$URL/api/devices/pairing/$CODE" > /dev/null
curl -s -b "$WORK/cookies" -H "$J" -H "$O" -d '{"approve":true}' "$URL/api/devices/pairing/$CODE" > /dev/null
check "Rechner gekoppelt" wait "$LOGIN"

echo "== Test-Repo $NAME"
mkdir -p "$WORK/$NAME" && cd "$WORK/$NAME" || exit 1
git init -q -b main
git config user.name "Smoke User"
git config user.email smoke@example.com
git config core.autocrlf false
echo "# $NAME" > README.md && git add README.md && git commit -qm initial
"$H" project add > /dev/null
check "Projekt angelegt" test -f "$HEARTH_HOME/projects/$NAME.toml"

echo "== hearth run pi (Druckmodus)"
"$H" run pi -- --print "Sag hallo" --provider hearth-mock --model mock-model --no-session \
  > "$WORK/run.log" 2>&1
check "pi antwortet über llm.hearth" grep -q "Testmodell" "$WORK/run.log"

echo "== hearth shell"
printf '%s\n' \
  'echo "vom Agent" > agent.txt && git add agent.txt && git commit -qm "agent: add agent.txt"' \
  'if curl -sS --max-time 20 -o /dev/null https://example.com/ 2>/dev/null; then echo example=reachable; else echo example=blocked; fi' \
  'echo "react=$(npm view react version 2>/dev/null)"' \
  'exit' | "$H" shell > "$WORK/shell.log" 2>&1
check "example.com gesperrt" grep -q "example=blocked" "$WORK/shell.log"
check "npm view react version geht durch" grep -qE "react=[0-9]+\." "$WORK/shell.log"

echo "== hearth sync / merge"
"$H" sync > "$WORK/sync.log" 2>&1
"$H" merge > "$WORK/merge.log" 2>&1
check "Commit des Agents im Host-Branch" test "$(git log -1 --format=%s)" = "agent: add agent.txt"
check "agent.txt im Arbeitsbaum" grep -q "vom Agent" agent.txt

echo "== hearth logout"
"$H" logout > /dev/null
"$H" run pi -- --version > "$WORK/run2.log" 2>&1
check "Umleitung von pi entfernt" grep -q "entfernt" "$WORK/run2.log"

if [ "$failures" -gt 0 ]; then
  echo "$failures Prüfung(en) fehlgeschlagen. Protokolle:"
  for log in "$WORK"/*.log; do
    echo "--- $(basename "$log")"
    tail -15 "$log"
  done
  exit 1
fi
echo "Alles grün."
