# Progress

_Last updated: 2026-11-26 · Offline lesen und Entwürfe je Chat in der Web-App; Direktmodus auf dem Linux-Laptop geprüft. Davor: Rechte des Direktmodus geschärft und sichtbar (Geheimnisse nie, Systemordner gesperrt, Jev bewertet Befehle, Chat-Freigaben zurücknehmbar; Protokoll 12), noch nicht veröffentlicht. Davor: Phase 6 (Direktmodus, Desktop-App, macOS und Linux): Client baut unter Linux und macOS, Direktmodus Ende zu Ende gebaut und unter Windows echt geprüft, Sandbox unter Linux in der CI geprüft, Desktop-App mit Netz und Programmen und im Startmenü. Standardmodell für neue Chats ist fertig. Release v0.4.0 mit Paketen für Windows, Linux und macOS._

## Where we are

Hearth ist ein selbst gehosteter Chat für LLMs (Server + Web-App) mit einem Windows-Client, der Coding-Agents (pi) in einer Podman-VM unter Hyper-V laufen lässt. Was es kann, steht in `README.md`. Wie es gebaut ist, steht in `ARCHITECTURE.md`. Die Stolpersteine je Bereich stehen in `CLAUDE.md`, und dort steht unter „Stand“ auch, was wann dazukam.

