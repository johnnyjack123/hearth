# Roadmap

> Stand 14.10.2026. Architektur und Begründungen: [`ARCHITECTURE.md`](../ARCHITECTURE.md). Aktueller Arbeitsstand: [`PROGRESS.md`](PROGRESS.md).
> Grundsatz: **ein** Sandbox-Provider (Podman + Hyper-V + Linux-VM, Isoliert-Modus), bis er im Alltag trägt. Weitere Provider kommen erst danach.

| Phase | Inhalt | Status |
|---|---|---|
| 0 | Spike Podman WSL und Hyper-V (I/O, Netz, Sicherheit) | ✅ erledigt, [`spikes/phase-0.md`](spikes/phase-0.md) |
| 0b | Spike Windows Sandbox | ✅ abgebrochen und verworfen, [`spikes/phase-0b-windows-sandbox.md`](spikes/phase-0b-windows-sandbox.md) |
| 1 | Server + Web-Chat (Multi-User, Keys pro Nutzer, Streaming) | ✅ im Kern fertig, Reste siehe unten |
| **2** | **Sandbox-Kern + `harness`-CLI (Terminal-Modus mit pi)** | **✅ im Kern fertig**: Pakete 1–10, Rückfragen im selben Terminal, Hinweise an den Agent. Vom Nutzer in Windows Terminal geprüft (16.10.) |
| 2b | Windows-Dienst `harness-vmd` (VM-Lebenszyklus ohne Admin-Fenster) | ✅ beim Nutzer eingerichtet (17.10.); offen: Stopp im Leerlauf |
| 2c | Merge-Prüfung: Diff des Agents vor `harness merge` auf Verdächtiges prüfen | 🔨 Stufe 1 (feste Regeln) fertig, Stufen 2 und 3 offen |
| 2d | Prompt-Injection: Hinweis im System-Prompt, Warnungen bei verdächtigen Inhalten | ✅ Hinweis und Warnungen (Muster); offen: Jev als Klassifikator |
| 3 | Desktop-App (Tauri) + UI für Netz- und Git-Regeln und Rückfragen | offen |
| 4 | Chat ↔ Umgebung (Pairing, Tunnel, pi-RPC-Adapter, LLM-Gateway) | offen |
| 5 | Ausbau (Host-Modus, Sync-Modus, weitere Harnesse und Provider …) | offen |

## Phase 1 – Reste

- [x] Server-Image (`apps/server/Dockerfile`, amd64 und arm64) mit `docker-compose.yml` und `.env`; GitHub baut und testet es bei jedem Push auf `main` (17.10., `docs/deploy.md`).
- [ ] ESLint- und Prettier-Konfiguration anlegen. Die Pakete sind installiert, es gibt aber keine Config-Dateien. Danach `pnpm lint` grün machen.
- [x] `README.md` mit Quick Start für Server (inkl. `.env`) und Sandbox (17.10.).
- [x] Postgres-Testlauf wiederholt (14.10.): 64/64 grün, SQLite und Postgres, inklusive Geräte und LLM-Durchgang. Die Portweiterleitung über gvproxy funktioniert unter Hyper-V.
- [ ] Bundle-Größe der Web-App: Haupt-Chunk ~955 kB (295 kB gzip), per `manualChunks` aufteilen.
- [x] Branch `feat/phase-1-chat` nach `main` gemergt (12.10.).

## Phase 2 – Sandbox-Kern und `harness`-CLI

**Ziel:** Im Ordner eines Git-Repos auf dem Host `harness run pi` aufrufen. pi läuft dann in einem Container in der Linux-VM, auf einer Arbeitskopie im Volume, mit Netz nur über das Gateway. Danach holt `harness sync` die Commits als Branch `agent/<projekt>` ins Host-Repo, und `harness merge` führt sie automatisch zusammen, wenn es konfliktfrei geht.

**Abnahme:** In einem Test-Repo legt `harness run pi` die Instanz an und startet pi interaktiv. pi committet eine Änderung, `harness sync` und `harness merge` bringen sie konfliktfrei in den Host-Branch. Ein `curl example.com` im Container wird vom Gateway geblockt, `npm view react version` geht durch. Alles läuft **ohne** Admin-Rechte, solange die VM läuft.

Arbeitspakete in sinnvoller Reihenfolge:

1. ✅ **Cargo-Workspace:** `Cargo.toml` im Repo-Root, `crates/harness-core` (Bibliothek) und `crates/harness-cli` (Binary `harness`). Toolchain `aarch64-pc-windows-msvc` ist installiert. RAM ist knapp, deshalb `jobs = 2` in `.cargo/config.toml` und `debug = "line-tables-only"`. Im Spike stürzte rustc bei Release-Builds ab, vermutlich wegen RAM. Der Debug-Build braucht ~22 s.
2. ✅ **Provider-Schnittstelle** in `harness-core` (`provider.rs`), zunächst nur `PodmanProvider` (`podman/`). Verifiziert mit `tests/podman_vm.rs` gegen die echte VM. **Offen:** exec mit TTY ist gebaut, aber noch nicht interaktiv getestet. Das muss der Nutzer mit `harness shell` und `harness run pi` in Windows Terminal ausprobieren.
   - ruft die `podman`-CLI mit expliziter SSH-Verbindung auf, nach dem Muster von `spikes/phase-0/pm.sh`
   - eigene `known_hosts`, die des Nutzers **nicht** anfassen
   - Port und Key aus `%USERPROFILE%\.config\containers\podman\machine\hyperv\harness.json` lesen (`SSH.Port`, `SSH.IdentityPath`)
   - Methoden: VM erreichbar?, Image bauen, Volume, Netz, Container anlegen, starten und stoppen, exec (mit TTY), Dateien in den Container und heraus (`copy_into`, `copy_out`)
