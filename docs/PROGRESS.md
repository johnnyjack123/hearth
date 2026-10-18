# Progress

_Last updated: 2026-10-18 · Block „Recherche“ fertig (Fundstellen mit Erklärung, Hauptinhalt per Readability, Websuche mit Recherche-Schalter); nächstes: Test des Nutzers mit Websuche auf dem Pi_

## Where we are

Repo auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). Gearbeitet wird auf Feature-Branches (zuletzt `feat/research`), die nach jedem geprüften Stück per `--no-ff` in `main` gemergt und gepusht werden (Freigabe des Nutzers). `feat/phase-4-chat-env` ist seit dem 18.10. nicht mehr der Arbeitsbranch.

**Terminal:** Im Projektordner startet `harness` (bzw. `harness run pi`) Gateway, Instanz und pi in der VM. Rückfragen des Gateways kommen im selben Terminal. `harness sync`/`merge` holen die Commits des Agents; `merge` prüft sie vorher (Regeln, OSV).

**Web-Chat:**
1. `harness login <Server>` koppelt den Rechner und richtet den Hintergrunddienst ein; `harness init` meldet ein Projekt.
2. Im Chat „Umgebung“ → Gerät → Projekt: pi arbeitet in der VM. Schritte, Werkzeugaufrufe, Rückfragen, Kontext und Kosten erscheinen im Chat. Warnungen des pi-Schutzes zeigen aufklappbar Fundstelle und Grund.
3. **Ohne Umgebung** lädt das Modell Seiten über `web_fetch` im Server, als Hauptinhalt (Readability). Fremde Hosts brauchen eine Freigabe. Bei Verdacht auf Prompt-Injection zeigt die Warnung aufklappbar jede Fundstelle mit Ausschnitt und einem Satz, warum das gefährlich ist.
4. **Recherche-Schalter** im Composer (nur wenn ein Admin unter Administration → Server → Websuche einen Anbieter gespeichert hat): Das Modell sucht mit `web_search` und liest Treffer und Links geladener Seiten ohne Rückfrage. Unter der Antwort stehen die gelesenen Seiten als Quellen.

**Server des Nutzers:** Docker-Compose mit Postgres auf seinem Raspberry Pi, `http://192.168.1.20:8085`. Das Image baut GitHub bei jedem Push auf `main`. Der Windows-Rechner war am 18.10. mit diesem Pi-Server gekoppelt (`harness doctor`).

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor`, dann `bash scripts/smoke-connect.sh` (~1 min). Browser-E2E nur bei freiem Speicher (siehe Known issues).

## Next step

1. **Test des Nutzers auf dem Pi** (seine Wahl vom 18.10. für den echten Suchlauf):
   - `docker compose pull`, dann mit Profil `search` starten (`COMPOSE_PROFILES=postgres,search` in `.env`, dazu `SEARXNG_SECRET`). Die neue `docker-compose.yml` muss auf den Pi; sie enthält die SearXNG-Konfiguration inline.
   - In der Web-App: Administration → Server → Websuche → SearXNG, `http://searxng:8080`, speichern, „Suche testen“.
   - Chat mit Recherche-Schalter und DeepSeek; dabei auf Antwortqualität, Dauer auf dem Pi (Readability bei großen Seiten) und Quellen achten.
   - Seine Rückmeldung geht vor. Typische Fehler stehen in `docs/deploy.md` („Websuche“).
2. Danach (Reihenfolge laut Nutzer):
   - **Zweitmeinung zu Warnungen** (ROADMAP, eigener Abschnitt): Jev oder ein Chat-Modell bewertet Warnungen mit frischem Kontext. Die Fundstellen (`InjectionFinding`) sind jetzt die natürliche Eingabe dafür.
   - **Stopp der VM im Leerlauf**
3. Phase-4-Reste: Übernehmen per Knopf (sync/merge samt Merge-Prüfung im Web), neuer Chat direkt mit Umgebung, fortsetzbare Streams, Suche für Agents in der VM (über den Server).
4. Später (ROADMAP Phase 5): Dokumente per markitdown, Bilder für Vision-Modelle, Windows-Builds auf GitHub.

## In flight / half-finished

Nichts. Alles ist committet, nach `main` gemergt und gepusht; keine Hilfsagenten oder Worktrees offen. Offen ist nur der echte Suchlauf (siehe oben).

## Decisions made this session (18.10.)

- **Suchanbieter:** SearXNG (Compose-Profil `search`) plus wahlweise Brave oder Tavily mit Key, alle hinter `SearchBackend`. So hat der Nutzer es gewählt.
- **Recherche erlaubt exakte URLs, nie Hosts.** Treffer und Links geladener Seiten hat ein Dritter geschrieben, ohne den Chat zu kennen. Eine selbst gebaute URL könnte Chat-Inhalte hinaustragen, auch auf einem bekannten Host. Weiterleitungen einer gefundenen URL sind frei, weil ihr Server sie wählt.
- **Suchanfragen gehen ungefiltert an den Anbieter.** Das lässt sich nicht verhindern; der System-Prompt bittet das Modell, nichts Privates hineinzuschreiben (Hinweis in `docs/deploy.md`).
- **Hauptinhalt mit Anteilsgrenze 20 %:** Auf den Startseiten von heise und SPIEGEL griff Readability einen Werbekasten (Anteil 0,12 bzw. 0,07). Dann lieber die ganze Seite als Information verlieren.
- **Fundstellen werden roh gespeichert**, unsichtbare Zeichen eingeschlossen. Sichtbar machen sie die Oberflächen; im Terminal (pi) werden auch Steuerzeichen ersetzt, damit keine Escape-Folgen durchkommen.
- **Hinweise vom Gerät bleiben Text:** Die erste Zeile ist die Meldung, weitere Zeilen sind aufklappbare Details. So bleibt das Tunnel-Protokoll unverändert (kein `TUNNEL_VERSION`-Sprung).
- Der Test vergleicht pi-Schutz und Server jetzt im Verhalten: Er lädt `guard.ts` per dynamischem `import()` und prüft beide auf denselben Texten.

