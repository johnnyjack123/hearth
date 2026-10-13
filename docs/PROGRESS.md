# Progress

_Last updated: 2026-10-14 · Phase 2 Pakete 1–6 fertig, Paket 7 zur Hälfte (LLM-Zugang über den Server, uncommittet), weiter mit pi im Container_

## Where we are

Das Repo liegt auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). `main` enthält Phase 1 und Phase 2 bis Paket 6 (`8df92f8`), gearbeitet wird auf `feat/phase-2-sandbox`. **Der LLM-Zugang über den Server ist fertig und getestet, aber noch nicht committet.**

Seit Paket 6 neu (Entscheidung des Nutzers: Keys nur im Server, auch für Agent-Läufe aus dem Terminal):
- **Server:**
  - Kopplung von Rechnern (`/api/pairing`, `/api/devices`, `/api/device`, Tabellen `device` und `devicePairing`, Migration `0002_devices`)
  - LLM-Durchgang `/api/llm/proxy/<Anbieter-ID>/…` und `/api/llm/providers` (`routes/llm.ts`)
  - CSRF-Ausnahme nur für die Token-Routen
- **Web-App:** Bestätigungsseite `/device` (mit Rücksprung nach dem Login), Reiter „Geräte“ in den Einstellungen.
- **Gateway:** virtuelle Adresse `http://llm.harness/…` zum Server, mit dem Geräte-Token, das es per stdin vom Host bekommt (`llm.rs`).
- **CLI:**
  - `harness login [URL] [--no-browser]` und `logout`; das Token liegt in der Windows-Anmeldeinformationsverwaltung
  - `doctor` prüft die Kopplung
  - `net start` gibt den LLM-Zugang ans Gateway, `net status` zeigt ihn

## Next step

Rest von Paket 7 (pi im Container), in dieser Reihenfolge:

1. **Tools-Volume** `harness-tools`, für Instanzen read-only unter `/opt/harness-tools`.
   - Die Installation läuft in einem kurzlebigen, gehärteten Container mit Schreibzugriff, im Projektnetz über dessen Gateway.
   - Befehl ist `install` aus `harnesses/pi.toml` (`npm install -g @earendil-works/pi-coding-agent`) mit `NPM_CONFIG_PREFIX=/opt/harness-tools`.
2. **Volume** `harness-cfg-pi` für `~/.pi` (`HarnessConfig::persist_paths`, `config_volume`).
3. **`~/.pi/agent/models.json`** aus `ServerClient::credentials` erzeugen:
   - Eingebaute pi-Anbieter (`deepseek` für openai-compatible mit Host `api.deepseek.com`, sonst `openai`, `anthropic`, `mistral`, `openrouter`) bekommen nur `baseUrl: http://llm.harness/<id>` und `apiKey: "harness"`. So behält pi seine Modelldaten.
   - Für `google` vorher prüfen, ob pis Basis-URL `/v1beta` enthält. Quelle: `@earendil-works/pi-ai/dist/providers/data/*.json`, dort stehen auch DeepSeek `https://api.deepseek.com`, OpenAI `…/v1`, Anthropic ohne `/v1`.
   - Ollama und andere eigene Endpunkte werden eigene Anbieter (`api: "openai-completions"`) mit der Modellliste vom Server.
4. Prüfen, welche Hosts pi beim Start sonst braucht (`harness net status`).

Danach Paket 8:
- `run`, `shell`, `status`, `stop`, `sync`, `merge`.
- `run`/`shell` rufen `gateway::ensure_running(…, login::gateway_llm(…))` vor dem Instanzstart, `stop` stoppt Instanz und Gateway.
- Rückfragen im selben Terminal brauchen eine ConPTY zwischen Terminal und `podman exec`. Bis dahin hilft `harness net watch` im zweiten Fenster.

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alle Zeilen ✓, solange die VM läuft).

## In flight / half-finished

- `PodmanProvider::exec` mit `tty: true` ist gebaut, aber **nicht interaktiv getestet**. Das kommt mit `harness shell` (Paket 8) und muss der Nutzer in Windows Terminal ausprobieren. Ctrl+C und Fenstergröße sind ungeprüft.
- Das echte `~/.harness` des Nutzers ist noch nicht angelegt. Probeläufe liefen mit `HARNESS_HOME` im Scratchpad.

## Decisions made this session