3. ✅ **Base-Image** `images/base/Containerfile` (Debian trixie-slim, Node 24 aus dem offiziellen Image, uv 0.12.24, ~680 MB). Der Kontext ist ins Binary eingebettet, gebaut wird mit `harness image build`. Verifiziert mit `tests/base_image_vm.rs`, dazu pnpm und Proxy-Verhalten von Node von Hand:
   - Debian stable slim, multi-arch
   - git, ca-certificates, curl, build-essential, Python 3 + uv, Node LTS + pnpm (corepack), ripgrep, fd, jq
   - Nutzer `agent` (UID 1000)
   - `/etc/gitconfig` mit `core.checkStat=minimal`, `core.trustctime=false`, `safe.directory=*` für `/workspace`
   - `HTTP(S)_PROXY` zeigt auf das Gateway

   Gebaut wird per `podman build` remote, der Kontext wird vom Host hochgeladen.
4. ✅ **Konfiguration** unter `%USERPROFILE%\.harness\` (oder `HARNESS_HOME`) als TOML (`config.toml`, `projects/<name>.toml`, `harnesses/<name>.toml`), Modul `harness-core/src/config/`, Vorlagen in `harness-core/defaults/`. Das Format folgt ARCHITECTURE.md §3.8, aber mit `mode = "isolated"`. Gelesen wird mit `serde`, geschrieben mit `toml_edit`, damit Kommentare erhalten bleiben. **Sicherheitsrelevantes nie im Projektordner:** Projekte, die den Konfigurationsordner enthalten oder in ihm liegen, werden abgelehnt, beim Anlegen und bei jedem Laden.
   - Unbekannte Schlüssel sind ein Fehler, damit Tippfehler wie `[netwrok]` nicht still auf weniger strenge Standards zurückfallen.
   - Jeder Schreibvorgang wird vorher geparst und validiert und dann atomar geschrieben.
   - Projektname: aus dem Ordnernamen abgeleitet (`[a-z0-9-]`, ≤ 40 Zeichen, Umlaute transliteriert, bei Kollision `-2` usw.). Daraus folgen Container `harness-<name>`, Netz `harness-net-<name>`, Volumes `harness-ws-<name>`/`harness-home-<name>`, Branch `agent/<name>` (`project.rs`).
   - CLI: `harness project [show|add|list]`. `doctor` prüft die Konfiguration mit. Die Machine kommt aus `config.toml`, `--machine` überschreibt sie.
   - Domain-Muster (`api.example.com`, `*.example.com`, keine IP-Literale) werden schon beim Laden geprüft. `DomainPattern::matches` ist für das Gateway vorbereitet.
5. ✅ **Git-Sync im Isoliert-Modus** (`harness-core/src/sync/`) mit Git-Bundles. Bis zum 16.10. lief das über einen 9p-Austauschordner. Seitdem kopiert harness die Bundles per `podman cp` bzw. `podman exec cat` in die Instanz und zurück. Die VM sieht dadurch keinen Host-Ordner mehr, das ist Voraussetzung für Phase 2b. Verifiziert mit 13 Unit-Tests gegen temporäre Repos und `tests/sync_vm.rs` mit den echten Skripten in der VM (Klonen ~0,8 s, Rückweg ~1,1 s):
   - **Host → VM** (`push_to_workspace`): `git bundle create <temp>/in.bundle HEAD --branches --tags`, dann `podman cp` nach `~/.harness-sync-in.bundle`. Bewusst nicht `--all`, damit Stash und Remote-Tracking-Refs auf dem Host bleiben. Im Container beim ersten Mal `git clone --origin host`, danach `git fetch host`. Der Agent-Branch setzt den Agent-Branch des Hosts fort, falls es ihn gibt, sonst beginnt er am HEAD des Hosts.
   - **VM → Host** (`pull_from_workspace`): Der Container bündelt nur `agent/<projekt> --not --remotes=host`, also nur, was dem Host fehlt. Fehlen dem Host die Voraussetzungen (Kopf selbst gelesen, `BundleHeader`), folgt automatisch ein vollständiges Bundle. Der Host holt die Objekte und setzt den Branch per Compare-and-swap (`update-ref`), nur als Fast-Forward (sonst `--force`) und nie, solange der Branch ausgecheckt ist.
   - **Merge** (`HostRepo::merge`): Fast-Forward, falls möglich, sonst `git merge --no-edit`. Abgelehnt werden detached HEAD, laufende Merges oder Rebases und uncommittete Änderungen an getrackten Dateien. Konflikte erkennt vorher `git merge-tree --write-tree` trocken. Bei Konflikt bleibt alles unangetastet, und die Dateien werden gemeldet.
   - **Härtung auf dem Host:** Jeder Git-Aufruf läuft mit `core.hooksPath` auf einen leeren Ordner, `core.fsmonitor=false`, `submodule.recurse=false` und `fetch.fsckObjects`/`transfer.fsckObjects`. Ein Test beweist, dass weder `.git/hooks` noch ein Hook-Ordner im Arbeitsbaum (husky) ausgeführt werden. Container-Ausgaben werden vor der Anzeige von Steuerzeichen befreit.
   - **Offen:** `in.bundle` ist jedes Mal vollständig, bei großen Repos ggf. inkrementell machen.
6. ✅ **Netzwerk-Gateway** (`crates/harness-gateway`, Image `images/gateway`, Host-Seite `harness-core/src/gateway.rs`, CLI `harness net`). Statt tinyproxy gleich ein eigener kleiner Proxy in Rust (aus Phase 3 vorgezogen): Nur er kann „open“ sicher machen und Anfragen festhalten, bis der Nutzer entscheidet. Nur Standardbibliothek außer TLS zum Server (rustls, Feature `tls`), ein Thread pro Verbindung. Gebaut wird in der VM aus den eingebetteten Quellen (`rust:1.97-slim-trixie`) mit dem gepinnten `images/gateway/Cargo.lock` (Crates von crates.io, ~34 s). Verifiziert mit 28 Tests im Crate (6 davon über echtes TCP), `tests/gateway_vm.rs` und einem Probelauf der CLI mit `harness net watch`:
   - **Ein Gateway pro Projekt** (`harness-gw-<projekt>`), im internen Projektnetz unter dem Alias `gateway` (Port 8888) und zusätzlich im normalen Netz `harness-egress`. Es hat eine eigene Provider-Methode `create_gateway`: gehärtet wie Instanzen, Nutzer `nobody`, keine Mounts, 128 MB. Sie prüft, dass das erste Netz intern ist und das zweite nicht. Instanzen bleiben **nur** im internen Netz.
   - **Entscheidung pro Host**, in dieser Reihenfolge:
     1. Die deny-Liste gewinnt immer.
     2. Modus `open` erlaubt jeden Host.
     3. Sonst gelten die allow-Liste und Freigaben zur Laufzeit.
     4. Bei `ask = true` wird nachgefragt, falls jemand zuschaut.
     5. Sonst kommt 403 mit Begründung, auch im Header `X-Harness-Gateway`.
   - **Ziel-IP-Prüfung in jedem Modus:** Das Gateway löst Namen selbst auf und verbindet sich nur mit öffentlichen Adressen (`address.rs`). Gesperrt sind:
     - Loopback und RFC 1918, inklusive gvproxy `192.168.127.0/24`
     - CGNAT (Tailscale) sowie Link-Local inklusive Cloud-Metadaten
     - Multicast
     - IPv6 ULA und Link-Local
     - IPv4-mapped, NAT64 und 6to4, wenn sie auf eine private Adresse zeigen

     Numerische Sonderformen wie `127.1` oder `0x7f000001` sind als Host ungültig.
   - **Voreinstellungen:** `off` = nur `llm_apis` aus `config.toml` (Standard `api.deepseek.com`). `development` = Paketquellen (npm, PyPI, crates.io, Go), GitHub, GitLab, Bitbucket und die allow-Liste. `open` = das ganze öffentliche Internet. `custom` = nur die allow-Liste. Die LLM-APIs sind in jeder Voreinstellung erreichbar, weil keine Harness ohne Modell arbeitet.
   - **Nachfragen wie bei Claude Code:** Bei `ask = true` (Standard) wartet eine Anfrage an einen unbekannten Host bis zu 2 Minuten, solange `harness net watch` läuft. Gleichzeitige Anfragen an denselben Host teilen sich eine Frage. Antworten:
     - einmal
     - 15 Minuten
     - bis das Gateway stoppt
     - immer (schreibt die Domain in die allow-Liste der Projektdatei)
     - nein (gesperrt bis zum Stopp)

     Ohne Zuschauer wird sofort abgelehnt, mit dem Hinweis auf `harness net allow <host>`.
   - **Steuerung** nur über `podman exec harness-gw-<projekt> harness-gateway ctl` zu einem Unix-Socket im Gateway-Container (Zeilenprotokoll, `control.rs`). Instanzen erreichen nur Port 8888. `watch` ist ein dauerhaftes `podman exec -i`, pingt alle 15 s und gilt nach 45 s Stille als beendet.
   - **CLI:** `harness net [status|watch|allow|deny|preset|start|stop]` und `harness image build [base|gateway]`. `doctor` prüft beide Images.
   - Geprüft: corepack holt pnpm von `registry.npmjs.org`, GitHub ist dafür nicht nötig.
   - `harness run`/`shell` starten das Gateway mit, `harness stop` stoppt es (Paket 8).
7. ✅ **Harness pi und LLM-Zugang:**
   - ✅ **LLM-Zugang über den Server** (Entscheidung des Nutzers am 13.10.: Keys nur im Server, auch für Agent-Läufe aus dem Terminal). Damit sind Teile von Phase 4 vorgezogen:
     - **Kopplung:** `harness login` startet einen Device-Flow (`/api/pairing`). Der Nutzer bestätigt den Code auf `/device` der Web-App. Das Token liegt in der Windows-Anmeldeinformationsverwaltung (`keyring`). Geräte stehen in den Einstellungen unter „Geräte“ und lassen sich dort widerrufen, ebenso mit `harness logout`.
     - **Durchgang im Server** `/api/llm/proxy/<Anbieter-ID>/…`: Anfragen gehen 1:1 im Protokoll des Anbieters durch, nur an die Chat-Endpunkte. Der Key wird je nach Anbieter gesetzt (Bearer, `x-api-key`, `x-goog-api-key`). `/api/llm/providers` liefert die Anbieter ohne Keys.
     - **Gateway:** Die virtuelle Adresse `http://llm.harness/…` führt zum Server. Das Gateway verwirft Zugangsdaten des Clients und setzt das Geräte-Token ein, das der Host per stdin übergibt. Der lokale Server ist über `192.168.127.254` erreichbar, als einzige Ausnahme vom Adressfilter.
     - Verifiziert:
       - Server-Tests 64/64 (SQLite und Postgres)
       - E2E 9/9, mit Kopplung im Browser
       - `gateway_vm` mit `llm.harness` bis zu einem Test-Server auf Windows
       - Probelauf: `harness login`, dann `net start`, dann `curl http://llm.harness/…` im Container, Antwort des Mock-LLM über den echten Server, dann `logout`
   - ✅ **pi in der Instanz** (`instance.rs`, `pi.rs`):
     - **Installation:** pi kommt per `npm install -g @earendil-works/pi-coding-agent` in das gemeinsame Tools-Volume `harness-tools`. Das läuft im kurzlebigen Container `harness-install-<projekt>`, nur dort ist das Volume beschreibbar, Instanzen lesen es nur. Erlauben die Regeln des Projekts npm nicht, bekommt der Installer eine Freigabe für 15 Minuten.
     - **`~/.pi`** liegt im Volume `harness-cfg-pi`, das alle Projekte teilen.
     - **`models.json`** entsteht bei jedem `harness run` aus `/api/llm/providers`:
       - Eingebaute Anbieter von pi (`deepseek`, `openai`, `anthropic`, `google`, `openrouter`) bekommen nur `baseUrl: http://llm.harness/<id>` und den Dummy-Key `harness`. So bleiben pis Modelldaten erhalten.
       - Eigene Endpunkte (Ollama, OpenAI-kompatibel) werden eigene Anbieter `harness-<name>` mit der Modellliste vom Server.
       - harness ersetzt nur seine eigenen Einträge, erkennbar an `llm.harness` mit Dummy-Key. Einträge des Nutzers bleiben und haben bei gleicher ID Vorrang.
       - Nach `harness logout` entfernt `harness run` die Umleitung, und pi nutzt wieder seine eigene Anmeldung.
     - **Gateway:** Es nimmt `CONNECT llm.harness:80` an, weil undici auch `http://` tunnelt.
     - **Offen:** Mistral. pi spricht dort `mistral-conversations`, das der Server-Durchgang nicht erlaubt.
