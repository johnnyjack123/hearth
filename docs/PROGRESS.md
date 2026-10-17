# Progress

_Last updated: 2026-10-17 · Phase 4 gebaut (Web-Chat steuert pi auf dem Rechner), TLS im Gateway; nächstes: Test des Nutzers mit DeepSeek, dann Jev und Stopp der VM im Leerlauf_

## Where we are

Das Repo liegt auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). Gearbeitet wird auf `feat/phase-4-chat-env`, nach jedem fertigen Stück in `main` gemergt.

**Terminal (Phase 2):** Im Ordner eines Git-Repos startet `harness` (oder `harness run pi`) Gateway und Instanz, klont das Repo, installiert pi und verbindet es über den Server mit dem Modell. Rückfragen des Gateways kommen im selben Terminal. `harness sync`/`merge` holen die Commits; `merge` prüft sie vorher (Regeln und OSV).

**Web-Chat (Phase 4, neu):**
1. Auf dem Rechner: `harness project remote on` im Projektordner, dann `harness connect` laufen lassen.
2. Im Web-Chat (bestehender Chat) im Composer „Umgebung“ → Gerät → Projekt.
3. Die erste Nachricht bestätigt man im Fenster von `harness connect`. Danach arbeitet pi in der Instanz. Der Chat zeigt Text, Werkzeugaufrufe (aufklappbar, mit Ausgabe) und Rückfragen des Gateways mit Knöpfen.
4. Die Commits des Agents holt man wie bisher mit `harness sync`/`merge`.

**Server per HTTPS:** Das Gateway spricht jetzt TLS. Ein Server hinter Caddy (öffentliches Zertifikat oder `tls internal` mit `[server] ca`) geht als LLM-Ziel, Anleitung in `docs/deploy.md`.

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alles ✓, solange die VM läuft), dann `bash scripts/smoke-connect.sh` (oder `smoke-run.sh` für Phase 2).

## Next step

1. **Nutzer testet Phase 4 mit DeepSeek**: Server läuft, `harness project remote on`, `harness connect`, im Web einen Chat verknüpfen. Rückmeldung zu Darstellung und Ablauf einholen.
2. Danach (Reihenfolge laut Nutzer):
   - **Jev bzw. ein Chat-Modell** als dritte Stufe der Merge-Prüfung und als Klassifikator gegen Prompt-Injection. Dafür: Server-Durchgang für Klassifikatoren, Einstellungen (was läuft, mit welchem Modell).
   - **Stopp der VM im Leerlauf** (Leases von `harness run` und `harness connect`).
3. Phase-4-Reste (ROADMAP Phase 4 „Offen“), am wichtigsten:
   - Übernehmen per Knopf (sync/merge mit Anzeige der Merge-Prüfung)
   - neuer Chat direkt mit Umgebung

Weitere Hinweise:
- pis Basis-URLs stehen in `@earendil-works/pi-ai/dist/providers/data/*.json`: DeepSeek `https://api.deepseek.com`, OpenAI `…/v1`, Anthropic ohne `/v1`, Google `…/v1beta`.
- Mistral nutzt dort `mistral-conversations`, das der Server-Durchgang nicht erlaubt. Mistral fehlt deshalb noch in `models.json`.
- pis RPC-Doku liegt im Tools-Volume unter `…/@earendil-works/pi-coding-agent/docs/rpc*.md` (lesen per `pm.sh run --rm -v harness-tools:/t:ro alpine …`).

## In flight / half-finished

- Nichts halb Fertiges im Code. Offene Phase-4-Punkte stehen in ROADMAP Phase 4.
- Das Terminal-Relay ist nur im simulierten Terminal und vom Nutzer kurz in Windows Terminal geprüft.
- Das echte `~/.harness` des Nutzers hat das Projekt `harness-test` in `$HOME\harness-test`. Eigene Probeläufe nur mit `HARNESS_HOME`.

## Decisions made this session

