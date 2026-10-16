# Progress

_Last updated: 2026-10-16 · Phase 2 im Kern fertig: Rückfragen im selben Terminal und Hinweise an den Agent gebaut, alle Tests grün; nächstes ist harness-vmd_

## Where we are

Das Repo liegt auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). `main` und `feat/phase-2-sandbox` stehen auf demselben Stand.

Im Ordner eines Git-Repos läuft nach `harness project add` und `harness login` jetzt der ganze Ablauf von Phase 2:
1. **`harness run pi`:**
   - startet das Gateway und die Instanz
   - klont das Repo ins Volume bzw. holt den neuen Stand
   - installiert pi beim ersten Mal ins Tools-Volume
   - schreibt pis `models.json` auf `llm.harness`
   - startet pi mit Hinweisen zur Sandbox im System-Prompt (`briefing.rs`)
2. **pi** erreicht das Modell über Gateway und Server, mit dem Key aus der Web-App. Will der Agent eine nicht freigegebene Domain erreichen, fragt harness im selben Terminal nach (`harness-cli/src/terminal/`).
3. **`harness sync` / `harness merge`** holen die Commits des Agents als `agent/<projekt>` und führen sie zusammen.

`scripts/smoke-run.sh` prüft das gegen die echte VM, mit einem Wegwerf-Server und einem Mock-LLM.

Bausteine:
- **Server:** Kopplung (`/api/pairing`, `/api/devices`, `/api/device`) und LLM-Durchgang (`/api/llm/proxy/<id>/…`)
- **Web-App:** Seite `/device`, Reiter „Geräte“
- **Gateway:** `llm.harness` mit dem Geräte-Token, auch per `CONNECT` (undici)
- **harness-core:** `server.rs`, `instance.rs`, `pi.rs`
- **CLI:**
  - Kopplung: `login`, `logout`
  - Agent: `run`, `shell`, `sync`, `merge`, `stop`, `status`
  - dazu wie bisher `net …`, `project …`, `image build`, `doctor`

## Next step

1. **Rückmeldung des Nutzers zum Terminal-Relay** in Windows Terminal: Maus, Einfügen, Farben, Rückfrage über pi. Bei Problemen `HARNESS_TERMINAL_LOG=<datei>` mitschneiden lassen. Notlösung ist `HARNESS_DIRECT_TERMINAL=1`.
2. **Phase 2b `harness-vmd`** (VM ohne Admin-Fenster) und `harness` ohne Argumente, das das Projekt nach Rückfrage selbst anlegt. Das ist der Wunsch des Nutzers vom 15.10., siehe ROADMAP.

Weitere Hinweise:
- pis Basis-URLs stehen in `@earendil-works/pi-ai/dist/providers/data/*.json`: DeepSeek `https://api.deepseek.com`, OpenAI `…/v1`, Anthropic ohne `/v1`, Google `…/v1beta`.
- Mistral nutzt dort `mistral-conversations`, das der Server-Durchgang nicht erlaubt. Mistral fehlt deshalb noch in `models.json`.

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alle Zeilen ✓, solange die VM läuft), dann `bash scripts/smoke-run.sh`.

## In flight / half-finished

- Das Terminal-Relay ist nur im simulierten Terminal geprüft (Größe, Strg+C, Rückfragen, pi im Vollbild). In Windows Terminal hat es der Nutzer noch nicht benutzt.
- Das echte `~/.harness` des Nutzers gibt es seit dem 15.10., mit dem Projekt `harness-test` in `$HOME\harness-test`. Eigene Probeläufe weiter nur mit `HARNESS_HOME`.

## Decisions made this session

LLM-Zugang und pi (14.10.):
- **Der Server ist die einzige Stelle für Keys** (Entscheidung des Nutzers), auch für pi im Terminal. Kein echter Key im Container: pi bekommt `llm.harness` und einen Dummy-Key. Das Gateway setzt das Geräte-Token ein, der Server den Anbieter-Key.
- **Eigene Geräte-Tokens statt Better-Auth-Device-Plugin**, weil dessen Token eine volle Sitzung wäre. Gespeichert wird nur der Hash.
- **Durchgang 1:1 im Protokoll des Anbieters**, damit Streaming, Reasoning und Caching der Harness unverändert funktionieren. Erlaubt sind nur die Chat-Endpunkte.
- **Rückfragen nie in der Oberfläche der Harness**, sondern auf dem Host (`net watch`, später `harness run`, App, Web-Chat).
- **pi liegt in einem gemeinsamen Tools-Volume, das Instanzen nur lesen.** So ändert ein Agent nicht die pi-Version anderer Projekte. Installiert wird in einem kurzlebigen Installer-Container.
- **Eingebaute Harness-Definitionen werden nicht mehr nach `~/.harness` kopiert.** Kopien veralten still, so fehlte `llm = "pi"`.
- **harness verwaltet in `models.json` nur seine eigenen Einträge.** Einträge des Nutzers haben Vorrang. Nach `logout` entfernt harness die Umleitung, damit pis eigene Anmeldung wieder greift.
- Commits: Der Nutzer erlaubt, nach jedem fertigen Stück zu committen, zu mergen und zu pushen.