8. ✅ **CLI-Befehle** (`harness-cli/src/run.rs`):
   - `run [harness] [-- args]`: Gateway, Instanz, Klon bzw. Fetch, Installation, `models.json`, dann pi
   - `shell`: Login-Shell in der Instanz
   - `sync [--force]`, `merge`, `stop`, `status`

   Ein Terminal wird nur durchgereicht, wenn stdin und stdout eines sind. Der Nutzer hat `harness run pi` am 15.10. in Windows Terminal mit DeepSeek genutzt.
   - ✅ **Rückfragen im selben Terminal** (`harness-cli/src/terminal/`, `harness-core/src/pty.rs`): podman läuft auf einer eigenen Pseudo-Konsole (ConPTY), die harness ans Terminal weiterreicht. Fragt das Gateway, hält harness die Ausgabe an, zeigt die Frage (e/m/s/i/n, Esc lehnt ab) und gibt das Terminal danach zurück. Das funktioniert auch über pis Vollbild-Oberfläche. Ohne Terminal oder mit `HARNESS_DIRECT_TERMINAL=1` bleibt es beim direkten Anschluss und `harness net watch`.
   - ✅ **Hinweise an den Agent** (`briefing.rs`, Wunsch des Nutzers vom 15.10.): Sie erklären Sandbox, Arbeitskopie, Commits und Netzregeln und hängen über `prompt_flag` an den System-Prompt der Harness. Der Nutzer muss das nicht in jeder Sitzung selbst erklären.
   - pi telefoniert nicht mehr nach Hause (`[env]` in `pi.toml`), sonst käme bei jedem Start eine Rückfrage zu `pi.dev`.
   - **Offen:** Rückmeldung des Nutzers zum Relay in Windows Terminal (Maus, Einfügen, Farben). Im simulierten Terminal sind Größe, Strg+C, Rückfragen und pi geprüft.
