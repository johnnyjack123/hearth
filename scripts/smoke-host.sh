#!/usr/bin/env bash
# Smoke test of the direct mode, without VM or containers: runs in Git Bash
# on Windows and on Linux or macOS.
#
#   throwaway server + scripted mock LLM -> hearth login -> hearth init and
#   `hearth project host on` -> background service -> a web chat linked to
#   the project in the direct mode -> the server's chat model reads the
#   README and writes a file through the device (free), runs a command
#   (asks: approved with `hearth host approve`), reads outside the project
#   (asks: denied with `hearth host deny`) -> the reply carries both
#   answers -> an answer from the chat is ignored while the project allows
#   approvals on the computer only -> the record -> hearth daemon stop.
#
# Needs `cargo build` and `pnpm install`. Uses ports 8097/8098 and a
# temporary HEARTH_HOME. SERVER_URL=<url> uses a server that already runs
# (with the mock on 127.0.0.1:8098 as its model), e.g. through an SSH tunnel
# from another machine; then SERVER_COOKIES and CREDENTIAL name the account.
# KEEP_LOGS=<folder> keeps the logs.
set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
H="${HEARTH_BIN:-$REPO/target/debug/hearth}"
SERVER_PORT=8097
MOCK_PORT=8098
URL="${SERVER_URL:-http://127.0.0.1:$SERVER_PORT}"
WORK="$(mktemp -d)"
NAME="host-$$"
export HEARTH_HOME="$WORK/home"
# The checks below look for German output.
export HEARTH_LANG=de
export HEARTH_TRAY_QUIET=1
J='Content-Type: application/json'
O="Origin: $URL"
PIDS=""

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
pending_id() { # the id of the first waiting question
  "$H" host 2> /dev/null | sed -n 's/^\[\([A-Z0-9]\{6\}\)\].*/\1/p' | head -1
}
pending_is() { # pending_is <text in its summary>
  "$H" host 2> /dev/null | grep -q "$1"
}

