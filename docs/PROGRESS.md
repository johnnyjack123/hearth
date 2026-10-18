# Progress

_Last updated: 2026-10-18 · Phase 4 fertig und überarbeitet (Hintergrunddienst, `harness init`, Fortschritt, Kontext und Kosten, Web-Fetch in normalen Chats); nächstes: Test des Nutzers, dann Block „Recherche“_

## Where we are

Repo auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`), Arbeitsbranch `feat/phase-4-chat-env`. Nach jedem fertigen, geprüften Stück wird nach `main` gemergt und gepusht (Freigabe des Nutzers).

**Terminal:** Im Projektordner startet `harness` (bzw. `harness run pi`) Gateway, Instanz und pi in der VM. Rückfragen des Gateways kommen im selben Terminal. `harness sync`/`merge` holen die Commits des Agents; `merge` prüft sie vorher (Regeln, OSV).

**Web-Chat:**
1. `harness login <Server>` koppelt den Rechner mit dem Konto und richtet den Hintergrunddienst ein (`harnessd.exe` bei jeder Anmeldung; bei schon gekoppeltem Rechner `harness daemon install`).
2. `harness init` im Projektordner meldet das Projekt dem Konto.
3. Im Chat „Umgebung“ → Gerät → Projekt. Die erste Nachricht startet VM, Gateway, Instanz und pi. Im Chat erscheinen:
   - jeder Schritt mit Dauer
   - Werkzeugaufrufe
   - Rückfragen des Gateways mit Knöpfen
   - Kontext-Füllstand und Kosten
4. **Ohne Umgebung** lädt das Modell Webseiten über `web_fetch` im Server (nur öffentliche Adressen; fremde Hosts mit Freigabe im Chat).

**Server des Nutzers:** Docker-Compose mit Postgres auf seinem Raspberry Pi, `http://192.168.1.20:8085`. Das Image baut GitHub bei jedem Push auf `main`. Der Windows-Rechner war am 18.10. noch mit dem lokalen Dev-Server `http://localhost:8080` gekoppelt.

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alles ✓, solange die VM läuft), dann `bash scripts/smoke-connect.sh` (~1 min). Den Browser-E2E nur bei freiem Speicher laufen lassen (siehe Known issues).

## Next step

1. **Test des Nutzers abwarten** (er hat den neuen Ablauf noch nicht ausprobiert): `harness daemon install`, `harness init`, verknüpfter Chat mit DeepSeek, Web-Fetch in einem normalen Chat. Seine Rückmeldung geht vor.
2. **Block „Recherche“** (Wünsche des Nutzers vom 18.10., Plan in ROADMAP Phase 4 „Recherche“), in dieser Reihenfolge:
   1. **Fundstellen von Prompt-Injection zeigen und erklären** (klein, sofort nützlich):
      - `scan()` in `crates/harness-core/pi/guard.ts` und `apps/server/src/webfetch/injection.ts` liefert pro Treffer die Stelle mit Umgebung; unsichtbare Zeichen werden sichtbar, Tag-Zeichen entschlüsselt.
      - Zu jeder Art von Fund kommt ein kurzer Satz, **warum** das gefährlich ist, für Laien verständlich (Wunsch des Nutzers).
      - Web: aufklappbar in `web-fetch.tsx` und bei Agent-Hinweisen. pi: Ausschnitt in der Meldung.
      - Beide Musterdateien gleich halten, ein Test vergleicht sie schon.
   2. **Hauptinhalt statt ganzer Seite:** `@mozilla/readability` + `linkedom` in `apps/server/src/webfetch/convert.ts` vor turndown, mit Rückfall auf die ganze Seite. Testseiten als Fixtures.
   3. **Websuche:**
      - Werkzeug `web_search`
      - SearXNG als Container im Compose (Profil `search`), wahlweise Brave/Tavily mit Key
      - Recherche-Schalter im Composer: freie Abrufe nur für URLs aus Suchergebnissen, Nutzernachrichten und Links geladener Seiten; selbst gebaute URLs fragen weiter