9. ✅ **Tests:**
   - Unit-Tests: Konfiguration, Bundle- und Merge-Logik gegen temporäre Repos, Domain-Muster, Gateway, `models.json`, Pseudo-Konsole, Tasten und Bildschirmzustand
   - VM-Tests: `podman_vm`, `base_image_vm`, `sync_vm`, `gateway_vm`, `instance_vm`, dazu `harness-cli/tests/terminal_vm.rs` (`harness shell` und `harness run pi` in einem simulierten Terminal)
   - Smoke-Skript `scripts/smoke-run.sh`, siehe CLAUDE.md
10. ✅ **Doku:** `CLAUDE.md` (Befehle, Stolpersteine) und `PROGRESS.md`.

## Phase 2b – `harness-vmd`

Anforderungen stehen in `spikes/phase-0.md` §12.3:
- Konfiguration in `%ProgramData%`
- Named Pipe nur für den eingerichteten Nutzer
- dynamischer Speicher mit kleinem Startwert
- Aufräumen nach gescheitertem Start (Ignition-KVP, vsock-Registry)
- Watchdog nach Standby

Der Installer richtet den Dienst einmal mit UAC ein.

**Stand 17.10.:** gebaut und vom Nutzer eingerichtet. Die VM startete über den Dienst in 22 s, vorher entfernte er 4 Ignition-Reste. gvproxy läuft seitdem als SYSTEM, `harness` startet aus dem normalen Terminal. Details und Sicherheitsüberlegungen in CLAUDE.md („VM-Dienst“).
- `crates/harness-vmd`: Windows-Dienst als SYSTEM. Pipe mit Status, Start, Stopp und Neustart. podman läuft dort in einer eigenen, nur für Admins beschreibbaren Umgebung.
- **Vorbereitung:** Die VM sieht seit dem 16.10. keinen Host-Ordner mehr. Sonst liefe der 9p-Dateiserver als SYSTEM und würde Ordner servieren, die ein Nutzerprozess umbiegen kann.
- `harness vmd install` (einmal UAC) übernimmt die bestehende Machine samt Images und Volumes. `uninstall` gibt sie zurück.
- **Automatischer Start:** `harness`, `run`, `shell`, `net` und `image build` starten die VM selbst, wenn SSH nicht antwortet. Antwortet SSH bei laufender VM nicht, startet der Dienst sie neu.
- `harness` ohne Argumente startet die Standard-Harness und legt beim ersten Mal nach Rückfrage Repository und Projekt an.
- **Offen:**
  - Stopp im Leerlauf (Leases von `harness run`, später `harnessd`)
  - Wiederanlauf nach Standby über den Leerlauf-Check hinaus
  - fehlende vsock-Registry-Einträge anlegen

