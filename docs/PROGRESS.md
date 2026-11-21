# Progress

_Last updated: 2026-11-21 · Phase 6 begonnen (Direktmodus, Desktop-App, macOS und Linux): Client baut unter Linux und macOS, Direktmodus Ende zu Ende gebaut und unter Windows echt geprüft. Standardmodell für neue Chats ist fertig._

## Where we are

Hearth ist ein selbst gehosteter Chat für LLMs (Server + Web-App) mit einem Windows-Client, der Coding-Agents (pi) in einer Podman-VM unter Hyper-V laufen lässt. Was es kann, steht in `README.md`. Wie es gebaut ist, steht in `ARCHITECTURE.md`. Die Stolpersteine je Bereich stehen in `CLAUDE.md`, und dort steht unter „Stand“ auch, was wann dazukam.

- **Server des Nutzers:** läuft auf seinem Heimserver hinter Nginx Proxy Manager mit HTTPS, also im Internet (Details nur in `CLAUDE.local.md`). Das Image `ghcr.io/johnnyjack123/hearth-server` baut `.github/workflows/server-image.yml` bei jedem Push auf `main`. Der Nutzer aktualisiert selbst: `docker compose pull`, dann `docker compose up -d --no-deps server`.
- **Client:** Release v0.3.0 (11.11.), `hearth-setup-<arch>.exe` aus den GitHub-Releases. Der Nutzer koppelt mit `hearth setup --server <adresse>`, Projekte meldet `hearth init`.
- **Web-App** auch als PWA auf dem Handy:
  - Abstand zu Notch und Home-Balken, Tastatur verkleinert die Seite, Modellname läuft von selbst durch
  - Foto-Auswahl beim Anhängen
  - gleicher Chat auf mehreren Geräten (`GET /api/events`)
  - Hinweis „Neu laden“ nach einem Server-Update
- **Repo:** `github.com/johnnyjack123/hearth`, seit 17.11. öffentlich. Feature-Branch je Block, nach jedem geprüften Stück per `--no-ff` in `main` gemergt und gepusht (Freigabe des Nutzers).

Schnelltest für eine neue Sitzung: `pnpm -r typecheck`, `pnpm --filter @hearth/server test`; für den Client `cargo build && target/debug/hearth doctor`. Browser-E2E und Smoke-Skripte nur mit freiem Speicher (siehe Known issues).

## Next step

1. **Rückmeldung des Nutzers von echten Geräten:** Der Live-Abgleich zwischen Handy und Laptop geht (18.11.). Die Foto-Auswahl war ausgegraut, weil im Chat DeepSeek V4 Pro gewählt war; laut models.dev liest es nur Text. Seit `fix/photo-hint` steht der Grund im Menü (gemergt 18.11.). Offen ist noch die Tastatur auf dem iPhone.
2. **Erledigt (21.11.): Standardmodell für neue Chats.** Einstellungen → Chat, gilt auf allen Geräten. Der Nutzer stellt dort einmal DeepSeek V4.1 Flash über OpenRouter ein, sobald der Server aktualisiert ist.
3. **Kleine Client-Texte** beim nächsten Client-Release mitnehmen (siehe Known issues: `pi.toml`-Kommentar, Hinweis in `hearth init`).
4. **Phase 6 (ROADMAP, Plan vom 21.11., Konzept `docs/security/concept.md`):**
   - ✅ Schritt 1: Unix-Build (Workflow `unix.yml` grün auf Linux x64/arm64 und macOS, Tests auf dem Linux-Laptop grün).
   - ✅ Schritt 2: Proxy-Kern `hearth-core/src/host/`.
   - ✅ Schritt 3: Tunnel-Protokoll 11, Server-Schleife, Web-Karten, `hearth project host`, `hearth host`. Geprüft: Server-Tests, Browser-E2E (25/25), `scripts/smoke-host.sh` unter Windows (26/26). **Offen:** `smoke-host.sh` auf dem Linux-Laptop (war am 21.11. nachmittags nicht erreichbar; Weg: Server und Mock unter Windows, `ssh -R 8097:127.0.0.1:8097 linux-laptop`, dort `SERVER_URL=http://127.0.0.1:8097 SERVER_COOKIES=… CREDENTIAL=… bash scripts/smoke-host.sh`).
   - Schritt 4: Benachrichtigung und Freigabe-Dialog am Rechner (heute nur `hearth host` im Terminal), geht in Schritt 5 auf.
   - 🔨 Schritt 5: Desktop-App `hearth-app` (egui) im ersten Stand: Freigaben, Projekte mit Modus, Rechner, Protokoll; Tray (Windows) öffnet sie und meldet Freigaben. Offen: Netzregeln und Programme je Projekt in der App, Tray unter macOS und Linux, Startmenü-Eintrag.
   - 🔨 Schritt 7 vorgezogen: Pakete für Linux (x64, arm64, OpenSSL eingebaut) und macOS (Universal-App, ad hoc signiert) mit `scripts/install.sh` und Workflow `unix-release.yml`; Linux/macOS: `hearth setup`, `hearth doctor`, Token-Datei, Benachrichtigungen ohne Tray. Anleitung `docs/client-unix.md`.
   - Offen: Sandbox unter Linux/macOS (Schritt 6), Tray-Symbol unter Linux/macOS, Test auf dem Linux-Laptop.
   - Der Server des Nutzers braucht für den Direktmodus ein Update (Protokoll 11); ein älterer Server bekommt Direktmodus-Projekte gar nicht angeboten.