3. Danach (Reihenfolge laut Nutzer):
   - **Jev bzw. Chat-Modell als Klassifikator** (dritte Stufe der Merge-Prüfung und gegen Prompt-Injection)
   - **Stopp der VM im Leerlauf**
4. Phase-4-Reste: Übernehmen per Knopf (sync/merge samt Merge-Prüfung im Web), neuer Chat direkt mit Umgebung, fortsetzbare Streams.
5. Später notiert (ROADMAP Phase 5): Dokumente per markitdown, Bilder für Vision-Modelle, Windows-Builds auf GitHub mit dem Installer.

## In flight / half-finished

Nichts. Alles ist committet, gemergt und gepusht; keine Hilfsagenten oder Worktrees offen.

## Decisions made this session (16.–18.10.)

- **Hintergrunddienst als Benutzerprozess** mit Run-Schlüssel statt Windows-Dienst: Er braucht das Token des Nutzers (Anmeldeinformationsverwaltung) und `~/.harness`. Ein Mutex pro Konfiguration verhindert zwei Verbindungen.
- **Projekte sind standardmäßig freigegeben, ohne Bestätigung pro Chat** (Nutzer, 18.10.). Schutz liegt in der Sandbox und darin, dass nur Chats des eigenen Kontos das Gerät erreichen. `confirm = true` geht noch, dann nur mit `harness connect`.
- **Agent-Antworten als normale AI-SDK-Nachrichten** (`dynamic-tool`, Datenteile `notice`/`approval`/`progress`). Eine pi-Sitzung pro Chat (`--session-id`). Nur das Gerät kennt pis RPC-Format.
- **Fortschritt jedes Schritts im Chat**, weil der Nutzer beim ersten Test nicht sah, ob etwas passiert.
- **Kosten aus models.dev**, pi rechnet für Agents selbst. Ohne Katalog fehlt nur der Preis.
- **Web-Fetch im Server, nicht in der VM.** Fremde Hosts brauchen eine Freigabe, damit ein gekapertes Modell keine Daten in URLs hinausschicken kann.
- **TLS im Gateway** mit rustls hinter Feature `tls` (ring baut nicht auf Windows ARM64). `[server] ca` ersetzt die öffentlichen CAs.
- **Merge-Prüfung:** KI-Stufen dürfen nur Funde hinzufügen, nie Regel-Funde wegerklären.
- Früher: Keys nur im Server; Rückfragen nie in der Oberfläche der Harness; pi ohne eigene Netzzugriffe (`PI_OFFLINE=1`).

## Open questions for the user

- Rückmeldung zum neuen Ablauf und zu Web-Fetch mit DeepSeek.
- Suchanbieter: SearXNG auf dem Pi als Standard einverstanden?

## Known issues / debt

- **Mittel:** Neuladen oder Schließen des Tabs während einer Agent-Antwort bricht sie ab. Abhilfe: fortsetzbare Streams.
- **Mittel:** „Neu erzeugen“ in einem verknüpften Chat schickt die Nachricht erneut; pis Sitzung enthält die alte Runde noch.
- **Mittel:** pis Sitzungen aller Projekte liegen gemeinsam in `harness-cfg-pi` (cwd überall `/workspace`).
- **Mittel:** Die VM bekommt nur RAM, den Windows frei hat (Start 1 GB; `doctor` zeigte am 17.10. 154 MiB frei). Bei mehreren Agents den Startspeicher anheben.
- **Mittel:** ~350–500 ms pro podman-Aufruf; der erste Chat-Prompt braucht ~10 s Vorbereitung, mit VM-Start ~30 s.
- **Mittel:** Kein Verbrauch oder Limit pro Gerät im LLM-Durchgang.
- **Mittel:** Web-Fetch: Markdown-Bilder könnten Daten hinausschicken, nur die CSP der Produktion blockt das (Vite-Dev-Server hat keine).
- **Mittel:** `in.bundle` enthält jedes Mal die ganze Historie. CLI-Texte nur Deutsch. Keine ESLint-Konfiguration.
- **Niedrig:** `https://localhost` geht fürs Gateway nicht; `<none>`-Images in der VM; Host-Key der VM per Trust on first use.
- **Rechner:** Wenig freier Speicher. Bei knappem Commit-Limit (17.10.: 29,6 von 31,6 GB) scheitert der Browser-E2E mit `ERR_INSUFFICIENT_RESOURCES` oder Timeouts, auch in unveränderten Schritten. Dann parallele Builds beenden und wiederholen. Windows Sandbox nie ungefragt starten (Bluescreen am 12.10.).