**Wunsch des Nutzers (15.10.):** Im Ordner nur noch `harness` sagen, den Rest erledigt harness im Hintergrund. Dazu:
- `harness` ohne Argumente startet die Standard-Harness.
- Die VM startet bei Bedarf über `harness-vmd` und stoppt nach einer Leerlaufzeit.
- Ist der Ordner noch kein Projekt, fragt harness einmal nach und legt es an. Ohne Git-Repository bietet es `git init` an.
- Nicht gekoppelt: Hinweis auf `harness login`.

## Phase 2c – Merge-Prüfung

**Wunsch des Nutzers (16.10.):** Bei `harness merge` läuft automatisch eine Prüfung über den Diff des Agents. Sie sucht nach Verdächtigem, auch nach verschachtelten Tricks, und zeigt die Funde an. Hintergrund: Der realistischste Weg aus der Sandbox ist Code, den der Nutzer übernimmt und dann auf Windows ausführt.

Fertig gibt es das für Agent-Diffs nicht, wohl aber Bausteine:
- GuardDog (Datadog, Semgrep- und YARA-Regeln für bösartige npm- und PyPI-Pakete)
- Semgrep
- OSV mit den `MAL-`-Einträgen für bekannte Schadpakete
- gitleaks bzw. trufflehog für Secrets
- YARA/ClamAV und Microsoft Defender (`MpCmdRun -Scan -ScanType 3 -File …`) für Binärdateien
- Socket als kommerzieller Dienst mit KI-Prüfung von Paketen

Geplant in drei Stufen:
1. **Feste Regeln in harness** (schnell, nicht zu täuschen, wichtigste Stufe):
   - **Stellen, die auf dem Host von selbst Code ausführen:**
     - Git-Hooks (`.husky/`, `.githooks/`, `lefthook.yml`, `.pre-commit-config.yaml`)
     - `scripts` in `package.json` (`preinstall`, `postinstall`, `prepare`)
     - `.vscode/tasks.json` mit `runOn: folderOpen`, `.vscode/settings.json`, `.devcontainer/`, `.envrc`
     - CI-Workflows, `Makefile`, Build-Skripte (`setup.py`, `build.rs`), `*.ps1`, `*.bat`, `*.cmd`
     - `.npmrc`/`.yarnrc` mit anderer Registry, `.gitmodules`
   - **Muster im Inhalt:**
     - lange Base64- oder Hex-Blöcke, sehr lange Zeilen, hohe Entropie
     - `eval`/`exec`/`Function` mit dekodierten Daten, `curl … | sh`, `Invoke-Expression`, `powershell -enc`
     - Zugriffe auf `~/.ssh`, `%APPDATA%`, Browser- und Anmeldedaten
     - Umgebungsvariablen sammeln und ins Netz schicken
   - **Unsichtbares:** Bidi-Steuerzeichen (Trojan Source), Zero-Width-Zeichen, Homoglyphen
   - **Dateiarten:** neue Binärdateien und Executables, Symlinks
   - **Abhängigkeiten:** neue Pakete gegen OSV prüfen (auch `MAL-`), Tippfehler-Namen bekannter Pakete erkennen