LLM-Zugang (14.10., nach Rückfrage beim Nutzer):
- **Der Server ist die einzige Stelle für Keys**, auch für pi im Terminal. Agent-Läufe aus Terminal und Web-Chat nutzen denselben Weg, denselben Key und dieselben Modelle.
- **Kein echter Key im Container:**
  - pi bekommt `llm.harness` und einen Dummy-Key.
  - Das Gateway setzt das Geräte-Token ein.
  - Der Server setzt den Anbieter-Key ein.
- **Eigene Geräte-Tokens statt Better-Auth-Device-Plugin**, weil dessen Token eine volle Sitzung wäre. Gespeichert wird nur der Hash. Den Token gibt es genau einmal, Widerruf in der Web-App oder mit `logout`.
- **Durchgang 1:1 im Protokoll des Anbieters** statt Übersetzung auf ein Format. So funktionieren Streaming, Reasoning und Caching der Harness unverändert. Erlaubt sind nur die Chat-Endpunkte.
- **Rückfragen nie in der Oberfläche der Harness**, sondern auf dem Host: `net watch`, später `harness run`, die App, der Web-Chat. Eigene Rückfragen der Harness betreffen nur das Innere der Sandbox und bleiben.
- Host-Ausführung: Der Nutzer hat noch keinen konkreten Bedarf. Sie soll sich später ergänzen lassen. Der Mechanismus (Anfrage halten, außerhalb entscheiden) ist mit den Netz-Rückfragen vorhanden.

Paket 6 (Gateway):
- **Eigener Rust-Proxy statt tinyproxy**, aus Phase 3 vorgezogen. Der Nutzer wollte „alles erlauben“ und Rückfragen wie bei Claude Code. tinyproxy kann weder Ziel-IPs prüfen (dann wäre „open“ ein Weg zu Host und LAN) noch Anfragen festhalten.
- **Nur Standardbibliothek, ein Thread pro Verbindung** (statt tokio/hyper wie in ARCHITECTURE geplant). Damit baut das Image offline in der VM aus eingebetteten Quellen, ohne crates.io, und braucht ~2 MB RAM. Gegen Cross-Compiling auf dem Host entschieden (Nutzer fragte danach): Die CLI bliebe sonst nicht eigenständig, und jede VM-Architektur bräuchte ein eigenes Target. Kosten: einmalig `rust:1.97-slim-trixie` in der VM (~860 MB auf der Platte).
- **IP-Prüfung in jedem Modus**, nicht nur bei „open“. Das Gateway verbindet sich mit genau der geprüften Adresse, deshalb hilft DNS-Rebinding nicht.
- **`off` = nur LLM-APIs.** Mit gar keinem Netz wäre jede Harness nutzlos. `llm_apis` steht global in `config.toml` (Standard DeepSeek), weil der Anbieter am Nutzer hängt, nicht am Projekt.
- **Rückfragen nur, wenn jemand zuschaut.** Ohne `harness net watch` kommt sofort 403 mit Hinweis, statt den Agent 2 Minuten hängen zu lassen.
- **Laufzeit-Freigaben leben im Gateway** und verfallen beim Stopp. Nur „immer“ bzw. `--always` schreibt in die Projektdatei. „nein“ sperrt bis zum Stopp, damit ein wiederholender Agent den Nutzer nicht mit Fragen flutet.
- **Steuerung per `podman exec … ctl` zu einem Unix-Socket** statt über einen Port. Die VM braucht keinen offenen Port, und Instanzen kommen nicht an die Steuerung.
- Gateway läuft als `nobody` (kein `keep-id`, keine Mounts), 128 MB, 512 PIDs (Threads zählen mit).

## Open questions for the user

- Commit des LLM-Zugangs (Server, Web, Gateway, CLI) und Merge nach `main`: Nutzer fragen.

## Known issues / debt

