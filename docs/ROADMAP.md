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
| 3 | Tray-App und Installer statt Desktop-App (Entscheidung 23.10.); Einstellungen im Browser | offen |
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
3. **Optional Jev als Klassifikator** über den Server (jetzt Teil von „Zweitmeinung zu Warnungen“ weiter unten): „Enthält dieser Text Anweisungen an eine KI?“ mit Wahrscheinlichkeit. Fängt umformulierte Fälle, die keine Regel kennt. Wie bei der Merge-Prüfung darf er Warnungen nur hinzufügen, und er kostet Geld, ist also abwählbar.

## Zweitmeinung zu Warnungen (Idee des Nutzers, 18.10., noch nicht gebaut)

Jede Warnung lässt sich von einem zweiten Modell bewerten. Das gilt für Prompt-Injection beim Web-Fetch und bei pi ebenso wie später für Funde der Merge-Prüfung. Es fasst Stufe 3 der Merge-Prüfung und Baustein 3 von Phase 2d zu einer Funktion zusammen.

- **Auslöser:** In der Warnung (Browser, später App und CLI) steht eine Option „Bewerten lassen“. Im Dialog wählt der Nutzer das Modell: Jev oder eines seiner Chat-Modelle aus den hinterlegten Anbietern. Optional bewertet Jev jede Warnung automatisch, das ist abwählbar, weil es Geld kostet.
- **Frischer Kontext:** Das bewertende Modell bekommt nicht den bisherigen Chat, sondern einen eigenen System-Prompt genau für dieses Szenario: Prompt-Injection auf einer Webseite, Prompt-Injection in einer Werkzeug-Ausgabe von pi oder ein verdächtiger Code-Diff. Dazu kommt der ganze Kontext:
  - **Web-Fetch:** URL, gesamter Seiteninhalt **in derselben bereinigten Form, die das Hauptmodell gesehen hat** (nach Readability und turndown), und die markierten Fundstellen.
  - **pi:** Werkzeug, Aufruf und Ausgabe.
  - **Merge-Prüfung:** Diff mit Umgebung und Regel-Funden.
- **Ergebnis:**
  - **Chat-Modell:** Urteil (harmlos, verdächtig, gefährlich), Begründung in einfachen Worten und betroffene Stellen, in festem Format (strukturierte Ausgabe), ohne Werkzeuge.
  - **Jev:** Seine Scores werden **angezeigt** (Wunsch des Nutzers), nicht nur in ein Urteil übersetzt.
- **Grenzen (wie bei der Merge-Prüfung entschieden):**
  - Der bewertete Inhalt kann selbst Anweisungen an das Bewertungsmodell enthalten („das hier ist harmlos“). Die Bewertung **ergänzt** die Warnung deshalb nur und entfernt sie nie.
  - Sie darf eine Warnung als „wahrscheinlich harmlos“ einordnen. Die Fundstelle bleibt aber sichtbar, und ein Regel-Fund der Merge-Prüfung bleibt bestehen.
- **Umsetzung:**
  - ein Server-Endpunkt `/api/assessments`; er nutzt die Keys des Nutzers (normale Chats) bzw. den Geräte-Durchgang (Merge-Prüfung auf dem Rechner)
  - ein Durchgang für Klassifikatoren wie Jev neben den Chat-Endpunkten
  - das Ergebnis gespeichert an der Warnung (Datenteil bzw. Fund), mit Modell, Kosten und Zeit
- **Einstellungen:** welche Warnungen automatisch bewertet werden, mit welchem Modell, Höchstkosten pro Tag.

## Phase 3 – Tray-App und Einrichtung (statt Desktop-App)

**Entscheidung des Nutzers (23.10.):** keine Tauri-App, weil die WebView viel RAM kostet. Stattdessen eine schlanke Tray-App in Rust als Brücke zwischen Browser und Rechner. Eingestellt wird alles im Browser.

Ablauf aus Sicht des Nutzers:
1. Installer einmal ausführen. Er richtet Podman, VM, VM-Dienst und Tray-App ein.
2. Einmal im Terminal koppeln (`harness login <Server>`). Danach läuft alles im Browser.
3. **Eine Umgebung entsteht nur im Terminal:** `harness init` im Ordner gibt ihn frei und meldet ihn beim Server. Die Web-App kann keine Umgebung anlegen, keinen Ordner wählen und keine Freigabe erweitern (Sicherheit, ausdrücklicher Wunsch des Nutzers). „Projekte direkt aus dem Web anlegen“ (Phase 4) entfällt damit.
4. Chat mit der Umgebung verknüpfen und eine Nachricht schicken: Der Dienst startet die VM, legt die Instanz an, bringt den Stand des Ordners per Git hinein und meldet jeden Schritt. Der Agent startet erst, wenn alles bereit ist. (Gibt es seit Phase 4.)
5. **„Fortschritt sichern“ im Chat:** Der Dienst committet offene Änderungen im Container, holt die Commits per sync, zeigt das Ergebnis der Merge-Prüfung im Chat und übernimmt sie per Knopf in den Ordner.
6. **Im Browser:** Zustand der VM, VM sofort beenden, Leerlaufzeit bis zum automatischen Stopp.