5. **Vorgemerkte kleinere Wünsche** (ROADMAP Phase 5, „Vorgemerkt“):
   - Copy-Knopf an eigenen Nachrichten
   - Schalter, ob frühere Denkblöcke mitgehen (A/B-Test)
   - Export und Import ausgewählter Chats (die alte Instanz auf dem Pi ist die Quelle; sie bleibt, bis das gebaut ist)
6. **Weg zu 1.0** (ROADMAP): Neuinstallation auf frischem Windows (auch x64), zweite Sicherheitsprüfung durch einen frischen Agenten, dann `v1.0.0-rc.1`.

## In flight / half-finished

Nichts Halbes. Alles ist committet, nach `main` gemergt und gepusht. Auf dem Linux-Laptop liegen rustup (`~/.rustup`, `~/.cargo`) und ein Klon unter `~/Dokumente/Programmieren/hearth_tests/hearth` (Freigabe des Nutzers vom 21.11.).

## Decisions made 21.11.

- **Direktmodus** (Nutzer): Freigaben am Rechner, im Browser nur per Opt-in je Projekt (`[host] browser_approvals`), weil ein gekaperter Server sonst Befehle freigeben könnte. Die Modellschleife läuft im Server (wie normale Chats), die Werkzeuge im Rust-Proxy; kein pi und kein Node auf dem Rechner.
- **Desktop-App mit egui/eframe** (Nutzer), Tray bleibt ein eigener kleiner Prozess, das Fenster läuft nur, solange es offen ist.
- **Standardmodell für neue Chats** als Einstellung je Nutzer auf dem Server, nicht fest verdrahtet.

## Decisions made 12.–17.11. (älter)

- **Repo öffentlich** (17.11., Nutzer): Die Actions-Minuten für private Repos waren aufgebraucht, öffentliche Repos haben freie Runner. Die Historie wurde dreimal umgeschrieben (E-Mail, Hostnamen und Heimnetz, Serverpfade und Hardware). Eine Support-Anfrage, damit GitHub die alten Stände löscht, stellt der Nutzer nicht: Seine E-Mail steht ohnehin in 13 seiner anderen öffentlichen Repos, und Hostnamen und private Adressen hält er für unkritisch. Alte Stände bleiben deshalb über die Activity-Ansicht erreichbar. Vor dem Umschalten wurde der ganze Verlauf auf Keys, Tokens, die öffentliche Domain und Namen durchsucht; es gab keine Treffer außer zwei Testschlüsseln.
- **Freier Webzugriff gilt nur für normale Chats**, nicht für Agents. Deren Netz bestimmt der Rechner, sonst könnte ein feindlicher Server die Sandbox öffnen (Client-Prüfung, Fund 7).
- **Live-Abgleich per Server-Sent Events, nicht über den Geräte-WebSocket.** Es gibt eine Verbindung je Browser (Web Locks plus BroadcastChannel), weil über HTTP/1.1 nur sechs Verbindungen je Server gehen und NPM beim Nutzer kein HTTP/2 spricht.
- **Kein Service-Worker in der PWA.** Updates bleiben dadurch einfach: Die Startseite ist `no-cache`, und nach einem Update erscheint ein Hinweis. Offline geht deshalb nichts; das Modell läuft ohnehin auf dem Server.
- **Client bleibt auf v0.3.0:** Seitdem hat sich nur die Lizenzangabe geändert, ein Release lohnt nicht.