Terminal-Relay und Hinweise (15./16.10.):
- **Rückfragen im selben Terminal über eine eigene Pseudo-Konsole** statt eines zweiten Fensters, weil der Nutzer dem Vorschlag zugestimmt hat. Die Entscheidung trifft weiter harness auf dem Host, nie die Harness im Container. Fällt die Pseudo-Konsole aus, geht es mit direktem Terminal und `net watch` weiter.
- **Hinweise an den Agent über den System-Prompt** (Wunsch des Nutzers): Arbeitskopie, Commits, Netzregeln. Der Text ist englisch, weil ihn das Modell liest.
- **pi ohne Versionsprüfung und Telemetrie**, sonst käme bei jedem Start eine Rückfrage zu `pi.dev`. Updates von pi macht harness.

Paket 6 (Gateway, 13.10.): eigener Rust-Proxy nur mit Standardbibliothek statt tinyproxy (Ziel-IP-Prüfung, Rückfragen), Bau offline in der VM, IP-Prüfung in jedem Modus, `off` = nur LLM-APIs, Rückfragen nur bei Zuschauer, Laufzeit-Freigaben im Gateway, Steuerung per Unix-Socket. Details in ROADMAP Paket 6.

## Open questions for the user

- Wie sich die Rückfragen im echten Windows Terminal anfühlen (siehe Next step 1).

## Known issues / debt

- **Hoch:** VM-Start nur per Admin-PowerShell (→ Phase 2b). Nach einem gescheiterten Start von Hand aufräumen, siehe `spikes/phase-0.md` §12.2. Die vsock-Registry-Einträge **nicht** löschen.
- **Mittel:** Die VM bekommt nur so viel RAM, wie Windows frei hat (Start 1 GB). Am 14.10. nahm sie nach den pi-Läufen keine SSH-Verbindungen mehr an, nur ein Neustart half. Ursache nicht sicher geklärt, vermutlich Speicher.
- **Mittel:** ~350–500 ms pro podman-Aufruf (neue SSH-Verbindung). `harness run` braucht bis zum Start von pi ~5–10 s. Abhilfe: libpod-REST über einen gehaltenen SSH-Tunnel, oder Prüfungen zusammenfassen.
- **Mittel:** Das Gateway spricht zum Server nur HTTP. Ein entfernter Server mit HTTPS braucht TLS im Gateway, also rustls und damit die ersten Abhängigkeiten. Der Bau in der VM bräuchte dann crates.io.
- **Mittel:** Der LLM-Durchgang erfasst noch keinen Verbrauch pro Gerät. Ein Limit für Anfragen pro Gerät gibt es auch nicht.
- **Mittel:** Rückfragen hält das Gateway bis zu 120 s fest. pip bricht nach 15 s ab (`--timeout`).
- **Mittel:** `in.bundle` enthält jedes Mal die ganze Historie. Bei großen Repos inkrementell machen.
- **Mittel:** CLI-Texte nur Deutsch, Englisch fehlt (Web-UI ist zweisprachig). Die Fehlerseite des Gateways ist zweisprachig.
- **Mittel:** kein Dockerfile/compose für den Server, keine ESLint/Prettier-Konfiguration, kein README (ROADMAP „Phase 1 – Reste“). Web-Haupt-Chunk ~955 kB.
- **Niedrig:** Image-Builds hinterlassen `<none>`-Images in der VM. Später `harness image prune`.
- **Niedrig:** `harness net watch` druckt Protokollzeilen während einer offenen Frage erst nach der Antwort.
- **Niedrig:** Mit `HARNESS_HOME` aus Git Bash (`C:/…`) zeigt `project show` gemischte Schrägstriche. Nur kosmetisch.
- **Niedrig:** Host-Key der VM per Trust on first use. Bei neuem Host-Key mit gleichem Port `%LOCALAPPDATA%\llm-harness\podman\harness-<port>\` löschen.
- Rechner-Stabilität: Bluescreen `0xA0` beim Start der Windows Sandbox, Abbruch der WSL-VM bei Standby. Nutzer vor riskanten Virtualisierungs-Starts fragen.

## Verification state

Stand 16.10.:
- **Rust:**
  - `cargo build`, `cargo fmt` und `cargo clippy --all-targets` laufen ohne Warnungen.
  - `cargo test`: 117 Tests grün (core 81, cli 3, gateway 26 + 7 TCP).
- **Terminal** (`cargo test -p harness-cli --test terminal_vm -- --ignored`): beide Tests grün, harness läuft dabei in einer Pseudo-Konsole des Tests.
  - `harness shell`: Größe 100×30 und nach Änderung 120×40 im Container, Strg+C bricht `sleep 30` ab.
  - Rückfrage zu `example.net`: „e“ ergibt HTTP 200, Esc lehnt ab (403).
  - Rückfrage, während das Programm im alternativen Bildschirm ist.
  - `harness run pi`: Vollbild-Oberfläche, keine Rückfrage beim Start, `!curl` in pi löst eine Rückfrage über pis Oberfläche aus, nach „e“ zeigt pi `PI-200`, Strg+D beendet.
- **Hinweise an den Agent:** Ein Mock-LLM, das mitschreibt, sah sie in pis System-Prompt.
- **VM-Tests** (`cargo test -p harness-core -- --ignored --test-threads=1`): alle 5 grün, Smoke-Skript grün.

Stand 14.10. abends:
- **VM-Tests** (`cargo test -p harness-core -- --ignored --test-threads=1`): alle 5 grün.
  - `gateway_vm`, neu mit dem CONNECT-Tunnel zu `llm.harness` (`curl --proxytunnel`, wie undici). Der Server sah nur das Geräte-Token.
  - neu `instance_vm`:
    - Anlegen, Wiederverwenden, Neuanlage bei neuen Mounts, Daten bleiben erhalten
    - Installation unter `off` nur über die Freigabe, Tools-Volume aus der Instanz nicht beschreibbar
    - `models.json`: verbinden, eigener Eintrag bleibt, Abmelden, kaputte Datei → `.bak`
- **`scripts/smoke-run.sh`** grün in 37 s:
  - Kopplung, `pi --print` antwortet über `llm.harness`
  - in `harness shell`: `example.com` gesperrt, `npm view react version` geht
  - Commit im Container, `sync`, `merge` per Fast-Forward
  - nach `logout` ist pis Umleitung entfernt
  - in der VM bleibt nichts übrig
- **Von Hand:** Nach `logout` bekommt ein noch laufendes Gateway vom Server 401 (`login-demo.sh` im Scratchpad).
- **15.10.:**
  - Ablauf des Nutzers nachgestellt: Server auf `localhost:8080` mit `PUBLIC_URL=http://localhost:5173`, `harness login` ohne URL. Der Link zeigt auf Vite, `doctor` ist ✓, pi antwortet.
  - `sync` meldet uncommittete Dateien im Container (`sync_vm` erweitert, CLI-Lauf mit `?? hello.py`).
  - Der Nutzer hat pi mit DeepSeek interaktiv genutzt.