Bausteine:
- **Tray-App:** `harnessd.exe` bekommt ein Tray-Symbol (Crates `tray-icon` und `muda`, reines Win32, kein WebView, wenige MB RAM). Menü: Zustand (verbunden, VM an oder aus), Web-App öffnen, VM stoppen, Protokoll öffnen, Beenden. Optional Windows-Benachrichtigungen für Rückfragen des Gateways. Offen: Tunnel weiter im Kindprozess `harness daemon run` oder im selben Prozess.
- **Installer (Setup-Assistent, ARCHITECTURE.md §5):** Virtualisierung prüfen, Hyper-V einschalten (Neustart), Podman installieren (winget oder mitgeliefert), `harness vmd install`, Machine über den Dienst anlegen, Images bauen (große Downloads vorher ansagen), Autostart. Windows Home ohne Hyper-V kommt später (WSL als Rückfall).
- **Server und Web:** Zustand des Geräts im Tunnel (VM, letzte Aktivität), Befehl „VM stoppen“ und Einstellung „Leerlaufzeit“ pro Gerät. **Das Gerät nimmt vom Server nur harmlose Befehle an** (VM stoppen, Leerlaufzeit, Sichern, Übernehmen eines schon freigegebenen Projekts), nie neue Ordner, Projekte oder Freigaben.
- **Sichern und Übernehmen:** neue Tunnel-Nachrichten, die Merge-Prüfung als Datenteil im Chat, Konflikte und ein unsauberer Arbeitsbaum als verständliche Meldung.
- Nicht mehr geplant: eigene Oberfläche für Netzregeln, Git-Regeln und Harnesse in einer App. Was davon im Browser gebraucht wird, kommt dorthin. Die Dateien in `~/.harness` bleiben die Quelle.

**Stand 23.10. (Branch `feat/environment-ux`), Block „Umgebung im Web-Chat“:**
1. ✅ **Zustand von Gerät und VM, Stopp und Leerlaufzeit im Browser.**
   - Tunnel-Protokoll 2 mit Aushandlung: Server und Rechner lassen sich nacheinander aktualisieren.
   - Das Gerät meldet VM-Zustand (auch „wird gestoppt“), laufende Agent-Antworten und harness-Befehle im Terminal sowie die Sekunden bis zum Stopp im Leerlauf.
   - Im Umgebungs-Popover des Chats und unter Einstellungen → Geräte: Zustand, „In Benutzung: …“ bzw. „Stoppt um 14:35“, Knopf „VM stoppen“ (fragt nach, wenn etwas läuft), Menü „Automatisch stoppen nach 15 min / 30 min / 1 h / 2 h / nie“ (Standard 30 min, pro Gerät auf dem Server gespeichert).
   - Der Hintergrunddienst stoppt die VM nach der Leerlaufzeit. Leerlauf heißt: keine Antwort eines Agents, kein harness-Befehl im Terminal (Belegt-Marker) und keine fremden Container in der VM.
   - Geprüft: Server-Tests, Browser-E2E, `VM_STOP=1 bash scripts/smoke-connect.sh` gegen die echte VM (Stopp aus dem Browser und nach 1 min Leerlauf).
2. neuer Chat direkt mit Umgebung
3. „Fortschritt sichern“ und „Übernehmen“ im Chat (vorher die offenen Fragen unten klären)
4. Tray-Symbol für `harnessd`
5. Installer
6. `web_fetch` für pi, fortsetzbare Streams

Offen (mit dem Nutzer klären):
- Übernehmen direkt in den ausgecheckten Branch (wie `harness merge`, nur bei sauberem Arbeitsbaum) oder erst nur `agent/<projekt>` holen und mit einem zweiten Knopf mergen.
- Schwere Funde der Merge-Prüfung: im Browser übergehbar oder nur im Terminal?
- „Immer erlauben“ bei Netz-Rückfragen aus dem Browser schreibt heute die allow-Liste des Projekts. Bleibt das so?
- Automatischer Sync (Wunsch vom 15.10.): `harnessd` holt neue Commits laufend als `agent/<projekt>`, ohne den Ordner anzufassen.

## Phase 4 – Chat ↔ Umgebung

**Ziel des Nutzers (14.10.):** Im Web-Chat einem Agent mit wenigen Klicks einen Container auf dem eigenen Rechner zuweisen. Der Agent arbeitet dort und bearbeitet Dateien. Ablauf:
1. Einmalig auf dem Rechner harness installieren und „Gerät verbinden“. Die Kopplung gibt es schon.
2. Im Chat „Mit Umgebung verbinden“ wählen, dann Gerät, Projekt und Harness.
3. Die erste Verknüpfung eines Chats mit einem Projekt bestätigt der Nutzer lokal. Optional: „eigene Chats automatisch erlauben“.
4. Der Agent arbeitet in der Instanz. Der Chat zeigt Dateiänderungen und Befehle, übernehmen geht per Knopf (sync/merge).

~~**Projekte direkt aus dem Web anlegen**~~: verworfen am 23.10. Umgebungen entstehen nur per `harness init` im Terminal (Sicherheit, Entscheidung des Nutzers, siehe Phase 3).

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
- **18.10., nach dem ersten Test des Nutzers:**
  - Hintergrunddienst statt `harness connect` im Terminal (`harness daemon`, `harnessd.exe`, Autostart bei der Anmeldung)
  - `harness init` meldet das Projekt sofort; Umgebungen bleiben beim Server gespeichert und sind auch bei ausgeschaltetem Rechner wählbar
  - keine Bestätigung pro Chat mehr als Standard
  - Fortschritt jedes Schritts im Chat; VM-Status im Dialog
  - Kontext-Füllstand und Kosten pro Chat (models.dev bzw. pi)
  - Bugfixes: Knöpfe im Composer schickten die Nachricht ab; DeepSeek schrieb nach einer Agent-Runde Werkzeugaufrufe als Text