## Open questions for the user

- **Gelöst (18.11.): `ERR_TOO_MANY_RETRIES` in der PWA im WLAN.** Laut Mitschnitt aus `chrome://net-export` (Chrome 157, Android 17) scheitert jede Verbindung zur lokalen Adresse mit `ERR_LOCAL_NETWORK_PERMISSION_MISSING`: Android 17 verlangt für Verbindungen ins lokale Netz eine Berechtigung, und Chrome hatte sie nicht. Chrome startet die Anfrage dann bis zum Limit neu. Mit mobilen Daten zeigt der Name auf die öffentliche Adresse, deshalb ging es dort. Abhilfe: Chrome die Berechtigung erteilen; beim Nutzer bestätigt (18.11.). Steht in `docs/deploy.md` unter „Probleme“.
- Jev-Schwellen bei der Merge-Prüfung (ab 75 % schwer, ab 50 % mittel) sind geschätzt; Rückmeldung nach echten Diffs.
- Ob der OpenRouter-Key aus dem Chat vom 21.10. widerrufen wurde (er stand im Gesprächsverlauf).

## Known issues / debt

- **Mittel:** Ungeprüft auf echten Geräten: Foto-Auswahl, Live-Abgleich und Tastatur auf iOS. Der E2E deckt sie nur emuliert ab.
- **Mittel:** Die Chat-Suche ist gegen Postgres ungeprüft (nur SQLite).
- **Mittel:** Laufende Antworten, der Geräte-Hub und die Live-Ereignisse liegen nur im Speicher; ein Neustart des Servers beendet laufende Antworten.
- **Mittel:** „Neu erzeugen“ in einem verknüpften Chat schickt die Nachricht erneut, aber pis Sitzung enthält die alte Runde noch.
- **Mittel:** Die VM hat wenig RAM (auf ARM64 wächst sie nicht), und jeder podman-Aufruf kostet ~0,4 s.
- **Mittel:** Markdown-Bilder könnten Daten hinaustragen; das blockt nur die CSP der Produktion (der Vite-Dev-Server sendet keine).
- **Mittel:** Brave, Tavily, Jev und die Embeddings sind in Tests nur gegen Stand-ins geprüft.
- **Niedrig:** Über `http://` im LAN gibt es keine Web Locks, dann hält jeder Tab eine eigene Live-Verbindung (HTTP/1.1: höchstens sechs je Server).
- **Niedrig:** Fund 8 und 10 der Client-Prüfung sind teilweise offen (`docs/security/review-2026-11-10-client.md`).
- Die Images `hearth-server` und `hearth-markitdown` sind seit 17.11. öffentlich (anonym geprüft). Die alten `llm-harness-server` (versehentlich öffentlich) und `llm-harness-markitdown` aus der Zeit vor der Umbenennung kann der Nutzer löschen; die alte Pi-Instanz hat sie noch lokal.
- **Niedrig:** Veraltete Texte im Client, mit dem nächsten Release korrigieren: Der Kommentar in `crates/hearth-core/defaults/harnesses/pi.toml` sagt, `~/.pi` gelte für alle Projekte (seit 0.1.17 eines je Projekt). `hearth init` (`project.rs`) schickt den Nutzer zu „Umgebung“ unten im Chat (heute im Zahnrad-Menü).
- **Niedrig:** Der LLM-Durchgang (`/api/llm/`) sendet absichtlich keine Lebenszeichen. Schweigt ein Anbieter länger als `proxy_read_timeout` (nginx: 60 s), bricht der Proxy die Antwort ab.
- **Rechner des Entwicklers:** Bei knappem Speicher scheitert der Browser-E2E mit `ERR_INSUFFICIENT_RESOURCES`; dann einfach noch einmal laufen lassen. Windows Defender hält `hearth.exe` manchmal für einen Trojaner (Fehlalarm des maschinellen Lernens). Windows Sandbox nie ungefragt starten.