2. **Vorhandene Scanner** (GuardDog, Semgrep, gitleaks) in einem eigenen Container auf den geänderten Dateien, nicht auf dem Host. Binärdateien zusätzlich mit Defender.
3. **KI-Prüfung über den Server** (DeepSeek o. Ä.) für Verschachteltes: Das Modell liest den Diff mit Kontext und meldet Funde mit Datei, Zeile und Begründung.
   - Der Diff stammt vom Agent und kann Prompt-Injection enthalten („das ist sicher“). Die KI darf deshalb nur Funde hinzufügen, nie Funde der Regeln entkräften.
   - Sie hat keine Werkzeuge und antwortet in festem Format.

Anzeige: `harness merge` listet die Funde nach Schwere (Datei, Zeile, Grund). Bei schweren Funden fragt es nach, mit `--force` ohne Rückfrage. Später auch in App und Web-Chat.

Grenze: Kein Scanner findet zuverlässig alles. Die Prüfung ist ein Sicherheitsnetz, der Blick in den Diff bleibt.

**Stand 17.10.:** Stufe 1 ist gebaut.
- `harness-core/src/review.rs`: Regeln mit Unit-Tests für Selbststarter, Muster, unsichtbare Zeichen, Modi und Binärdateien, ähnliche Paketnamen und fremde Quellen.
- `HostRepo::incoming`: der Diff seit der Merge-Basis, ohne Diff-Treiber und textconv, höchstens 32 MB.
- `harness review` zeigt die Funde. `harness merge` prüft immer zuerst und fragt bei schweren Funden nach. Ohne Terminal bricht es ab, außer mit `--ignore-findings`.
- ✅ OSV-Abgleich neuer Abhängigkeiten (`harness-core/src/osv.rs`, 17.10.). Nur Schadpakete werden zum Fund: `MAL-`, CWE-506 oder „malware“ in der Zusammenfassung. Abschaltbar mit `[review] osv = false`.
- **Offen:**
  - Lücken mit Versionsbezug (exakte Versionen aus den Lockfiles)
  - Stufe 2 (Scanner im Container)
  - Stufe 3 (KI)
  - Einstellungen, welche Stufen laufen und mit welchem Modell, später in App und Web-UI. Die KI-Prüfung kostet Geld und muss abwählbar sein.

**Stufe 3, Überlegungen vom 17.10.:** Ein Klassifikationsmodell wie Jev (TypeSafe; über pi auch per OpenRouter, Cloudflare und Vercel erreichbar) passt zu Ja/Nein-Fragen mit Wahrscheinlichkeit, etwa „schädlich?“ pro Datei oder Hunk. Es ist billig und schnell. Ein Chat-Modell liefert dafür Begründungen. Für beide gilt:
- Sie können Funde nur hinzufügen.
- Eine hohe Wahrscheinlichkeit hebt die Schwere an.
- Eine niedrige entkräftet keinen Regel-Fund.

Der Server-Durchgang erlaubt bisher nur Chat-Endpunkte, für Klassifikatoren käme ein eigener hinzu.

## Phase 2d – Prompt-Injection

**Frage des Nutzers (17.10.):** Lassen sich Prompt-Injections erkennen, etwa indem Jev den Text einer Webseite prüft?

Grundsatz: **Erkennen hilft, verlassen kann man sich darauf nicht.** Angreifer formulieren um, bis ein Erkenner nichts merkt. Wichtiger ist, was eine gelungene Injection anrichten kann, und das begrenzt harness schon:
- keine Secrets im Container
- Netz nur nach Regeln; eine Reverse Shell bräuchte einen fremden Host, und das Gateway fragt nach
- keine Host-Ordner
- Merge-Prüfung vor dem Übernehmen

Bausteine:
1. ✅ **Hinweis im System-Prompt** (`briefing.rs`, 17.10.): Gelesene Inhalte sind Daten, keine Anweisungen. Der Agent soll nie deswegen Installations-Skripte, Hooks oder Tasks anlegen, an Zugangsdaten gehen, andere Hosts ansprechen oder Prüfungen abschwächen. Verdächtigen Text meldet er dem Nutzer.
2. ✅ **Warnungen bei verdächtigen Inhalten** (17.10., `harness-core/pi/guard.ts`): eine harness-Erweiterung für pi, die harness aus dem schreibgeschützten Tools-Volume lädt (`-e`), damit der Agent sie nicht ändern kann. Sie hängt sich an `tool_result` (Ausgaben von `bash`/`curl`, gelesene Dateien) und sucht mit festen Mustern:
   - unsichtbare Unicode-Tag-Zeichen (U+E0000–E007F, „ASCII smuggling“), Richtungs- und Nullbreitenzeichen
   - Wendungen wie „ignore previous instructions“, „you are now“, „system prompt“, „as an AI“
   - gefälschte Rollenmarken (`<|im_start|>`, `</system>`) und Anweisungen in HTML-Kommentaren

   Bei einem Treffer setzt sie eine Warnung vor das Ergebnis, damit das Modell es als Daten behandelt, und zeigt sie dem Nutzer in pi (`ctx.ui.notify`). Geprüft mit einem Mock-LLM, das pi eine Datei mit Anweisung im HTML-Kommentar lesen lässt: Das Werkzeug-Ergebnis kommt mit der Warnung davor beim Modell an.