- **Hoch:** VM-Start nur per Admin-PowerShell (→ Phase 2b). Nach einem gescheiterten Start von Hand aufräumen, siehe `spikes/phase-0.md` §12.2. Die vsock-Registry-Einträge **nicht** löschen.
- **Mittel:** ~350–500 ms pro podman-Aufruf (neue SSH-Verbindung). `gateway::ensure_running` braucht ~4,3 s beim Anlegen und ~2,4 s, wenn das Gateway schon läuft (etwa 8 Aufrufe: Image-Status, zwei Netze, Container, Regeln). Sync hin ~0,6 s, zurück ~0,7 s. Abhilfe: libpod-REST über einen gehaltenen SSH-Tunnel, oder Prüfungen zusammenfassen.
- **Mittel:** Das Gateway spricht zum Server nur HTTP. Ein entfernter Server mit HTTPS braucht TLS im Gateway, also rustls und damit die ersten Abhängigkeiten. Der Bau in der VM bräuchte dann crates.io.
- **Mittel:** Der LLM-Durchgang erfasst noch keinen Verbrauch pro Gerät. Ein Limit für Anfragen pro Gerät gibt es auch nicht.
- **Mittel:** Rückfragen hält das Gateway bis zu 120 s fest. pip bricht nach 15 s ab (`--timeout`), npm und Node-`fetch` warten länger. Bei pip ggf. erst freigeben, dann wiederholen.
- **Mittel:** `in.bundle` enthält jedes Mal die ganze Historie. Bei großen Repos inkrementell machen (`--not` mit dem Stand des letzten Exports).
- **Mittel:** CLI-Texte nur Deutsch, Englisch fehlt (Web-UI ist zweisprachig). Die Fehlerseite des Gateways ist zweisprachig.
- **Mittel:** kein Dockerfile/compose für den Server, keine ESLint/Prettier-Konfiguration, kein README (ROADMAP „Phase 1 – Reste“). Web-Haupt-Chunk ~955 kB.
- **Niedrig:** Image-Builds hinterlassen `<none>`-Images in der VM (Build-Stufe ~860 MB, alte Versionen). Platz ist genug (36 GB frei); später `harness image prune`.
- **Niedrig:** `harness net watch` druckt Protokollzeilen während einer offenen Frage erst nach der Antwort. Strg+C beendet ohne `bye`, das Gateway merkt es dann erst nach 45 s.
- **Niedrig:** Host-Key der VM per Trust on first use. Bei neuem Host-Key mit gleichem Port `%LOCALAPPDATA%\llm-harness\podman\harness-<port>\` löschen.
- **Niedrig:** Podman-Hilfsprozesse laufen im Kontext des Admin-Fensters. Schließt der Nutzer es, ist das Verhalten ungeprüft.
- Rechner-Stabilität: Bluescreen `0xA0` beim Start der Windows Sandbox, Abbruch der WSL-VM bei Standby. Nutzer vor riskanten Virtualisierungs-Starts fragen.

## Verification state

Stand 14.10., nach dem LLM-Zugang:
- **Rust:**
  - `cargo build`, `cargo fmt --check` und `cargo clippy --all-targets` laufen ohne Warnungen.
  - `cargo test`: 104 Tests grün.
    - `harness-core`: 72, neu Server-URL in `config.toml`, Upstream-Adresse für die VM, Antworten der Kopplung.
    - `harness-gateway`: 25 Unit-Tests, neu `llm.rs` (Upstream-URL, Pfade bleiben unter dem Präfix, Token nicht im `Debug`).
    - `harness-gateway`: 7 TCP-Tests. Neu: `llm.harness` gibt dem Server nur das Geräte-Token, `..` ergibt 400, `CONNECT llm.harness` 400, die Server-Adresse direkt 403, das Token fehlt im Status.
- **Server:** 64/64 grün mit SQLite **und** Postgres (`TEST_POSTGRES_URL`). Neu sind 9 Tests je Datenbank:
  - Kopplung mit Token genau einmal; Ablehnung, Ablauf und unbekannte Codes
  - Geräte-Token kommt nur an Geräte- und LLM-Routen
  - Widerruf über die Web-App und über das Gerät selbst
  - CSRF bleibt für Cookie-Routen
  - Durchgang OpenAI-artig und Anthropic, mit Key und Pfad
  - fremde Anbieter und fremde Pfade ergeben 404
  - Anbieterliste ohne Keys
- **Web:**
  - `pnpm -r typecheck` grün.
  - E2E 9/9 ohne Konsolenfehler: Kopplung im Browser bestätigt, danach Antwort des Mock-LLM über den Durchgang, Geräteliste, Rücksprung nach Login auf `/device`.
  - Screenshots 12–15 angesehen.
- **VM:** `gateway_vm` grün, mit `curl http://llm.harness/…` aus der Instanz zu einem Test-Server auf Windows (`127.0.0.1`, über `192.168.127.254`). Der Server sah das Geräte-Token, nicht den Key des Agents.
- **Probelauf** (`login-demo.sh` im Scratchpad):
  - echter Server mit Wegwerf-DB auf :8093, Mock-LLM auf :8094
  - `harness login --no-browser`, Bestätigung per API
  - `doctor` ✓, `net start` meldet „LLM: über http://127.0.0.1:8093“
  - Container im Projektnetz: `POST http://llm.harness/<id>/chat/completions` → Antwort des Mock-LLM über Gateway und Server
  - `logout` widerruft; danach kein Eintrag mehr in der Anmeldeinformationsverwaltung