- **Server des Nutzers:** läuft auf seinem Heimserver hinter Nginx Proxy Manager mit HTTPS, also im Internet (Details nur in `CLAUDE.local.md`). Das Image `ghcr.io/johnnyjack123/hearth-server` baut `.github/workflows/server-image.yml` bei jedem Push auf `main`. Der Nutzer aktualisiert selbst: `docker compose pull`, dann `docker compose up -d --no-deps server`.
- **Client:** Release v0.4.0 (21.11.), unter Windows `hearth-setup-<arch>.exe` aus den GitHub-Releases, unter Linux und macOS `install.sh` (erstmals). Der Nutzer koppelt mit `hearth setup --server <adresse>`, Projekte meldet `hearth init`.
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
3. **Erledigt (21.11.): Kleine Client-Texte** (`pi.toml`-Kommentar, Hinweis in `hearth init`) sind in v0.4.0.
3a. **Erledigt (24./25.11.): Rechte des Direktmodus** (Wunsch des Nutzers): Geheimnisse nie lesbar (auch nicht per Befehl, der sie nennt), Systemordner, hearths Ordner und Startdateien gesperrt, Jev bewertet jeden Befehl, Freigaben „für diesen Chat“ sichtbar und zurücknehmbar (App, `hearth host revoke`, Browser), Übersicht „Was darf der Agent?“. Tunnel-Protokoll 12. Am 26.11. auf dem Linux-Laptop geprüft: Kern-Tests (24) und `smoke-host.sh` (40 Prüfungen) grün, App rendert offscreen wie unter Windows. Offen: Release 0.4.1 nach Rückfrage; der Server des Nutzers bekommt es mit dem nächsten `docker compose pull`.
3b. **Erledigt (26.11.): Offline lesen und Entwürfe** (Wünsche des Nutzers): Service Worker hält App und zuletzt geladene Chats vor (nur über HTTPS), ohne Server zeigt die App sie mit einem Hinweis und sperrt das Senden; der Text im Eingabefeld bleibt je Chat im Browser (Neuladen, Chatwechsel). Abmelden löscht beides. Die Desktop-App nennt die Sandbox unter Linux jetzt „Container“ statt „VM“. Offen: Rückmeldung des Nutzers auf dem Handy (PWA über HTTPS).
4. **Phase 6 (ROADMAP, Plan vom 21.11., Konzept `docs/security/concept.md`):**
   - ✅ Schritt 1: Unix-Build (Workflow `unix.yml` grün auf Linux x64/arm64 und macOS, Tests auf dem Linux-Laptop grün).
   - ✅ Schritt 2: Proxy-Kern `hearth-core/src/host/`.
   - ✅ Schritt 3: Tunnel-Protokoll 11, Server-Schleife, Web-Karten, `hearth project host`, `hearth host`. Geprüft: Server-Tests, Browser-E2E (25/25), `scripts/smoke-host.sh` unter Windows (26/26). Auf dem Linux-Laptop mit dem Paket aus v0.4.0 ebenfalls 26/26 (21.11. abends; Server und Mock unter Windows, `ssh -R 8097:127.0.0.1:8097 linux-laptop`, dort `SERVER_URL=http://127.0.0.1:8097 SERVER_COOKIES=… CREDENTIAL=… HEARTH_BIN=… bash scripts/smoke-host.sh`).
   - Schritt 4: Benachrichtigung und Freigabe-Dialog am Rechner (heute nur `hearth host` im Terminal), geht in Schritt 5 auf.
   - 🔨 Schritt 5: Desktop-App `hearth-app` (egui): Freigaben, Projekte mit Modus, Netzregeln und Programmen der Sandbox, Rechner, Protokoll; Tray (Windows) öffnet sie und meldet Freigaben; `hearth setup` trägt sie ins Startmenü ein (Windows `Hearth.lnk`, Linux `.desktop`). Offen: Tray unter macOS und Linux (Linux: StatusNotifierItem, z. B. `ksni`; macOS: `NSStatusItem`), beides ohne echtes Testgerät hier.
   - 🔨 Schritt 7 vorgezogen: Pakete für Linux (x64, arm64, OpenSSL eingebaut) und macOS (Universal-App, ad hoc signiert) mit `scripts/install.sh` und Workflow `unix-release.yml`; Linux/macOS: `hearth setup`, `hearth doctor`, Token-Datei, Benachrichtigungen ohne Tray. Anleitung `docs/client-unix.md`.
   - 🔨 Schritt 6: Sandbox unter Linux (rootless Podman ohne VM) und macOS (Podman-Machine, `hearth setup` legt sie an) im Provider; Instanzen ohne DNS (Fund: Podman 4 löste im internen Netz externe Namen auf). Geprüft: Provider-Test in der Windows-VM; unter Ubuntu in der CI (Job `sandbox` in `unix.yml`) grün: Provider, Images (Base und Gateway im lokalen Podman gebaut), Git-Sync, Gateway mit Rückfrage und TLS, Instanz mit Installation über das Gateway. Unter Linux erreicht die Sandbox keinen Server unter `localhost` (kein gvproxy), dokumentiert. Offen: macOS-Sandbox ungeprüft (die CI-Runner haben keine verschachtelte Virtualisierung); zusätzliche Runtime unter Linux (gVisor/krun, ARCHITECTURE §3); die Images in der VM des Nutzers sind für das neue Netz-Layout veraltet (erst neu bauen, wenn er zustimmt, siehe Stolpersteine „TLS im Gateway“ zum Sichern seines Stands; beim nächsten Update baut `hearth setup` sie ohnehin neu).
   - Offen außerdem: Desktop-App und Sandbox auf dem Linux-Laptop (dort ist kein Podman installiert), Rückmeldung des Nutzers zu v0.4.0 (erstes Release mit Linux- und macOS-Paketen, 21.11.); Korrekturen kommen als 0.4.x.
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
- **Release v0.4.0 sofort** (Nutzer, 21.11. abends): Er testet wie immer mit dem Release, Korrekturen kommen als 0.4.x.

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
- **Niedrig:** Antwortet SSH nach dem VM-Start nicht rechtzeitig (podman: „ssh: handshake failed … forcibly closed“), zeigt `hearth setup` nur podmans Text. Beim Nutzer am 21.11. nach dem Update auf 0.4.0: erst 32778 (1,35 GB frei), dann bekam die VM 1,4 GB, das Gastsystem lief nach 7 s, SSH war nach 88 s noch nicht erreichbar; Windows hatte 2,4 GB frei, Commit 27 von 31,6 GB. Kandidat für 0.4.1: Hinweis auf freien Speicher wie bei 32778, in der CLI (dann braucht der VM-Dienst kein Update).
- **Mittel:** Markdown-Bilder könnten Daten hinaustragen; das blockt nur die CSP der Produktion (der Vite-Dev-Server sendet keine).
- **Mittel:** Brave, Tavily, Jev und die Embeddings sind in Tests nur gegen Stand-ins geprüft.
- **Niedrig:** Über `http://` im LAN gibt es keine Web Locks, dann hält jeder Tab eine eigene Live-Verbindung (HTTP/1.1: höchstens sechs je Server).
- **Niedrig:** Fund 8 und 10 der Client-Prüfung sind teilweise offen (`docs/security/review-2026-11-10-client.md`).
- Die Images `hearth-server` und `hearth-markitdown` sind seit 17.11. öffentlich (anonym geprüft). Die alten `llm-harness-server` (versehentlich öffentlich) und `llm-harness-markitdown` aus der Zeit vor der Umbenennung kann der Nutzer löschen; die alte Pi-Instanz hat sie noch lokal.
- **Niedrig:** Der LLM-Durchgang (`/api/llm/`) sendet absichtlich keine Lebenszeichen. Schweigt ein Anbieter länger als `proxy_read_timeout` (nginx: 60 s), bricht der Proxy die Antwort ab.
- **Rechner des Entwicklers:** Bei knappem Speicher scheitert der Browser-E2E mit `ERR_INSUFFICIENT_RESOURCES`; dann einfach noch einmal laufen lassen. Windows Defender hält `hearth.exe` manchmal für einen Trojaner (Fehlalarm des maschinellen Lernens). Windows Sandbox nie ungefragt starten.

## Verification state