## Open questions for the user

- Rückmeldung zur Websuche auf dem Pi (Qualität, Tempo) und weiterhin zum Ablauf mit Hintergrunddienst und DeepSeek.

## Known issues / debt

- **Mittel:** Echter Suchlauf (SearXNG, Brave, Tavily) noch nicht gegen echte Dienste geprüft, nur gegen Stand-ins. Die Antwortformate sind aus deren Doku.
- **Mittel:** Readability braucht für 1–2-MB-Seiten hier ~1 s, auf dem Pi vermutlich mehrere Sekunden.
- **Mittel:** Neuladen oder Schließen des Tabs während einer Agent-Antwort bricht sie ab (fortsetzbare Streams fehlen).
- **Mittel:** „Neu erzeugen“ in einem verknüpften Chat schickt die Nachricht erneut; pis Sitzung enthält die alte Runde noch.
- **Mittel:** pis Sitzungen aller Projekte liegen gemeinsam in `harness-cfg-pi`.
- **Mittel:** VM-RAM knapp (Start 1 GB; am 18.10. 183 MiB frei); ~350–500 ms pro podman-Aufruf.
- **Mittel:** Kein Verbrauch oder Limit pro Gerät im LLM-Durchgang.
- **Mittel:** Markdown-Bilder könnten Daten hinausschicken, nur die CSP der Produktion blockt das (Vite-Dev-Server hat keine).
- **Niedrig:** Ob pis Oberfläche mehrzeilige Warnungen sauber umbricht, ist nur am Code geprüft (`showWarning` → `ThemedText`), nicht im Terminal gesehen.
- **Rechner:** Wenig freier Speicher. Der Browser-E2E scheiterte am 18.10. zweimal mit `ERR_INSUFFICIENT_RESOURCES` und lief beim zweiten Versuch grün. Windows Sandbox nie ungefragt starten.

## Verification state

Stand 18.10., nach dem Merge von Schritt 3:
- **Server:** 103/103 (SQLite), darunter Websuche (Anbieter-Stand-ins, Einstellungen, Recherche mit Drehbuch-Modell), Readability-Fixtures, Vergleich pi-Schutz ↔ Server. Postgres zuletzt am 14.10.
- **pi-Schutz:** `node --test crates/harness-core/pi/guard.test.ts` 6/6; `cargo test -p harness-core --lib` 103 grün (nach Schritt 1).
- **Typecheck:** alle drei Pakete grün.
- **Browser-E2E:** 12/12, 0 Konsolenprobleme:
  - Fundstellen mit Tag-Zeichen
  - Hauptinhalt
  - Websuche einrichten und testen
  - Recherche ohne Rückfrage, Quellen, Schalter nach Neuladen
  - Screenshots angesehen (dunkel Desktop, hell mobil)
- **Echte Läufe:**
  - `scripts/smoke-connect.sh` 27/27: Die Warnung des pi-Schutzes kommt mit Fundstelle aus der VM in den Chat.
  - Readability gegen echte Seiten: Wikipedia, MDN, Node-Doku, GitHub, heise und SPIEGEL (Start und Artikel), tagesschau, example.com
  - `docker compose --profile search config` gültig
- **Nicht geprüft:** echter Suchlauf; SearXNG-Container; Autostart nach echter Windows-Anmeldung; Lauf mit DeepSeek über den Web-Chat; echter Caddy.

---

## Log

### 2026-10-18 (Block „Recherche“)
- Fundstellen von Prompt-Injection mit Ausschnitt und Erklärung (`findInjections` in `guard.ts` und `injection.ts`, `injection-findings.tsx`, aufklappbare Agent-Hinweise).
- Hauptinhalt per Readability + linkedom (`pageToMarkdown`), Tabellen als Markdown, `fullPage`.
- Websuche: `apps/server/src/websearch/`, Admin-Einstellungen mit Testsuche, Recherche-Schalter, Trefferblock, Quellen, SearXNG im Compose.

### 2026-10-18 (Phase 4 nach dem ersten Test)
- Hintergrunddienst, `harness init`, gespeicherte Umgebungen, Fortschritt, Kontext und Kosten, Web-Fetch in normalen Chats.

### 2026-10-17 (Phase 4, TLS, Server-Image)
- Tunnel, Hub, Agent-Stream, pi im RPC-Modus; TLS im Gateway; Server-Image amd64/arm64; pi-Schutz gegen Prompt-Injection.

### 2026-10-12 bis 2026-10-17 (Phase 2, 2b, 2c)
- Workspace, Provider, Base-Image, Konfiguration, Git-Sync, Gateway, Kopplung, LLM-Durchgang, `run`/`shell`/`sync`/`merge`, Terminal-Relay, VM-Dienst, Merge-Prüfung Stufe 1.

### 2026-10-10 bis 2026-10-11
- Architektur, Server und Web-App (Phase 1), WSL-Spike, E2E-Test.
