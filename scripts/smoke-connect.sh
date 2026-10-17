#!/usr/bin/env bash
# Smoke test of Phase 4 against the real VM, from Git Bash:
#
#   throwaway server + tool-calling mock LLM -> harness login
#   -> harness project remote on -> harness connect (in the background)
#   -> a web chat linked to the project over the API -> pi in RPC mode runs
#   a command that commits and asks the gateway for example.com -> the
#   question is answered over the chat API -> the reply with tool call and
#   text is streamed and stored -> harness sync brings the commit home
#
# Needs the running VM, both images, `cargo build` and `pnpm install`. Uses
# ports 8095/8096 and a temporary HARNESS_HOME; pi's shared volume
# harness-cfg-pi gets this server's providers in models.json until the next
# `harness run` (the logout at the end removes them). Do not run it while
# pi is in use. KEEP_LOGS=<folder> keeps the logs.
set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
H="$REPO/target/debug/harness"
PM="$REPO/spikes/phase-0/pm.sh"
SERVER_PORT=8095
MOCK_PORT=8096
URL="http://127.0.0.1:$SERVER_PORT"
WORK="$(mktemp -d)"
NAME="connect-$$"
export HARNESS_HOME="$WORK/home"
J='Content-Type: application/json'
O="Origin: $URL"
CONNECT=""

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
api() { # api <method> <path> [json]
  if [ $# -ge 3 ]; then
    curl -s -b "$WORK/cookies" -H "$J" -H "$O" -X "$1" -d "$3" "$URL$2"
  else
    curl -s -b "$WORK/cookies" -H "$O" -X "$1" "$URL$2"
  fi
}
wait_for() { # wait_for <seconds> <command…>
  local seconds="$1"
  shift
  for _ in $(seq $((seconds * 2))); do
    "$@" && return 0
    sleep 0.5
  done
  return 1
}
json() { # json <expression> (reads stdin)
  node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const v=JSON.parse(s);console.log(eval(process.argv[1]))})" "$1"
}