Stand 26.11. (Branch `feat/offline-drafts`, danach `main`):
- `pnpm --filter @hearth/web typecheck`, Browser-E2E (Dev-Modus, alle Schritte) grün.
- Neu: `e2e:offline` gegen den Produktions-Build: chatten, Entwurf übersteht Neuladen und Chatwechsel, Service Worker übernimmt, Server gestoppt → Chat, Liste und Entwurf da, Band „Keine Verbindung“, Senden gesperrt; Server wieder da → Band weg; Abmelden löscht Chats im Cache, Entwürfe und gemerktes Konto.
- Linux-Laptop: `cargo test -p hearth-core --lib host::` (24), `cargo test -p hearth-app` mit Bildern, `smoke-host.sh` (40 Prüfungen) über einen SSH-Rückwärtstunnel gegen Server und Mock unter Windows.
- Nicht geprüft: Offline auf echten Handys, Sandbox auf dem Laptop (kein Podman installiert).

Stand 25.11. (Branch `feat/host-rights`, danach `main`):
- `pnpm -r typecheck`, Server-Tests (329) und Browser-E2E (Direktmodus-Schritt mit Jev-Plakette, Rechte-Liste und Zurücknehmen) grün.
- `scripts/smoke-host.sh` unter Windows grün (40 Prüfungen, neu: `.env` per Werkzeug und per Befehl abgelehnt, Chat-Freigabe am Rechner und im Browser sichtbar und aus dem Browser zurückgenommen).
- Rust: `cargo fmt --check`, `cargo clippy --all-targets` ohne Warnungen, Unit-Tests von `hearth-core` (`host::`, `tunnel::`) und `hearth-app` grün, App-Bilder angesehen.
- Nicht geprüft: Linux-Laptop, echter Jev (nur der Stand-in), echte Freigaben mit dem Nutzer.

Stand 21.11. abends (Branch `feat/unix-sandbox`, danach `main`):
- `pnpm -r typecheck`, Server-Tests (325) und Browser-E2E (25/25) nach dem Direktmodus grün; seitdem kein Server- oder Web-Code geändert.
- `scripts/smoke-host.sh` unter Windows 26/26.
- Rust unter Windows: `cargo fmt --check`, `cargo clippy --all-targets` ohne Warnungen, Unit-Tests grün; `podman_vm` in der Windows-VM mit dem neuen Netz-Layout grün. `gateway_vm` und `instance_vm` unter Windows nicht gelaufen (dafür müssten die Images in der VM des Nutzers neu gebaut werden).
- CI `unix.yml`: fmt, clippy, Tests auf Ubuntu x64/arm64 und macOS; Job `sandbox` (Ubuntu 24.04, rootless Podman) mit Provider, Image-Bau, `sync_vm`, `gateway_vm`, `instance_vm` grün.
- `unix-release.yml` als Probelauf grün (Pakete gebaut, nichts veröffentlicht).
- Desktop-App: kittest-Tests mit Bildern angesehen, Speicher gemessen (glow 78 MB).
- Startmenü-Eintrag: PowerShell-Teil gegen eine Testdatei geprüft (anlegen, unverändert lassen, Ziel ändern); `hearth setup` selbst nicht mit dem echten Startmenü laufen lassen.
- v0.4.0: alle Release-Workflows grün, 8 Dateien im Release. Auf dem Linux-Laptop (CachyOS, x86_64): `install.sh` mit dem Archiv aus dem Release, `hearth 0.4.0` startet, OpenSSL eingebaut, `hearth-app` findet alle Bibliotheken, `smoke-host.sh` 26/26 mit dem Release-Programm.
- Nicht geprüft: Desktop-App auf einem echten Linux-Desktop (nur die Bibliotheken), Sandbox auf dem Laptop, macOS-Sandbox, Tray unter Linux/macOS (gibt es noch nicht), echte Handys.

---

## Log

### 2026-11-26
- Offline lesen (Service Worker, `e2e:offline`) und Entwürfe je Chat. Direktmodus auf dem Linux-Laptop geprüft; die App heißt die Linux-Sandbox jetzt „Container“.

### 2026-11-24/25
- Rechte des Direktmodus (Wunsch des Nutzers): Geheimnisse nie, Systemordner, hearths Ordner, Startdateien; Befehle mit Geheimnis-Namen abgelehnt; Jev bewertet jeden Befehl; Chat-Freigaben mit IDs, sichtbar und zurücknehmbar; Übersicht in App, Popover, CLI; Tunnel-Protokoll 12.

### 2026-11-21
- Standardmodell für neue Chats (Einstellungen → Chat).
- Phase 6 geplant und begonnen: Sicherheitskonzept, Client baut unter Linux und macOS (CI), Proxy-Kern, Tunnel-Protokoll 11 mit Direktmodus in Server, Web und CLI, Smoke-Test `smoke-host.sh`.
- Desktop-App `hearth-app` (egui), Pakete für Linux und macOS (`install.sh`, `unix-release.yml`), Sandbox unter Linux und macOS im Provider, Instanzen ohne DNS (Sicherheitsfund unter Podman 4), Sandbox-Job in `unix.yml`, Startmenü-Eintrag unter Windows.
- Release v0.4.0 (Wunsch des Nutzers). `target/` mit `cargo clean` von 36,7 GB befreit; die alte VM `harness` (8,9 GB) und der Dienst `llm-harness-vmd` stehen beim Nutzer noch, Befehle zum Entfernen hat er bekommen.

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