cleanup() {
  [ -n "${KEEP_LOGS:-}" ] && mkdir -p "$KEEP_LOGS" && cp "$WORK"/*.log "$HEARTH_HOME"/daemon.log "$KEEP_LOGS"/ 2> /dev/null
  "$H" daemon stop > /dev/null 2>&1
  "$H" logout > /dev/null 2>&1
  for pid in $PIDS; do kill "$pid" 2> /dev/null; done
  if command -v powershell.exe > /dev/null 2>&1; then
    powershell.exe -NoProfile -Command "foreach (\$p in $SERVER_PORT,$MOCK_PORT) { Get-NetTCPConnection -LocalPort \$p -State Listen -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id \$_.OwningProcess -Force } }" > /dev/null 2>&1
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

if [ -z "${SERVER_URL:-}" ]; then
  echo "== Server und Mock-LLM"
  MOCK_PORT=$MOCK_PORT MOCK_LOG="$WORK/mock-requests.log" node "$REPO/scripts/mock-agent-llm.mjs" > "$WORK/mock.log" 2>&1 &
  PIDS="$PIDS $!"
  (cd "$REPO/apps/server" && DATA_DIR="$WORK/server-data" PORT=$SERVER_PORT PUBLIC_URL="$URL" HEARTH_SETUP_CODE=SMOKE-HOST \
    exec node_modules/.bin/tsx src/index.ts > "$WORK/server.log" 2>&1) &
  PIDS="$PIDS $!"
  wait_for 30 curl -s -o /dev/null "$URL/api/health"
  curl -s -c "$WORK/cookies" -H "$J" -H "$O" -H "x-hearth-setup-code: SMOKE-HOST" \
    -d '{"email":"host@example.com","name":"Host","password":"correct-horse-battery"}' \
    "$URL/api/auth/sign-up/email" > /dev/null
  CREDENTIAL=$(api POST /api/providers \
    "{\"kind\":\"openai-compatible\",\"label\":\"Mock\",\"baseUrl\":\"http://127.0.0.1:$MOCK_PORT/v1\",\"apiKey\":\"sk-mock-key\"}" |
    json v.id)
else
  cp "$SERVER_COOKIES" "$WORK/cookies"
fi

echo "== hearth login"
"$H" login --no-browser "$URL" > "$WORK/login.log" 2>&1 &
LOGIN=$!
wait_for 30 grep -qE '[A-Z2-9]{4}-[A-Z2-9]{4}' "$WORK/login.log"
CODE=$(grep -oE '[A-Z2-9]{4}-[A-Z2-9]{4}' "$WORK/login.log" | head -1)
api POST "/api/devices/pairing/$CODE" '{"approve":true}' > /dev/null
check "Rechner gekoppelt" wait "$LOGIN"

echo "== Projekt im Direktmodus"
mkdir -p "$WORK/$NAME" && cd "$WORK/$NAME" || exit 1
git init -q -b main
git config user.name "Host User"
git config user.email host@example.com
git config core.autocrlf false
echo "# $NAME" > README.md && git add README.md && git commit -qm initial
echo "geheim" > "$WORK/outside.txt"
"$H" init > "$WORK/init.log" 2>&1
check "Projekt angelegt" grep -q "steht in der Web-App zur Auswahl" "$WORK/init.log"
"$H" project host on > "$WORK/host-on.log" 2>&1
check "Direktmodus an" grep -q "Direktmodus an" "$WORK/host-on.log"
check "Freigaben nur am Rechner" grep -q "Freigaben nur an diesem Rechner" "$WORK/host-on.log"

echo "== Hintergrunddienst"
"$H" daemon start > "$WORK/daemon-start.log" 2>&1
check "Dienst verbunden" wait_for 30 grep -q "Verbunden" "$HEARTH_HOME/daemon.log"
offered() { test "$(api GET /api/devices | json "v[0].online && v[0].projects.find((p) => p.name === '$NAME')?.harnesses.join(',')")" = host; }
check "Projekt mit Direktmodus angeboten" wait_for 15 offered
DEVICE=$(api GET /api/devices | json 'v[0].id')

echo "== Chat im Direktmodus"
CONVERSATION=$(api POST /api/conversations '{}' | json v.id)
LINK=$(api PUT "/api/conversations/$CONVERSATION/link" "{\"deviceId\":\"$DEVICE\",\"project\":\"$NAME\",\"harness\":\"host\"}")
check "Chat verknüpft" test "$(echo "$LINK" | json v.link.harness)" = host
printf '%s' "{\"message\":{\"id\":\"m1\",\"role\":\"user\",\"parts\":[{\"type\":\"text\",\"text\":\"Arbeite\"}]},\"credentialId\":\"$CREDENTIAL\",\"modelId\":\"mock-model\"}" > "$WORK/m1.json"
curl -sN -b "$WORK/cookies" -H "$J" -H "$O" --data-binary "@$WORK/m1.json" "$URL/api/conversations/$CONVERSATION/chat" > "$WORK/m1.log" &
CHAT=$!

check "Befehl wartet auf Freigabe" wait_for 30 pending_is "Befehl ausführen: cat notes/agent.txt"
check "Datei im Projekt ohne Rückfrage geschrieben" grep -q "hallo vom agent" "$WORK/$NAME/notes/agent.txt"
COMMAND_ID=$(pending_id)
# The project allows approvals on this computer only: the chat's answer is ignored.
api POST "/api/conversations/$CONVERSATION/host-approvals/$COMMAND_ID" '{"decision":"once"}' > /dev/null
sleep 1
check "Antwort aus dem Chat ignoriert" pending_is "Befehl ausführen"
check "Dienst sagt warum" grep -q "Projekt erlaubt Freigaben nur an diesem Rechner" "$HEARTH_HOME/daemon.log"
"$H" host approve "$COMMAND_ID" > "$WORK/approve.log" 2>&1
check "am Rechner freigegeben" grep -q "freigegeben" "$WORK/approve.log"

check "Lesen außerhalb wartet auf Freigabe" wait_for 30 pending_is "Datei lesen:"
"$H" host deny "$(pending_id)" > /dev/null 2>&1
check "Antwort fertig" wait "$CHAT"
check "Befehl lief mit Ausgabe" grep -q 'hallo vom agent' "$WORK/m1.log"
check "Modell erfährt die Ablehnung" grep -q 'The user denied this' "$WORK/m1.log"
check "Geheimnis von außerhalb nicht gelesen" bash -c "! grep -q geheim '$WORK/m1.log'"
check "Rückfrage-Karte im Chat" grep -q '"type":"data-hostApproval"' "$WORK/m1.log"
STORED=$(api GET "/api/conversations/$CONVERSATION")
check "Antwort gespeichert" test "$(echo "$STORED" | json "v.messages[1].parts.filter((p) => p.type.startsWith('tool-')).map((p) => p.type).join(',')")" = "tool-read,tool-write,tool-shell,tool-read"

echo "== Protokoll"
"$H" host log "$NAME" > "$WORK/record.log" 2>&1
check "Befehl einmal erlaubt am Rechner" grep -qE "shell +once +computer" "$WORK/record.log"
check "Lesen außerhalb abgelehnt" grep -qE "read +deny +computer" "$WORK/record.log"
check "Schreiben frei" grep -qE "write +free +rules" "$WORK/record.log"
check "keine offenen Freigaben" bash -c "\"$H\" host | grep -q 'Keine offenen Freigaben'"

echo "== Dienst stoppen"
"$H" daemon stop > /dev/null 2>&1
check "Dienst beendet" bash -c "\"$H\" daemon | grep -q 'läuft nicht'"

if [ "$failures" -eq 0 ]; then
  echo "Alles grün."
else
  echo "$failures Prüfung(en) fehlgeschlagen. KEEP_LOGS=<ordner> behält die Protokolle."
fi
exit "$failures"