cleanup() {
  [ -n "${KEEP_LOGS:-}" ] && mkdir -p "$KEEP_LOGS" && cp "$WORK"/*.log "$KEEP_LOGS"/ 2> /dev/null
  [ -n "$CONNECT" ] && kill "$CONNECT" 2> /dev/null
  "$H" logout > /dev/null 2>&1
  (cd "$WORK/$NAME" 2> /dev/null && "$H" stop > /dev/null 2>&1)
  "$PM" rm -f "harness-$NAME" "harness-gw-$NAME" "harness-install-$NAME" > /dev/null 2>&1
  "$PM" volume rm "harness-ws-$NAME" "harness-home-$NAME" > /dev/null 2>&1
  "$PM" network rm "harness-net-$NAME" > /dev/null 2>&1
  powershell.exe -NoProfile -Command "foreach (\$p in $SERVER_PORT,$MOCK_PORT) { Get-NetTCPConnection -LocalPort \$p -State Listen -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id \$_.OwningProcess -Force } }" > /dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "== Server und Mock-LLM"
COMMAND='echo hallo > agent.txt && git add agent.txt && git commit -qm "agent: add agent.txt" && curl -sS --max-time 90 -o /dev/null -w "status=%{http_code}" https://example.com/'
(MOCK_PORT=$MOCK_PORT MOCK_LOG="$WORK/mock-requests.log" MOCK_COMMAND="$COMMAND" \
  node "$REPO/scripts/mock-agent-llm.mjs" > "$WORK/mock.log" 2>&1) &
(cd "$REPO/apps/server" && DATA_DIR="$WORK/server-data" PORT=$SERVER_PORT PUBLIC_URL="$URL" \
  node_modules/.bin/tsx src/index.ts > "$WORK/server.log" 2>&1) &
wait_for 30 curl -s -o /dev/null "$URL/api/health"
curl -s -c "$WORK/cookies" -H "$J" -H "$O" \
  -d '{"email":"connect@example.com","name":"Connect","password":"correct-horse-battery"}' \
  "$URL/api/auth/sign-up/email" > /dev/null
CREDENTIAL=$(api POST /api/providers \
  "{\"kind\":\"openai-compatible\",\"label\":\"Mock\",\"baseUrl\":\"http://127.0.0.1:$MOCK_PORT/v1\",\"apiKey\":\"sk-mock-key\"}" |
  json v.id)

echo "== harness login"
"$H" login "$URL" --no-browser > "$WORK/login.log" 2>&1 &
LOGIN=$!
wait_for 30 grep -qE '[A-Z2-9]{4}-[A-Z2-9]{4}' "$WORK/login.log"
CODE=$(grep -oE '[A-Z2-9]{4}-[A-Z2-9]{4}' "$WORK/login.log" | head -1)
api POST "/api/devices/pairing/$CODE" '{"approve":true}' > /dev/null
check "Rechner gekoppelt" wait "$LOGIN"

echo "== Test-Repo $NAME, für Web-Chats freigegeben"
mkdir -p "$WORK/$NAME" && cd "$WORK/$NAME" || exit 1
git init -q -b main
git config user.name "Connect User"
git config user.email connect@example.com
git config core.autocrlf false
echo "# $NAME" > README.md && git add README.md && git commit -qm initial
"$H" project add > /dev/null
"$H" project remote on > "$WORK/remote.log" 2>&1
check "Projekt freigegeben" grep -q "freigegeben" "$WORK/remote.log"

echo "== harness connect"
# stdin is no terminal here, so nobody can confirm a new chat.
"$H" connect < /dev/null > "$WORK/connect.log" 2>&1 &
CONNECT=$!
check "verbunden" wait_for 30 grep -q "Verbunden" "$WORK/connect.log"
DEVICES=$(api GET /api/devices)
DEVICE=$(echo "$DEVICES" | json 'v[0].id')
check "Gerät online mit Projekt" test "$(echo "$DEVICES" | json "v[0].online && v[0].projects[0].name")" = "$NAME"

echo "== Chat verknüpfen, ohne Bestätigung am Rechner"
CONVERSATION=$(api POST /api/conversations '{}' | json v.id)
LINK=$(api PUT "/api/conversations/$CONVERSATION/link" "{\"deviceId\":\"$DEVICE\",\"project\":\"$NAME\",\"harness\":\"pi\"}")
check "Chat verknüpft" test "$(echo "$LINK" | json v.link.project)" = "$NAME"
api POST "/api/conversations/$CONVERSATION/chat" \
  "{\"message\":{\"id\":\"m0\",\"role\":\"user\",\"parts\":[{\"type\":\"text\",\"text\":\"Hallo\"}]},\"credentialId\":\"$CREDENTIAL\",\"modelId\":\"mock-model\"}" \
  > "$WORK/refused.log"
check "neuer Chat ohne Bestätigung abgelehnt" grep -q "nicht bestätigt" "$WORK/refused.log"
check "pi nicht gestartet" bash -c "! grep -q 'läuft im Projekt' '$WORK/connect.log'"
"$H" project remote on --no-confirm > /dev/null

echo "== Chat schreiben"
# From a file: arguments of Windows programs lose their umlauts in Git Bash.
# -N, or curl holds the stream back until its buffer is full.
printf '%s' "{\"message\":{\"id\":\"m1\",\"role\":\"user\",\"parts\":[{\"type\":\"text\",\"text\":\"Leg agent.txt an und prüfe example.com\"}]},\"credentialId\":\"$CREDENTIAL\",\"modelId\":\"mock-model\"}" \
  > "$WORK/chat.json"
curl -sN -b "$WORK/cookies" -H "$J" -H "$O" --data-binary "@$WORK/chat.json" \
  "$URL/api/conversations/$CONVERSATION/chat" > "$WORK/stream.log" &
CHAT=$!
check "Rückfrage des Gateways im Chat" wait_for 240 grep -q '"type":"data-approval"' "$WORK/stream.log"
APPROVAL=$(grep -oE '"id":"approval-[0-9]+"' "$WORK/stream.log" | head -1 | grep -oE '[0-9]+')
api POST "/api/conversations/$CONVERSATION/approvals/$APPROVAL" '{"decision":"once"}' > /dev/null
wait_for 120 bash -c "! kill -0 $CHAT 2> /dev/null"
check "Umlaute kommen beim Modell an" grep -q 'prüfe example.com' "$WORK/mock-requests.log"
check "Frage als beantwortet markiert" grep -q '"state":"resolved"' "$WORK/stream.log"
check "Werkzeugaufruf gestreamt" grep -q '"type":"tool-input-available"' "$WORK/stream.log"
check "example.com nach Freigabe erreicht" grep -q 'status=200' "$WORK/stream.log"
check "Antwort des Modells gestreamt" grep -q '"delta":"fertig"' "$WORK/stream.log"
REPLY=$(api GET "/api/conversations/$CONVERSATION")
check "Antwort mit Werkzeugaufruf gespeichert" test \
  "$(echo "$REPLY" | json "v.messages.at(-1).parts.some(p => p.type === 'dynamic-tool' && p.output.text.includes('status=200'))")" = true
check "Agent in den Metadaten" test "$(echo "$REPLY" | json "v.messages.at(-1).metadata.agent.harness")" = pi

echo "== harness sync"
"$H" sync > "$WORK/sync.log" 2>&1
check "Commit des Agents auf agent/$NAME" test "$(git log -1 --format=%s "agent/$NAME" 2> /dev/null)" = "agent: add agent.txt"

echo "== Verknüpfung lösen"
api DELETE "/api/conversations/$CONVERSATION/link" > /dev/null
check "Sitzung auf dem Rechner beendet" wait_for 30 grep -q "Sitzung beendet" "$WORK/connect.log"

if [ "$failures" -gt 0 ]; then
  echo "$failures Prüfung(en) fehlgeschlagen. Protokolle:"
  for log in "$WORK"/*.log; do
    echo "--- $(basename "$log")"
    tail -25 "$log"
  done
  exit 1
fi
echo "Alles grün."