## Verification state

Stand 18.10., nach dem Merge von Web-Fetch:
- **Rust:** `cargo fmt --check`, `cargo clippy --all-targets` ohne Warnungen, `cargo test` grün (core 103).
- **Server:** 89/89 (SQLite): Hub, Verknüpfung, Agent-Stream, Tunnel mit Token, gespeicherte Umgebungen, Preise, Web-Fetch (31). Postgres zuletzt am 14.10.
- **Typecheck:** alle drei Pakete grün.
- **Browser-E2E:** 11/11, 0 Konsolenprobleme. Geprüft:
  - Web-Fetch mit Rückfrage
  - Umgebung verknüpfen, ohne den Entwurf abzuschicken
  - Fortschritt, Rückfrage des Gateways, Kosten, VM-Status
  - Screenshots hell, dunkel und mobil angesehen
- **VM:**
  - `scripts/smoke-connect.sh` 30/30: `harness init`, Ablehnung mit `confirm = true`, echter Hintergrunddienst, pi per RPC, Rückfrage, `harness sync`, `daemon stop`
  - `terminal_vm` 3/3, `gateway_vm` 2/2 (auch HTTPS)
- **GitHub:** Server-Image und Gateway-Workflow grün.
- **Nicht geprüft:**
  - Autostart nach echter Windows-Anmeldung
  - Lauf mit DeepSeek über den Web-Chat
  - echter Caddy
  - Postgres seit Phase 4

---

## Log

### 2026-10-18 (Phase 4 nach dem ersten Test)
- Bugfixes: Composer-Knöpfe schickten die Nachricht ab; DeepSeek schrieb nach einer Agent-Runde Werkzeugaufrufe als Text (`agentToolsAsText`).
- Hintergrunddienst (`harness daemon`, `harnessd.exe`), `harness init`, gespeicherte Umgebungen, VM-Status, Fortschritt im Chat.
- Kontext und Kosten (`pricing.ts`, models.dev; pi für Agents), Ring im Composer.
- Web-Fetch in normalen Chats (Hilfsagent, `apps/server/src/webfetch/`).
- Fehlerhinweise für den Pi-Server: ungültige `DATABASE_URL`, fehlendes Compose-Profil, nicht beschreibbarer Datenordner.

### 2026-10-17 (Phase 4, TLS, Server-Image)
- Tunnel, Hub, `chatLink`, Agent-Stream; `harness connect`, pi im RPC-Modus; Umgebung-Knopf, Werkzeug-Blöcke, Rückfrage-Karten.
- TLS im Gateway (Hilfsagent). Server-Image amd64/arm64, README, OSV-Abgleich, pi-Schutz gegen Prompt-Injection.

### 2026-10-12 bis 2026-10-17 (Phase 2, 2b, 2c)
- Workspace, Provider, Base-Image, Konfiguration, Git-Sync, Gateway, Kopplung, LLM-Durchgang, `run`/`shell`/`sync`/`merge`, Terminal-Relay, VM-Dienst, Merge-Prüfung Stufe 1.

### 2026-10-10 bis 2026-10-11
- Architektur, Server und Web-App (Phase 1), WSL-Spike, E2E-Test.