- **Web-Fetch in normalen Chats (18.10., gebaut von einem Hilfsagenten):** Das Modell eines Chats ohne Umgebung kann über das Werkzeug `web_fetch` Seiten laden, serverseitig.
  - Schutz: nur öffentliche Adressen (DNS geprüft und festgehalten, jeder Redirect neu), Ports 80/443, 20 s, 2 MiB, nur Text-Inhaltstypen. HTML wird als Markdown umgewandelt (turndown).
  - Freigabe: Hosts aus den Nachrichten des Nutzers laden ohne Frage. Sonst erscheint eine Karte mit voller URL (einmal / Host für diesen Chat / ablehnen).
  - Inhalte gehen als markierte, nicht vertrauenswürdige Daten ans Modell, mit Warnungen nach den Mustern des pi-Schutzes.
  - Admin-Schalter in den Server-Einstellungen (Standard an).
  - Offen: CSP auch im Vite-Dev-Server (Bild-Exfiltration per Markdown). Hauptinhalt und Suche siehe „Recherche“.
- **Recherche, Wünsche des Nutzers vom 18.10., am selben Tag gebaut:**
  - **Umgesetzt** (Details in CLAUDE.md, „Websuche und Recherche-Modus“ und „Web-Fetch“):
    - Fundstellen mit Ausschnitt und Satz zur Gefahr (Web aufklappbar, pi in der Meldung)
    - Hauptinhalt per Readability; Rückfall auf die ganze Seite unter 500 Zeichen oder 20 % des Seitentexts (Startseiten). Tabellen als Markdown, `fullPage` für die ganze Seite.
    - `web_search` mit SearXNG, Brave oder Tavily, Schalter im Composer, Trefferblock und Quellen unter der Antwort
  - **Offen:**
    - echter Suchlauf gegen SearXNG auf dem Pi (Nutzer)
    - Suche für Agents in der VM
    - **`web_fetch` für pi** (Frage des Nutzers, 23.10.): eine pi-Erweiterung (wie der Guard per `-e`) bringt ein Werkzeug `web_fetch` mit `focus`, `grep`, `sections` und `fullPage`. Es ruft über `http://llm.harness/…` einen neuen Server-Pfad mit dem Geräte-Token. Der Server lädt die Seite wie im Web-Chat (Adressfilter, Readability, Abschnitte, Embeddings mit dem OpenRouter-Key des Nutzers, Prompt-Injection-Funde). Das Gateway prüft den Ziel-Host der Anfrage gegen die Netzregeln des Projekts und fragt wie bei direkten Verbindungen, sonst wäre der Server ein Umweg um die Regeln.
    - **DeepSeek als weiterer Suchanbieter prüfen** (Hinweis des Nutzers vom 18.10.): DeepSeek bietet laut seiner Website neuerdings eine Websuche unter `https://api.deepseek.com/v1/tools/search` an, vermutlich auf Sprachmodelle zugeschnitten. Noch ungeprüft:
      - Antwortformat und Preis
      - ob der normale API-Key reicht

      Passen würde es als weiteres `SearchBackend` neben Brave und Tavily. Offen ist auch, ob dafür der DeepSeek-Zugang eines Nutzers dienen soll. Die Sucheinstellungen gelten bisher serverweit und haben einen eigenen Key.
  - **Nach dem ersten Test des Nutzers (20.10.) erledigt:**
    - Gelesene Treffer sind im Suchblock markiert („gelesen, Quelle n“).
    - Fragt eine URL im Recherche-Modus, sagt die Karte warum und zeigt die ähnlichste gefundene URL.
    - Gefundene URLs zählen auch in aufgeräumter Form (Schema, `www.`, Schrägstrich, weniger Parameter).
    - Fehlalarme behoben: Kommentar-Regel über leere Svelte-Marken hinweg, Nullbreitenzeichen als Umbruchstellen
  - **Verbessern, Ideen des Nutzers vom 20.10. (noch nicht gebaut):**
    1. **Quellen gewichten.** Seriöse Quellen sollen oben stehen, damit das Modell weniger Unsinn liest.
       - *Stufe 1, fest und kostenlos:* Domain-Listen in den Server-Einstellungen (bevorzugt, abgewertet, ausgeschlossen) mit einer mitgelieferten Grundliste (Wikipedia, offizielle Doku, Behörden, Normen, Nachrichtenagenturen). Der Server holt mehr Treffer (etwa 20), sortiert sie um und gibt die besten 8 weiter. Bevorzugte Treffer sind für Modell und Oberfläche markiert. SearXNG kann das mit dem Plugin „Hostnames“ selbst, eine eigene Liste gilt aber auch für Brave und Tavily.
       - *Stufe 2, Jev:* Jev bewertet pro Treffer „passt Titel und Ausschnitt zur Frage?“ mit Wahrscheinlichkeit und sortiert danach. Das passt zu einem Klassifikator (Ja/Nein mit Score). Es braucht den Durchgang für Klassifikatoren, den auch „Zweitmeinung zu Warnungen“ braucht, und kostet pro Suche 8–20 kurze Anfragen plus Wartezeit.
       - *Schwächen:* „Seriös“ hängt vom Thema ab (für Erfahrungsberichte ist ein Forum oft besser als ein Lexikon). Eine Liste bevorzugt Bekanntes. Die Ausschnitte stammen von den Seiten selbst, ein Klassifikator kann also getäuscht werden; er sortiert aber nur und gibt keine Anweisungen weiter.
    2. **Kontext sparen: Teilagent liest.** Seiten in Blöcke teilen, passende Blöcke auswählen (Stichwortsuche wie BM25, Embeddings oder Jev), ein günstiges Modell fasst sie zur Frage zusammen, das Hauptmodell liest nur die Zusammenfassung.
       - *Nutzen:* Der Kontext des Hauptmodells bleibt klein. Jeder Werkzeugschritt schickt den ganzen bisherigen Verlauf erneut, bei 5 Seiten à 10 000 Token und 8 Schritten werden das schnell 200 000+ Eingabe-Token pro Antwort. Kleiner Kontext heißt auch: Das Modell übersieht weniger (lange Kontexte verwässern die Aufmerksamkeit).
       - *Kosten:* Mit DeepSeek als Hauptmodell spart es wenig Geld, weil DeepSeek selbst billig ist und Präfixe cacht; die Zusammenfassungen lesen die Seiten ja trotzdem einmal. Deutlich lohnt es sich bei teuren Hauptmodellen (Claude, GPT) und für die Qualität.
       - *Schwächen:* Informationsverlust (der Teilagent entscheidet mit engerem Blick, was wichtig ist, und das Hauptmodell sieht nicht, was fehlt), eine zusätzliche Stelle für Halluzinationen, Wartezeit pro Seite (ein weiterer Modellaufruf), die Frage muss präzise mitgegeben werden, und Zitate werden ungenauer. Gegen eingeschleuste Anweisungen hilft es kaum, auch der Teilagent liest die Seite.
       - *Vorschlag:* erst **ohne Modell**: `web_fetch` bekommt einen Parameter `focus` (was das Modell sucht), der Server teilt die Seite in Abschnitte, wählt per BM25 die passenden aus und schickt sie mit Gliederung und dem Hinweis, wie es mehr bekommt. Kostet keine Token und läuft auf dem Pi. Dann Token pro Recherche-Antwort messen (`metadata.usage`). Erst wenn das nicht reicht, ein Teilagent, der **wörtliche Zitate** samt Abschnitt zurückgibt statt freier Zusammenfassung; Modell wählbar, Abschalter.
  - **Planung mit dem Nutzer, 20.10. (Fakten und Rechnung; noch nichts entschieden):**
    - *Alles über einen OpenRouter-Key* ist möglich: DeepSeek-Modelle, Jev und Embeddings. Kosten, Cache und Reasoning werden für OpenRouter genauso erfasst wie für DeepSeek direkt. OpenRouter verteilt ein Modell auf mehrere Hoster mit je eigenem Cache; ob Treffer ankommen, zeigt die Token-Tabelle unter jeder Antwort.
    - *Preise (Katalog vom 20.10., je Mio. Token):*

      | Modell | Eingabe | Cache-Treffer | Ausgabe |
      |---|---|---|---|
      | `deepseek-v4-flash` direkt | 0,15 $ | 0,003 $ | 0,60 $ |
      | `deepseek/deepseek-v4-flash` über OpenRouter | 0,091 $ | 0,0182 $ | 0,182 $ |
      | `deepseek-v4-pro` direkt | 0,66 $ | 0,022 $ | 1,98 $ |
      | Jev (`typesafe/jev-1.13`, `~typesafe/jev-latest`) | 0,042 $ | – | kostenlos |
      | Embeddings über OpenRouter (`pplx-embed-v1-0.6b`, `qwen3-embedding-8b`, `bge-m3`, `text-embedding-3-small`) | 0,004–0,02 $ | – | – |

      Einige Embeddings gibt es auch kostenlos mit Limit. Das Kontextfenster der DeepSeek-V4-Modelle ist 1 Mio. Token.
    - *Jev:* eigener Endpunkt `POST /api/alpha/decisions` (Alpha), kein Chat-Modell. Er bekommt einen Zustand und benannte Fragen und gibt Wahrscheinlichkeiten zurück, aber keinen Text: `Choice` (Option), `Noul` (ja/nein), `Score` (Stufe). Mehrere Fragen gehen in einen Aufruf. Das Kontextfenster hat 262 144 Token, über etwa 32 KB Text schaltet Jev aber in einen langsameren und ungenaueren Modus; Anfragen also unter ~8 000 Token halten. Der Server braucht dafür einen eigenen kleinen Client; für Funde der Merge-Prüfung auf dem Rechner zusätzlich einen erlaubten Pfad im LLM-Durchgang.
    - *Seitengrößen* nach Readability (gemessen am 18.10.): Nachrichtenartikel etwa 1 000–2 000 Token, Wikipedia und Doku bis zur Grenze von 40 000 Zeichen (rund 10 000 Token).
    - *Rechnung mit `deepseek-v4-flash` direkt*, eine Recherche-Antwort mit Suche und zwei Seiten à 5 000 Token:
      - Seiten neu 0,15 Cent, Treffer 0,015 Cent
      - Wiederholungen innerhalb der Antwort aus dem Cache unter 0,01 Cent
      - Ausgabe samt Reasoning um 0,15 Cent
      - Zusammen rund 0,3 Cent. Beim Programmieren ist fast alles Cache-Treffer (1/50 des Preises), deshalb ist Code dort so billig.
    - *Varianten:*
      - Jev sortiert 8 Treffer anhand ganzer Seiten: rund 0,17 Cent, so teuer wie das Lesen, das es sparen soll, dazu Abrufzeit für 8 Seiten. **Lohnt nicht.**
      - Jev sortiert nach Titel, URL und Ausschnitt: ein Aufruf, rund 0,003 Cent. **Lohnt.**
      - Jev wählt Abschnitte einer Seite (5 000 Token): rund 0,02 Cent. Liest das Hauptmodell dann 1 500 statt 5 000 Token, spart es rund 0,05 Cent pro Seite, also netto etwa die Hälfte; bei 10 000-Token-Seiten mehr.
      - Embeddings statt Jev für dieselbe Auswahl: rund 0,005 Cent, gröber im Urteil. Sinnvoll gemischt: BM25 und Embeddings wählen vor (zusammengeführt per Reciprocal Rank Fusion), Jev bewertet nur die besten Kandidaten.
      - Teilagent fasst zusammen: kostet mit DeepSeek als Hauptmodell rund 0,03 Cent pro Seite **mehr**, als er spart, und bringt 2–5 s Wartezeit. Lohnt nur bei teurem Hauptmodell (Pro, Claude, GPT) oder wenn spätere Wiederholungen teuer sind (OpenRouter-Cache nur 1/5). Kostenlose Alternative fürs Kontext-Schonen: nach der Antwort Seiteninhalte im Verlauf durch die verwendeten Abschnitte ersetzen.
    - *Domain-Listen nach Art der Frage* (Idee des Nutzers): z. B. Wissenschaft und Fakten (Wikipedia, Fachportale, Behörden), Erklären und Lernen (Studyflix, simpleclub …), Erfahrungen (Reddit, große Foren), Technik und Doku (offizielle Doku, GitHub, Stack Overflow), Nachrichten. `web_search` bekommt einen Parameter für die Art; das Hauptmodell wählt sie (kostet nichts), Jev wäre später möglich. Die Listen gewichten nur: Der Server holt mehr Treffer (etwa 20), sortiert passende nach oben und gibt 8–10 weiter, nichts wird entfernt.
  - **Abgestimmt mit dem Nutzer, 20.10. (abends):**
    - *Er nutzt DeepSeek V4.1 Flash.* Preise je Mio. Token (Eingabe / Cache-Treffer / Ausgabe):
      - direkt `deepseek-flash`: 0,15 $ / 0,003 $ / 0,60 $
      - OpenRouter `deepseek/deepseek-v4.1-flash`: 0,028 $ / 0,00057 $ / 0,12 $ (Listenpreis; am 22.10. bot ihn kein Anbieter, abgerechnet hat DeepInfra 0,14 $ / 0,0042 $ / 0,42 $. Seitdem zeigt der Chat die abgerechneten Kosten.)
      - OpenRouter `…-1017`: 0,15 $ / 0,003 $ / 0,60 $

      Der Cache-Rabatt ist bei allen 1/50. „1/5“ galt nur für V4 Flash über OpenRouter.
    - *Folge:* Mit dem günstigen OpenRouter-Flash liest das Hauptmodell eine Seite à 5 000 Token für rund 0,014 Cent. Eine Abschnittsauswahl mit Jev (~0,022 Cent) wäre dort teurer als das Lesen. Jev lohnt dann wegen der Qualität (bessere Quellen, gezielter Kontext), nicht wegen des Geldes; mit Flash direkt spart es rund 40 % pro Seite.
    - *Jev statt Embeddings:* Beides kostet Bruchteile eines Hundertstel-Cents. Jev liest Frage und Text zusammen und urteilt „beantwortet das die Frage?“; Embeddings vergleichen nur Ähnlichkeit. Jev allein ist außerdem weniger Bauteile (keine BM25-, keine Embedding-Strecke). **Entscheidung: Jev für Reihung und Abschnitte**, ohne Jev ein einfacher Rückfall (Domain-Listen bzw. Seitenanfang). Embeddings nur, wenn Jev enttäuscht. Kein zusammenfassender Teilagent vorerst.
    - *Reihung:* Der Server holt rund 20 Treffer. Jev bewertet sie in **einem** Aufruf (Titel, URL, Ausschnitt; Frage = Suchanfrage plus letzte Nachricht des Nutzers, rund 2 000 Token, ~0,008 Cent). Weitergegeben werden die besten 8–10 in Jevs Reihenfolge, mit Relevanz-Angabe. Das Modell wählt anhand von Titel, URL und Ausschnitt.
    - *Abschnitte:* `web_fetch` bekommt `focus` (was das Modell sucht). Der Server teilt die Seite in Abschnitte, Jev bewertet sie (pro Aufruf unter ~8 000 Token), zurück gehen die besten in Seitenreihenfolge mit Gliederung; weitere Abschnitte kann das Modell gezielt nachfordern.
    - *Verlauf eindampfen nur mit Schwelle:* Jede Änderung an älteren Nachrichten ändert den Anfang des Prompts, alles danach kostet dann **einmal** den vollen Preis. Mit Cache zu 1/50 ist Mitschleppen billiger als Eindampfen. Mit Abschnittsauswahl kommt ohnehin keine ganze Seite mehr in den Verlauf. Eindampfen also erst ab einer Kontextgröße (z. B. 50 000 Token) oder nach längerer Pause (Cache dann sowieso kalt): ältere Seiten nur noch mit Titel und URL, die das Modell bei Bedarf neu lädt (ohne Rückfrage, die URL ist bekannt).
    - *Cache-fester Prompt (Idee des Nutzers):* System-Prompt und Werkzeugliste bleiben im ganzen Chat gleich. Der Prompt sagt, dass sich Datum und Recherche-Modus ändern können. Ändert sich eins davon, steht vor der neuen Nutzernachricht ein kurzer Hinweis („Recherche ist jetzt aus“, „Heute ist …“). `web_search` ist immer definiert und lehnt bei ausgeschalteter Recherche ab. Weil die AI SDK keine System-Nachrichten im Verlauf erlaubt, ist der Hinweis Teil der Nutzernachricht. Er wird in ihren Metadaten gespeichert, damit er bei späteren Anfragen gleich aussieht. Ein Modellwechsel bricht den Cache weiterhin.
    - *Entwicklermodus:* Kosten je Modell (Hauptmodell, Jev, Titel). Jev liefert seine Kosten selbst (`usage.cost`).
    - *Wann Vorauswahl Geld spart (gilt für jeden Klassifikator):* Der Klassifikator liest die ganze Seite, das Hauptmodell danach nur den ausgewählten Teil. Das spart nur, wenn der Eingabepreis des Hauptmodells über dem des Klassifikators geteilt durch den weggelassenen Anteil liegt; bei 70 % weggelassen etwa über 0,06 $ je Mio. Token. Flash direkt (0,15 $) spart, OpenRouter-Flash (0,028 $) nicht. Abschnittsauswahl deshalb nur, wenn sie sich laut Preiskatalog rechnet oder die Seite lang ist (Qualität, Kontext). Das Reihen der Treffer lohnt immer, es kostet fast nichts.
    - *Alternativen zu Jev auf OpenRouter (21.10.):*
      - kein anderes Entscheidungsmodell und kein Reranker; nur Sicherheits-Klassifikatoren (`gpt-oss-safeguard-20b`, `llama-guard-4-12b`)
      - kleine Chat-Modelle als Klassifikator: `ibm-granite/granite-4.0-h-micro` (0,017 $ ein, 0,112 $ aus, mit Logprobs), `openai/gpt-oss-20b` (0,018/0,09 $, denkt mit), `inclusionai/ling-3.0-flash` (0,021/0,063 $). Die Eingabe kostet rund halb so viel wie bei Jev, dafür gibt es keine echten Wahrscheinlichkeiten, sie sind langsamer und folgen eher Anweisungen im Text.
      - Embeddings ab 0,004 $
      - kostenlos lokal: BM25 oder ein kleiner Reranker auf dem Pi
      - Reranker von Cohere, Jina und Voyage nur mit eigenem Key

      Der Unterschied zu Jev liegt bei rund 0,01 Cent pro Seite, **Jev bleibt**. Für eine spätere Zusammenfassung vorgemerkt: `scaledown/summarize` (0,04 $ Eingabe, Ausgabe kostenlos).
    - *Geändert am 21.10.: Abschnitte ohne Jev.* BM25 im Server (kostenlos, stark bei Namen, Versionen, Fehlercodes) plus Embeddings über OpenRouter, zusammengeführt per Reciprocal Rank Fusion. Modell: `qwen/qwen3-embedding-8b` (0,01 $ je Mio., ausdrücklich mehrsprachig, 40 000 Token Kontext); billiger, aber ohne Angabe zur Mehrsprachigkeit: `perplexity/pplx-embed-v1-0.6b` (0,004 $). Selbst mit OpenRouter-Flash spart das noch rund ein Drittel pro Seite, mit Flash direkt rund zwei Drittel. Ohne Embedding-Modell nur BM25. Jev bleibt für das Reihen der Treffer, später optional als Reranker für die besten 3–5 Abschnitte bei teurem Hauptmodell.
    - *Embeddings auf dem Pi* (geschätzt, nicht gemessen; ONNX, int8): kleines englisches Modell (MiniLM, 22 Mio. Parameter) etwa 1–2 s pro Seite à 5 000 Token, mehrsprachig (`multilingual-e5-small`) etwa 4–7 s, auf einem älteren Pi rund doppelt so lange, dazu 150–500 MB RAM neben SearXNG und Postgres. Die API ist schneller, besser und kostet ~0,005 Cent pro Seite. Lokal nur, wenn nichts nach außen soll.
    - *Llama Guard passt nicht:* Es erkennt schädliche Inhalte nach festen Kategorien (Gewalt, Hass …), keine Prompt-Injection, und prüft keinen Code. Mit 0,18 $ je Mio. Ein- und Ausgabe ist es zudem teurer als Jev. Für Injection gibt es Metas **Prompt Guard 2** (22 Mio. bzw. 86 Mio. Parameter, mehrsprachig), aber nicht auf OpenRouter; er könnte lokal auf dem Pi laufen. `gpt-oss-safeguard-20b` arbeitet nach einer selbst geschriebenen Richtlinie mit Begründung, kostet aber mehr als Jev (0,075 $ ein, 0,30 $ aus plus Denken). Für die Merge-Prüfung eignen sich eher Jev mit gezielten Fragen pro Änderung oder ein Code-Modell mit festem Antwortformat.
    - *Reihenfolge* (alle fünf erledigt am 20./21.10., Details in CLAUDE.md unter „Websuche und Recherche-Modus“):
      1. ✅ cache-fester Prompt: Hinweise `[LLM Harness: …]` in `metadata.note`, Werkzeugliste nur von den Server-Einstellungen abhängig
      2. ✅ Domain-Listen als Vorgewicht und Rückfall: fünf Arten, 20 Treffer holen, 10 weitergeben
      3. ✅ Jev-Anbindung samt Reihung und Kosten je Modell: Key des Nutzers, Modelle in den Sucheinstellungen änderbar
      4. ✅ Abschnittsauswahl mit `focus` (oder `sections`): BM25 plus Embeddings, RRF, ~8 000 Zeichen plus Gliederung. Das Hauptmodell entscheidet selbst; für genaue Zahlen, Tabellen und Zitate lässt es `focus` weg (Wunsch des Nutzers).
      5. ✅ Eindampfen: ältere Seiten nur mit URL und Titel, Suchen nur mit Titeln und URLs. Die Stelle wird gespeichert, damit der Cache danach wieder greift. Gekürzte Seiten laden ohne Rückfrage neu. Schwellen seit dem 21.10. abends in der Web-App: 200 000 Token oder 50 % des Fensters oder 6 h Pause (Wunsch des Nutzers: höher, damit Cache-Treffer nicht verschenkt werden).
    - *Nachgezogen am 21.10. (abends):* `grep` für genaue Zahlen, rekursives Halbieren langer Abschnitte, Anhänge ausgenommen, Embeddings 3–5× schneller (base64, 1024 Dimensionen, parallel), Hinweis „neu suchen“ bei schwachen Treffern, Entwicklermodus entfernt, Hersteller in der Technik-Liste. Jev und Embeddings mit dem Key des Nutzers echt gemessen (CLAUDE.md, „Websuche und Recherche-Modus“).
    - ✅ *BM25 mit Stammformen und Wortteilen* (21.10. spät): CISTEM für Deutsch, Porter für Englisch, Wortteile deutscher Komposita, Füllwörter raus; 19/20 statt 15/20 auf echten Seiten. Ein Thesaurus (OpenThesaurus, Open English WordNet, ConceptNet) bleibt aus: Er kennt keinen Zusammenhang und bringt bei mehrdeutigen Wörtern Fehltreffer, Synonyme decken die Embeddings ab.
    - ✅ *Nachgezogen am 22./23.10.:* abgerechnete OpenRouter-Kosten, gesperrte Suchmaschinen erkannt (Bing und Yahoo zugeschaltet), Kostentabellen getrennt nach Chat-Modell und Hilfsmodellen, die Karte zeigt „semantisch“, „Stichworte“ oder „ganze Seite“, Belege als Link nach jeder Aussage mit Hinweis auf „Ungeprüfte Links“ (Wunsch des Nutzers), Laufschrift für lange Modellnamen. Danach (Wunsch des Nutzers): echte Links statt Streamdowns Knöpfen, Quellen gesammelt als Badge am Absatzende mit Liste beim Zeigen, Tooltip zu „bevorzugte Quelle“.
  - Ursprünglicher Plan:
  - **Hauptinhalt:** Mozilla Readability (`@mozilla/readability`) auf einem `linkedom`-DOM, danach turndown. Liefert Readability nichts Brauchbares (unter ~500 Zeichen oder `null`), wird die ganze Seite wie bisher umgewandelt. Defuddle ist die Alternative, falls Readability bei Dokumentationsseiten zu viel wegschneidet. Kein externer Dienst (Jina Reader o. Ä.), weil der jede abgerufene URL sähe.
  - **Websuche:** Werkzeug `web_search({ query })` in normalen Chats. Es liefert die ersten Treffer mit Titel, URL, Ausschnitt und Datum. Ob gesucht wird, entscheidet das Modell selbst über den Werkzeugaufruf, wie bei Perplexity. Der System-Prompt sagt: suchen, wenn Fakten unsicher, aktuell oder nach dem Wissensstand des Modells sind, und Quellen als Links nennen.
    - **Suchanbieter** in den Server-Einstellungen:
      - SearXNG als selbst gehosteter Standard (eigener Dienst in `docker-compose.yml` hinter einem Profil, JSON-Ausgabe einschalten, kein Key, nichts verlässt den Pi außer den Suchanfragen)
      - wahlweise Brave Search API oder Tavily mit Key, verschlüsselt gespeichert wie die Anbieter-Keys
    - **Recherche-Schalter im Composer** (Vorschlag des Nutzers: eine Fähigkeit, die man wie eine Domain freigibt). Ist er an, darf das Modell suchen und ohne Rückfrage jede URL laden, die in Suchergebnissen, in Nachrichten des Nutzers oder als Link auf schon geladenen Seiten vorkam. Nur URLs, die das Modell selbst zusammensetzt, fragen weiter nach: Darüber ließen sich Chat-Inhalte in eine Adresse packen und hinausschicken. Die Muster gegen Prompt-Injection laufen immer.
    - Anzeige: ein Block mit der Trefferliste, darunter die Quellen als nummerierte Links. Keine Favicons von fremden Servern (CSP, Privatsphäre).
    - Später auch für Agents in der VM: Suche über den Server (eigener Pfad wie `/api/llm`), sonst bräuchte jedes Gateway den Suchanbieter freigegeben.
  - **Fundstellen von Prompt-Injection:** Treffer kommen mit Ausschnitt und Markierung, unsichtbare Zeichen sichtbar gemacht, Tag-Zeichen entschlüsselt. Im Browser lässt sich die Warnung aufklappen, in pi steht der Ausschnitt in der Meldung. So kann der Nutzer selbst einschätzen, ob es kritisch ist.
    - **Zu jedem Fund ein Satz, warum er gefährlich ist** (Wunsch des Nutzers, für Laien verständlich). Beispiele:
      - „ignore previous instructions“: Die Seite versucht, deine Anweisungen an das Modell zu ersetzen.
      - Versteckter Text: Anweisungen, die du nicht siehst, das Modell aber liest.
      - Anweisung im HTML-Kommentar: im Browser unsichtbar, für das Modell normaler Text.
      - „sag es dem Nutzer nicht“: Das Modell soll etwas vor dir verbergen.
      - Gefälschte Rollenmarken: Der Text tut so, als käme er vom System oder von dir.

      Die Texte stehen in den i18n-Dateien. pi bekommt sie in der Meldung auf Deutsch.
  - **Windows-Builds auf GitHub** (Frage des Nutzers): Bisher baut GitHub nur das Server-Image und prüft das Gateway. `harness.exe`/`harnessd.exe` entstehen lokal mit `cargo build`. Ein Workflow auf `windows-11-arm` und `windows-latest` mit Artefakten bei Tags gehört zum Installer in Phase 3.