Phase 4 (17.10., Nutzer: „Mach als nächstes Phase 4 und parallel TLS im Gateway“):
- **WebSocket mit `ws` (Server) und `tungstenite` (Gerät)**, ausgehend vom Gerät. Reverse-Proxies wie Caddy reichen WebSockets ohne Konfiguration durch.
- **Agent-Antworten als normale AI-SDK-Nachrichten** (dynamische Tool-Teile, Datenteile) statt eines eigenen Formats. Chat-Ansicht, Speicherung und `useChat` bleiben gleich; Modell- und Agent-Antworten mischen sich in einem Chat.
- **Neutrale Agent-Ereignisse vom Gerät**: Nur das Gerät kennt pis RPC-Format. Weitere Harnesses (opencode) brauchen nur einen neuen Übersetzer.
- **Eine pi-Sitzung pro Chat** über `--session-id <conversation-id>`. Sie übersteht Neustarts von `harness connect`, den Verlauf bekommt pi nur bei leerer Sitzung.
- **Bestätigung neuer Chats auf dem Rechner**, gemerkt in der Projektdatei. Ohne Terminal wird abgelehnt. Grund: Wer das Server-Konto übernimmt, soll nicht ungefragt Agents auf dem Rechner starten.
- **Rückfragen des Gateways im Chat** beantwortet der Nutzer im Browser, ausgeführt von `harness connect` auf dem Host, nie im Container.
- **Erste Version als `harness connect` im Vordergrund** statt `harnessd`: Der Dienst samt App ist Phase 3. Die Logik in `connect.rs` lässt sich dorthin umziehen.

TLS im Gateway (17.10., gebaut von einem Hilfsagenten in einem eigenen Worktree):
- rustls/ring hinter dem Feature `tls`, weil Windows ARM64 ring nicht baut. Der Image-Bau lädt Crates, gepinnt über einen eigenen Lock.
- `[server] ca` ersetzt die öffentlichen CAs für den Server (Host und Gateway), die Prüfung ist nicht abschaltbar.

Früher (Kurzfassung, Details in ROADMAP):
- Keys nur im Server, auch für pi im Terminal; eigene Geräte-Tokens statt Better-Auth-Device-Plugin; Durchgang 1:1 im Protokoll des Anbieters.
- Rückfragen nie in der Oberfläche der Harness; pi in einem gemeinsamen, für Instanzen schreibgeschützten Tools-Volume.
- pi ohne eigene Netzzugriffe (`PI_OFFLINE=1`); Hinweise an den Agent im System-Prompt; Schutz-Erweiterung gegen Prompt-Injection.
- Der Nutzer erlaubt, nach jedem fertigen Stück zu committen, zu mergen und zu pushen.

## Open questions for the user

- Wie fühlt sich der Web-Chat mit pi an (Darstellung der Werkzeugaufrufe, Rückfragen, Bestätigung im connect-Fenster)?

## Known issues / debt