3. **Optional Jev als Klassifikator** über den Server: „Enthält dieser Text Anweisungen an eine KI?“ mit Wahrscheinlichkeit. Fängt umformulierte Fälle, die keine Regel kennt. Wie bei der Merge-Prüfung darf er Warnungen nur hinzufügen, und er kostet Geld, ist also abwählbar.

## Phase 3 – Desktop-App und Gateway

- Tauri 2 als Hülle, Frontend ist `apps/web` mit Desktop-Funktionen per Capability-Erkennung
- Prozess `harnessd` (IPC) für CLI und App gemeinsam
- Gateway-Oberfläche: Live-Protokoll und Rückfragen als Desktop-Benachrichtigung statt `harness net watch`. Das Gateway selbst (DNS, Ziel-IP-Prüfung, Voreinstellungen, Rückfragen) gibt es seit Phase 2, Paket 6. Die App nutzt denselben Steuerkanal (`watch`, `decide`).
- UI für Netz-Allowlist, Git-Regeln, Projekte und Harnesse (ARCHITECTURE.md §5, §7)
- **Umgebung grafisch anlegen** (Wunsch des Nutzers, 15.10.):
  - Ordner wählen
  - Voreinstellung fürs Netz wählen
  - Schalter „für Agents aus dem Web freigeben“ (Phase 4)
- **Automatischer Sync** (Wunsch des Nutzers, 15.10.): `harnessd` holt neue Commits des Agents laufend als `agent/<projekt>`. Das ist sicher, weil es den Ordner des Nutzers nicht anfasst.
  - Übernehmen per Knopf, Benachrichtigung bei neuen Commits.
  - Pro Projekt optional automatisch, aber nur konfliktfrei und nur bei sauberem Arbeitsbaum.
  - Der Explorer sieht die Dateien also nach dem Übernehmen. Eine Live-Spiegelung ist der Sync-Modus in Phase 5.

## Phase 4 – Chat ↔ Umgebung

**Ziel des Nutzers (14.10.):** Im Web-Chat einem Agent mit wenigen Klicks einen Container auf dem eigenen Rechner zuweisen. Der Agent arbeitet dort und bearbeitet Dateien. Ablauf:
1. Einmalig auf dem Rechner harness installieren und „Gerät verbinden“. Die Kopplung gibt es schon.
2. Im Chat „Mit Umgebung verbinden“ wählen, dann Gerät, Projekt und Harness.
3. Die erste Verknüpfung eines Chats mit einem Projekt bestätigt der Nutzer lokal. Optional: „eigene Chats automatisch erlauben“.
4. Der Agent arbeitet in der Instanz. Der Chat zeigt Dateiänderungen und Befehle, übernehmen geht per Knopf (sync/merge).

**Projekte direkt aus dem Web anlegen:** leerer Arbeitsbereich oder Klon einer Git-URL, ohne Repository auf dem Host. Offen ist dabei, wohin die Ergebnisse gehen: Download, Push aus dem Container mit eigenem Token, oder ein Host-Ordner.

Bausteine: WSS-Tunnel von `harnessd` zum Server, Projektfreigabe mit lokaler Bestätigung, **pi-RPC-Adapter** (`pi --mode rpc` über `podman exec -i`, Instanz und pi-Anbindung aus Phase 2 wiederverwendet), `AgentEvent`-Rendering im Chat (ARCHITECTURE.md §6). Für „wenige Klicks“ müssen außerdem der VM-Start ohne Admin-Fenster (Phase 2b) und der Setup-Assistent (Phase 3) stehen. Kopplung und LLM-Durchgang des Servers gibt es schon seit Phase 2 (Paket 7, eigene Geräte-Tokens statt Better-Auth-`deviceAuthorization`). TLS im Gateway gibt es seit dem 17.10. (`https://`-Server, eigene CA über `[server] ca`). Offen dazu: Verbrauch pro Gerät erfassen.

**Stand 17.10.: gebaut und gegen die echte VM geprüft.**
- **Server** (`apps/server/src/tunnel/`, `routes/agent.ts`):
  - WebSocket `/api/device/tunnel` mit Geräte-Token und `DeviceHub` (Online-Status, Projekte, laufende Antworten).
  - Tabelle `chatLink`; `PUT/DELETE /api/conversations/:id/link`, `POST …/approvals/:id`.
  - Ein verknüpfter Chat schickt Nachrichten ans Gerät statt ans Modell. Die Agent-Ereignisse werden als AI-SDK-Stream gestreamt und gespeichert.
- **Gerät** (`harness connect`, `harness project remote on|off`):
  - nur freigegebene Projekte, neue Chats mit Bestätigung im Terminal
  - pi im RPC-Modus mit einer Sitzung pro Chat (`--session-id`)
  - Übersetzung in neutrale Agent-Ereignisse (`pi_rpc.rs`)
  - Rückfragen des Gateways in den Chat und Antworten zurück
- **Web:**
  - Umgebung-Knopf im Composer, Dialog Gerät → Projekt
  - Werkzeugaufrufe als Blöcke, Hinweise, Rückfragen als Karte mit den Antworten aus `harness net watch`
  - Online-Status; Fehlercodes werden übersetzt