## Verification state

Stand: Commit `f70a911` (14.11.); danach nur Doku und `scripts/install.ps1` (lädt das Release ohne GitHub-CLI). Beim Skript ist nur die Syntax geprüft (PowerShell-Parser) und dass die Release-Adresse antwortet; installiert wurde damit nicht.
- `pnpm -r typecheck`: grün.
- `pnpm --filter @hearth/web build`: grün.
- Server-Tests: 40 Dateien, 321 Tests grün, davon neu `test/events.test.ts` mit Live-Ereignissen, Cache-Headern und Build-Tag.
- Server-Image vom 17.11.: grün (amd64 und arm64, Smoke-Test im Workflow).
- Browser-E2E (18.11., nach `fix/photo-hint`): 23/23 Schritte, 0 Konsolenprobleme. Der Schritt „a chat open on phone and laptop …“ prüft jetzt auch das Menü bei einem Modell ohne Bilder. Screenshots `07f`, `07g` und `07h` angesehen. Vorher scheiterten drei Läufe an `ERR_INSUFFICIENT_RESOURCES` (1,8 GB Commit frei); mit 5 GB lief er durch.
- Nicht geprüft:
  - Rust-Tests und Smoke-Skripte; der Client-Code ist seit v0.3.0 unverändert.
  - Echte Handys.

---

## Log

### 2026-11-21
- Standardmodell für neue Chats (Einstellungen → Chat).
- Phase 6 geplant und begonnen: Sicherheitskonzept, Client baut unter Linux und macOS (CI), Proxy-Kern, Tunnel-Protokoll 11 mit Direktmodus in Server, Web und CLI, Smoke-Test `smoke-host.sh`.

### 2026-11-13/14
- Live-Abgleich zwischen Geräten (`/api/events`), Foto-Auswahl auf Touch-Geräten, Sitzung bleibt bei Verbindungsfehlern, Startseite `no-cache` mit Build-Tag und Hinweis „Neu laden“, `immutable` für `/assets` kommt jetzt wirklich an.
- Suche in den Nachrichten (Migration 0021), Neuladen statt Fehlerseite nach einem Update, Tastatur verkleinert die Seite, Modellname läuft auf Touch-Geräten von selbst durch, Abstand zu Notch und Home-Balken.
- Freier Webzugriff je Nutzer (`webFetchOpen`). Lizenz als `LICENSE.md`. Die Historie wurde zweimal umgeschrieben (Hostnamen und Heimnetz, dann Serverpfade und Hardware).

### 2026-11-12
- AGPL-3.0-or-later, PWA installierbar, persönliche Angaben in `CLAUDE.local.md`, erste Umschreibung der Historie (noreply-Adresse).

### 2026-11-08 bis 11 (v0.1.20 bis v0.3.0)
- Server bereit fürs Internet, Zwei-Faktor-Anmeldung, Umbenennung in Hearth (v0.2.0), Sicherheitsblock nach der Client-Prüfung (v0.2.2), Lebenszeichen in Streams (v0.3.0).

### 2026-11-01 bis 07 (v0.1.12 bis v0.1.19)
- Gedächtnis, Fortschrittsliste, lange Dokumente und OCR, Kostendeckel, Versionen, Sicherheitsprüfung mit allen hohen und mittleren Funden behoben, Optionsmenü im Composer, fester OpenRouter-Anbieter, Chat-Tags.

### 2026-10 (Phasen 0 bis 4, v0.1.0 bis v0.1.11)
- Sandbox-Kern, Gateway, VM-Dienst, Merge-Prüfung, Web-Chat ↔ Umgebung, Recherche, Tray-App, Installer, App-Katalog, Dateien im Chat. Einzelheiten stehen in `CLAUDE.md` („Stand“) und im Git-Log.