- **Offen:**
  - neuer Chat direkt mit Umgebung (heute erst verknüpfen, wenn der Chat existiert)
  - Übernehmen per Knopf (sync/merge mit Anzeige der Merge-Prüfung)
  - Dialoge von pi-Erweiterungen im Web (heute abgelehnt)
  - fortsetzbare Streams: Neuladen während einer Antwort bricht sie ab
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
- **Secrets mit dem Passwort des Nutzers verschlüsseln** (Idee des Nutzers vom 23.10.; am 23.10. entschieden: wird gebaut, aber erst am Ende, nach den anderen Blöcken). Heute: AES-256-GCM (`SecretBox`), Schlüssel per HKDF aus dem Server-Secret (`HARNESS_SECRET` oder `data/server-secret`). Datenbank allein reicht also nicht, Datenbank plus Secret schon; beides liegt auf derselben SSD.
  - Vorschlag: pro Nutzer ein zufälliger Datenschlüssel, verpackt mit einem Schlüssel aus dem Passwort (Argon2id). Beim Login entpackt, nur im RAM gehalten, an die Sitzung gebunden. Passwortwechsel verpackt nur neu.
  - Folgen: Nach einem Neustart des Servers funktionieren Keys erst wieder, wenn sich der Nutzer anmeldet. Gekoppelte Rechner (LLM-Durchgang, Hintergrunddienst) bräuchten einen eigenen, mit dem Geräte-Token verpackten Schlüssel, sonst gehen sie nur, solange der Nutzer im Web angemeldet ist. Passwort vergessen heißt Keys neu eintragen (Admin-Reset kann sie nicht retten). Serverweite Keys (Brave, Tavily) bleiben beim Server-Secret. Swap kann RAM-Inhalte auf die SSD schreiben (auf dem Pi abschalten oder zram).
  - Schützt gegen: gestohlene SSD oder Backup. Nicht gegen: Angreifer auf dem laufenden Server (der sieht den RAM und jede Anmeldung).
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
- **Dateien im Chat** (Idee des Nutzers, 18.10.):
  - **Dokumente** (Word, PowerPoint, Excel, PDF, HTML …) per [markitdown](https://github.com/microsoft/markitdown) von Microsoft in Markdown umwandeln und dem Modell geben. markitdown ist Python, der Server Node. Deshalb kommt es als eigener kleiner Container neben den Server, mit Größen- und Zeitlimit, ohne Netz. Datei-Parser sind Angriffsfläche und laufen deshalb nicht im Server-Prozess. In verknüpften Chats kann die Umwandlung auch in der Instanz laufen.
  - **Bilder für Vision-Modelle:** Anhänge im Composer, an das Modell als AI-SDK-Dateiteil. Ob ein Modell Bilder versteht, steht im Katalog von models.dev (`modalities.input`); sonst Knopf ausgrauen. Bilder im Datenordner speichern, nicht als Base64 in der Datenbank, mit Größenlimit und Herunterskalieren. pi nimmt Bilder per RPC (`images` im `prompt`).
- **Harnesses aktualisieren** (`harness update [harness]`): Mit `PI_OFFLINE` nutzt pi seine mitgelieferte Modellliste, neue Modelle kommen erst mit einer neuen pi-Version ins Tools-Volume.
- Git-Worker mit Shim und Richtlinien wie in ARCHITECTURE.md §3.6, Checkpoints
- weitere Harnesse (opencode, Claude Code, Codex) und Adapter
- weitere Provider: Docker/Podman nativ unter Linux mit gVisor/krun, macOS, Windows Sandbox für Windows-Builds auf Rechnern mit mehr RAM
- Secret-Injection im Gateway, OIDC, Rate-Limits, Remote-Härtung