- **Geprüft:**
  - Server-Tests (Hub, Verknüpfung, Stream, WebSocket mit Token)
  - Browser-E2E mit simuliertem Gerät
  - `scripts/smoke-connect.sh` gegen die VM: Ablehnung ohne Bestätigung, pi per RPC, `bash` mit Commit, Rückfrage zu example.com im Chat beantwortet, Antwort gespeichert, `harness sync`
- **Offen:**
  - neuer Chat direkt mit Umgebung (heute erst verknüpfen, wenn der Chat existiert)
  - Übernehmen per Knopf (sync/merge mit Anzeige der Merge-Prüfung)
  - Dialoge von pi-Erweiterungen im Web (heute abgelehnt)
  - fortsetzbare Streams: Neuladen während einer Antwort bricht sie ab
  - `harnessd` statt `harness connect` im Vordergrund, Bestätigung später in der App
  - Verbrauch und Limits pro Gerät

## Phase 5 – Ausbau (ohne feste Reihenfolge)

- **Host-Modus** (ungesichert, Harness direkt auf dem Host, ARCHITECTURE.md §3)
- **Host-Ausführung mit Einzelbestätigung** (Idee des Nutzers vom 13.10., noch kein konkreter Bedarf; so bauen, dass sie sich später ergänzen lässt): Der Agent darf einzelne Befehle auf Windows ausführen lassen, etwa PowerShell, MSVC/.NET, Windows-GUI oder Hardware. Jeder Befehl wird außerhalb der Sandbox bestätigt, so wie die Netz-Rückfragen. Vorschlag zur Umsetzung:
  - pro Projekt aus, solange nicht eingeschaltet, und nie für verknüpfte Remote-Chats
  - Anfrage aus dem Container über das Gateway, Bestätigung über dessen Steuerkanal (Muster wie `ask`)
  - Befehl als argv ohne Shell-String, angezeigt mit Arbeitsordner und Umgebung
  - Ausführung in einem eigenen Worktree des Agent-Branches, nie im Checkout des Nutzers
  - Umgebung ohne Secrets, mit Timeout, Ausgabelimit und Audit-Log
  - kein „immer erlauben“ für Befehle, die Agent-Code ausführen (`python x.py`, `npm test`, `make`)

  Wichtig: Wer bestätigt, führt Agent-Code mit vollen Rechten aus. Die Befehlszeile sieht harmlos aus, das Skript dahinter kann alles.
- **Sync-Modus** (Live-Spiegelung des Volumes in einen Host-Ordner)
- **pi auf dem Host, Werkzeuge in der Sandbox** (Idee des Nutzers vom 16.10., für später vorgemerkt; vorerst bleibt pi im Container). pi unterstützt das selbst: Seine Doku nennt es „tool-only isolation“, die Beispiele sind `examples/extensions/ssh.ts` und Gondolin.
  - So ginge es: Eine harness-Erweiterung ersetzt `read`, `write`, `edit` und `bash` durch Varianten mit eigenen Operations (`createBashTool` usw.), die über eine gehaltene `podman exec`-Verbindung in der Instanz arbeiten. `user_bash` leitet pis `!`-Befehle ebenso um. Ein `tool_call`-Handler blockiert jedes andere Werkzeug, so führen Fehler zur Ablehnung statt zur Ausführung auf dem Host. Rückfragen des Gateways erscheinen als pi-Dialog (`ctx.ui.select`).
  - Vorteile: pis Oberfläche läuft ohne Relay direkt im Terminal (Maus, Zwischenablage, Abfragen), und Rückfragen sind echte pi-Dialoge.
  - Risiken:
    - Der vertrauenswürdige Teil wächst um pi und seine ~120 npm-Pakete, dazu Erweiterungen und MCP-Server des Nutzers, alles mit vollen Rechten auf Windows.
    - Jedes Werkzeug, das nicht umgeleitet wird, läuft auf dem Host.
    - Projektdateien wie `.pi/` und `AGENTS.md` kämen nach einem Merge vom Agent; pis Projekt-Vertrauen müsste das abfangen.
    - Pfade müssen zwischen Windows und `/workspace` übersetzt werden, und jeder Werkzeugaufruf kostet ohne gehaltene Verbindung ~0,4 s.
  - Einschätzung: als zusätzlicher Modus machbar (ein Spike von 1–2 Tagen), aber nicht als Standard. Standard bleibt pi im Container, weil dort nichts, was das Modell auslöst, den Host erreicht, egal welche pi-Funktion es nutzt.
- **Harnesses aktualisieren** (`harness update [harness]`): Mit `PI_OFFLINE` nutzt pi seine mitgelieferte Modellliste, neue Modelle kommen erst mit einer neuen pi-Version ins Tools-Volume.
- Git-Worker mit Shim und Richtlinien wie in ARCHITECTURE.md §3.6, Checkpoints
- weitere Harnesse (opencode, Claude Code, Codex) und Adapter
- weitere Provider: Docker/Podman nativ unter Linux mit gVisor/krun, macOS, Windows Sandbox für Windows-Builds auf Rechnern mit mehr RAM
- Secret-Injection im Gateway, OIDC, Rate-Limits, Remote-Härtung