- **Mittel:** Neuladen oder Schließen des Tabs während einer Agent-Antwort bricht sie ab (Server kann Abbruch und Stop nicht unterscheiden). Abhilfe: fortsetzbare Streams.
- **Mittel:** „Neu erzeugen“ in einem verknüpften Chat schickt die Nachricht erneut; pis Sitzung enthält die alte Runde noch.
- **Mittel:** pis Sitzungen aller Projekte liegen gemeinsam in `harness-cfg-pi` (cwd ist überall `/workspace`). `--continue` im Terminal könnte die Sitzung eines anderen Projekts fortsetzen.
- **Mittel:** Die VM bekommt nur so viel RAM, wie Windows frei hat (Start 1 GB). `doctor` meldete am 17.10. 154 MiB frei in der VM. Bei mehreren Agents parallel den Startspeicher anheben.
- **Mittel:** ~350–500 ms pro podman-Aufruf (neue SSH-Verbindung). Der erste Chat-Prompt braucht ~10 s Vorbereitung.
- **Mittel:** Der LLM-Durchgang erfasst keinen Verbrauch pro Gerät, kein Limit pro Gerät.
- **Mittel:** Rückfragen hält das Gateway bis zu 120 s fest. pip bricht nach 15 s ab (`--timeout`).
- **Mittel:** `in.bundle` enthält jedes Mal die ganze Historie.
- **Mittel:** CLI-Texte nur Deutsch. Keine ESLint/Prettier-Konfiguration. Web-Haupt-Chunk ~955 kB.
- **Niedrig:** `https://localhost` geht fürs Gateway nicht (Zertifikat passt nicht zu `192.168.127.254`). Lokal weiter `http://localhost`.
- **Niedrig:** Image-Builds hinterlassen `<none>`-Images in der VM.
- **Niedrig:** Host-Key der VM per Trust on first use.
- Rechner-Stabilität: Bluescreen `0xA0` beim Start der Windows Sandbox. Nutzer vor riskanten Virtualisierungs-Starts fragen. Bei knappem Commit-Limit (17.10.: 29,6 von 31,6 GB) scheitern Browser-Tests.

## Verification state

Stand 17.10. abends, nach dem Merge von TLS und Phase 4:
- **Rust:** `cargo fmt --check`, `cargo clippy --all-targets` ohne Warnungen, `cargo test` grün (core 103 inkl. Tunnel-Protokoll, pi-Übersetzer und `[remote]`).
- **Server:** 49/49 Tests (SQLite), davon 9 neu für Hub, Verknüpfung, Agent-Stream und WebSocket mit Token. Postgres diesmal nicht gelaufen.
- **Typecheck:** alle drei Pakete grün.
- **Browser-E2E:** 10/10 Schritte, 0 Konsolenprobleme, neu mit simuliertem Gerät (Dialog, Rückfrage per Klick, gespeicherte Antwort). Screenshots 16–20 hell, dunkel und mobil angesehen.
- **VM:**
  - `scripts/smoke-connect.sh`: 19/19, mit dem neuen TLS-Gateway.
  - `gateway_vm`: 2/2, auch HTTPS mit eigener CA und mit falschem Namen.
  - `terminal_vm`: 3/3 (pi mit Schutz-Erweiterung über `harness run`).
- Gateway-Workflow auf GitHub grün (amd64, arm64). Gateway-Image beim Nutzer neu gebaut (`deac67bae652`), `doctor` grün.
- **Nicht geprüft:** echter Lauf mit DeepSeek über den Web-Chat, Bestätigung im connect-Fenster per Tastatur, echter Caddy.

---

## Log

### 2026-10-17 (Phase 4, TLS, Server-Image)
- Phase 4: Tunnel, Hub, `chatLink`, Agent-Stream (Server); `harness connect`, `project remote`, pi im RPC-Modus (Gerät); Umgebung-Knopf, Werkzeug-Blöcke, Rückfrage-Karten (Web); `scripts/smoke-connect.sh`.
- TLS im Gateway (rustls, eigener Lock, `[server] ca`, Workflow `gateway.yml`).
- Server-Image für amd64/arm64 mit GitHub Actions, README mit Quick Start, OSV-Abgleich, pi-Schutz gegen Prompt-Injection.

### 2026-10-16/17 (Phase 2b und 2c)
- Bundles per `podman cp`/`exec cat` statt 9p, `harness-vmd`, automatischer VM-Start, Merge-Prüfung Stufe 1.

### 2026-10-12 bis 2026-10-15 (Phase 2)
- Workspace, Provider, Base-Image, Konfiguration, Git-Sync, Gateway, Kopplung und LLM-Durchgang, `run`/`shell`/`sync`/`merge`, Terminal-Relay, Hinweise im System-Prompt.

### 2026-10-10 bis 2026-10-11
- Architektur, Server und Web-App (Phase 1), WSL-Spike, E2E-Test.