Stand 13.10. (Paket 6), weiterhin gültig:
- VM-Tests nacheinander (`cargo test -p harness-core -- --ignored --test-threads=1`): alle 4 grün (`podman_vm`, `base_image_vm`, `sync_vm`, neu `gateway_vm`). In der VM bleibt danach nur das gemeinsame Netz `harness-egress`.
- `gateway_vm`, Ergebnisse:
  - development: `https://example.com` → 403, `api.deepseek.com` → 401 (erreichbar), `npm view react version` → 19.3.0, `pnpm --version` → 12.11.2 (corepack lädt von `registry.npmjs.org`), ohne Proxy kein DNS
  - open: example.com → 200. Gesperrt (403) blieben `host.containers.internal:8080`, `192.168.127.254`, `192.168.127.1`, `10.88.0.1`, `169.254.169.254`, `gateway:8888`, `localhost`, `[::1]`. Die deny-Liste gilt auch hier.
  - Rückfrage über `watch`: Frage nach 358 ms, „einmal“ → 200. Ohne Zuschauer sofort 403, nach `allow` → 200.
- Probelauf der CLI mit `HARNESS_HOME` im Scratchpad:
  - `project add`, `net`, `net start` (4,3 s), `preset open`/`development`, `allow --minutes`, `allow --always` (Projektdatei behält Kommentare), `deny`, `status`, `stop`, `doctor` (6/6 ✓, meldet ein veraltetes Gateway-Image richtig)
  - `net watch` per Skript: Ein Container im Projektnetz ruft `example.net` und `example.edu` auf. Antwort „e“ → 200, „n“ → 403, danach ohne Zuschauer weiter 403.

---

## Log

### 2026-10-14 (Phase 2, Paket 7: LLM-Zugang)
- Nutzer entscheidet: Keys nur im Server. Rückfragen gehören in App, Web und `harness run`, nie in die Harness selbst.
- Server: `services/devices.ts`, `routes/devices.ts`, `routes/llm.ts`, Migration `0002_devices`, `requireDevice`, CSRF-Ausnahme für Token-Routen, `HTTPException` jetzt richtig beantwortet (vorher 500).
- Web: `features/devices/pairing-page.tsx` (`/device`), `features/settings/devices-panel.tsx`, Rücksprung nach Login (`lib/redirect.ts`), Texte de/en.
- Gateway: `llm.rs`, Anfrage `llm`, Gründe `llm`/`no-llm`. Host: `server.rs` (ureq, keyring), `GatewayControl::send_private`, `Provider::host_address`. CLI: `login`, `logout`, `doctor` und `net` mit LLM.
- Postgres-Lauf wiederholt (64/64), Portweiterleitung unter Hyper-V geht.

### 2026-10-13 (Phase 2, Paket 6)
- `crates/harness-gateway`: `pattern.rs`, `address.rs`, `policy.rs`, `control.rs`, `http.rs`, `gateway.rs`, `proxy.rs`, `main.rs` (`serve`, `ctl`). Image `images/gateway` (Bau in der VM mit `rust:1.97-slim-trixie`).
- `harness-core`: `gateway.rs`, `Provider::create_gateway` und `exec_piped`, `GATEWAY_IMAGE`, `llm_apis` und `ask` in der Konfiguration, `DomainPattern` kommt jetzt aus dem Gateway-Crate.
- CLI: `harness net …`, `image build [base|gateway]`, `doctor` prüft das Gateway-Image.
- Host-Ausführung mit Einzelbestätigung als Idee in ROADMAP Phase 5 aufgenommen.

### 2026-10-12 (Phase 2, Pakete 1–5)
- Cargo-Workspace, Provider, Base-Image, Konfiguration (`~/.harness`), Git-Sync über Bundles. Nutzer wählt DeepSeek für pi.
- Remote `origin` eingerichtet, `feat/phase-1-chat` und `feat/phase-2-sandbox` (bis Paket 5) nach `main` gemergt und gepusht.

### 2026-10-12
- Windows-Sandbox-Spike verworfen, zurück zu Hyper-V + Linux. Machine `harness` angelegt, Phase 0 abgeschlossen.

### 2026-10-11
- Chat-Bug (useChat + StrictMode → `chat-store.ts`) und Login-Schleife behoben, E2E-Test und `CLAUDE.md` angelegt.

### 2026-10-10
- Architektur geplant, Server und Web-App (Phase 1) gebaut, WSL-Spike durchgeführt.