- **Server und Web unverändert seit `183a113`:** Server 64/64 (SQLite und Postgres), E2E 9/9, Typecheck grün.

---

## Log

### 2026-10-15/16 (Phase 2, Terminal-Relay)
- Erster interaktiver Lauf des Nutzers mit DeepSeek. `sync` meldet uncommittete Arbeit des Agents.
- `briefing.rs` (Hinweise im System-Prompt), `[env]` und `prompt_flag` in Harness-Definitionen.
- `harness-core/src/pty.rs` (ConPTY), `harness-cli/src/terminal/` (Relay, Rückfragen, Tasten, Bildschirmzustand), `Provider::exec_command`, `tests/terminal_vm.rs`.

### 2026-10-14 (Phase 2, Pakete 7–9)
- Server: Kopplung von Geräten, LLM-Durchgang mit gespeicherten Keys; Web: `/device`, Reiter „Geräte“.
- Gateway: `llm.harness` mit Geräte-Token, CONNECT-Tunnel auf Port 80.
- harness-core: `server.rs`, `instance.rs`, `pi.rs`; CLI: `login`, `logout`, `run`, `shell`, `sync`, `merge`, `stop`, `status`.
- `tests/instance_vm.rs`, `scripts/smoke-run.sh`. VM musste einmal neu gestartet werden (SSH-Banner blieb leer).

### 2026-10-13 (Phase 2, Paket 6)
- `crates/harness-gateway`, Image `images/gateway`, `harness-core/src/gateway.rs`, CLI `harness net …`, `doctor` prüft das Gateway-Image.

### 2026-10-12 (Phase 2, Pakete 1–5)
- Cargo-Workspace, Provider, Base-Image, Konfiguration (`~/.harness`), Git-Sync über Bundles. Nutzer wählt DeepSeek für pi. Remote `origin` eingerichtet.
- Windows-Sandbox-Spike verworfen, zurück zu Hyper-V + Linux. Machine `harness` angelegt, Phase 0 abgeschlossen.

### 2026-10-10 bis 2026-10-11
- Architektur geplant, Server und Web-App (Phase 1) gebaut, WSL-Spike. Chat-Bug (useChat + StrictMode) und Login-Schleife behoben, E2E-Test angelegt.
