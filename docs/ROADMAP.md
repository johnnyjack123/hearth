# Roadmap

> Stand 14.10.2026. Architektur und Begründungen: [`ARCHITECTURE.md`](../ARCHITECTURE.md). Aktueller Arbeitsstand: [`PROGRESS.md`](PROGRESS.md).
> Grundsatz: **ein** Sandbox-Provider (Podman + Hyper-V + Linux-VM, Isoliert-Modus), bis er im Alltag trägt. Weitere Provider kommen erst danach.

| Phase | Inhalt | Status |
|---|---|---|
| 0 | Spike Podman WSL und Hyper-V (I/O, Netz, Sicherheit) | ✅ erledigt, [`spikes/phase-0.md`](spikes/phase-0.md) |
| 0b | Spike Windows Sandbox | ✅ abgebrochen und verworfen, [`spikes/phase-0b-windows-sandbox.md`](spikes/phase-0b-windows-sandbox.md) |
| 1 | Server + Web-Chat (Multi-User, Keys pro Nutzer, Streaming) | ✅ im Kern fertig, Reste siehe unten |
| **2** | **Sandbox-Kern + `harness`-CLI (Terminal-Modus mit pi)** | **🔨 fast fertig**, Pakete 1–9 gebaut und per Smoke-Skript geprüft. Offen: interaktiver Test in Windows Terminal durch den Nutzer, Rückfragen per ConPTY |
| 2b | Windows-Dienst `harness-vmd` (VM-Lebenszyklus ohne Admin-Fenster) | offen |
| 3 | Desktop-App (Tauri) + UI für Netz- und Git-Regeln und Rückfragen | offen |
| 4 | Chat ↔ Umgebung (Pairing, Tunnel, pi-RPC-Adapter, LLM-Gateway) | offen |
| 5 | Ausbau (Host-Modus, Sync-Modus, weitere Harnesse und Provider …) | offen |

## Phase 1 – Reste

- [ ] Server-`Dockerfile` (multi-stage, multi-arch) und `compose.yaml` (Profile `sqlite`/`postgres`). War geplant, ist **nicht** gebaut.
- [ ] ESLint- und Prettier-Konfiguration anlegen. Die Pakete sind installiert, es gibt aber keine Config-Dateien. Danach `pnpm lint` grün machen.
- [ ] `README.md` (Kurzbeschreibung, Start, Verweise).
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
   - Methoden: VM erreichbar?, Image bauen, Volume, Netz, Container anlegen, starten und stoppen, exec (mit TTY), Datei in den Austauschordner
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
5. ✅ **Git-Sync im Isoliert-Modus** (`harness-core/src/sync/`) über den Austauschordner (Host `%LOCALAPPDATA%\llm-harness\exchange` = VM `/mnt/exchange`, 9p). Einzelne Dateien und Gits Lockfile mit rename gehen dort, nur `chmod` und rekursives Löschen nicht. Deshalb Bundles statt Arbeitsbäumen. Jedes Projekt bekommt einen eigenen Unterordner (`ProjectName::exchange_dir`), den der Container als `/mnt/exchange` sieht. Verifiziert mit 13 Unit-Tests gegen temporäre Repos und `tests/sync_vm.rs` mit den echten Skripten in der VM (Klonen ~0,6 s, Rückweg ~0,7 s):
   - **Host → VM** (`push_to_workspace`): `git bundle create <exchange>/<projekt>/in.bundle HEAD --branches --tags`. Bewusst nicht `--all`, damit Stash und Remote-Tracking-Refs auf dem Host bleiben. Im Container beim ersten Mal `git clone --origin host`, danach `git fetch host`. Der Agent-Branch setzt den Agent-Branch des Hosts fort, falls es ihn gibt, sonst beginnt er am HEAD des Hosts.
   - **VM → Host** (`pull_from_workspace`): Der Container bündelt nur `agent/<projekt> --not --remotes=host`, also nur, was dem Host fehlt. Fehlen dem Host die Voraussetzungen (Kopf selbst gelesen, `BundleHeader`), folgt automatisch ein vollständiges Bundle. Der Host holt die Objekte und setzt den Branch per Compare-and-swap (`update-ref`), nur als Fast-Forward (sonst `--force`) und nie, solange der Branch ausgecheckt ist.
   - **Merge** (`HostRepo::merge`): Fast-Forward, falls möglich, sonst `git merge --no-edit`. Abgelehnt werden detached HEAD, laufende Merges oder Rebases und uncommittete Änderungen an getrackten Dateien. Konflikte erkennt vorher `git merge-tree --write-tree` trocken. Bei Konflikt bleibt alles unangetastet, und die Dateien werden gemeldet.
   - **Härtung auf dem Host:** Jeder Git-Aufruf läuft mit `core.hooksPath` auf einen leeren Ordner, `core.fsmonitor=false`, `submodule.recurse=false` und `fetch.fsckObjects`/`transfer.fsckObjects`. Ein Test beweist, dass weder `.git/hooks` noch ein Hook-Ordner im Arbeitsbaum (husky) ausgeführt werden. Container-Ausgaben werden vor der Anzeige von Steuerzeichen befreit.
   - **Offen:** `in.bundle` ist jedes Mal vollständig, bei großen Repos ggf. inkrementell machen.
6. ✅ **Netzwerk-Gateway** (`crates/harness-gateway`, Image `images/gateway`, Host-Seite `harness-core/src/gateway.rs`, CLI `harness net`). Statt tinyproxy gleich ein eigener kleiner Proxy in Rust (aus Phase 3 vorgezogen): Nur er kann „open“ sicher machen und Anfragen festhalten, bis der Nutzer entscheidet. Nur Standardbibliothek, ein Thread pro Verbindung. Gebaut wird in der VM aus den eingebetteten Quellen (`rust:1.97-slim-trixie`, Build ~10 s, offline). Verifiziert mit 28 Tests im Crate (6 davon über echtes TCP), `tests/gateway_vm.rs` und einem Probelauf der CLI mit `harness net watch`:
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

   Ein Terminal wird nur durchgereicht, wenn stdin und stdout eines sind. **Offen:**
   - interaktiver Test in Windows Terminal: TTY, Strg+C, Fenstergröße
   - Rückfragen des Gateways im selben Terminal. `harness run` läuft auf dem Host, also außerhalb der Sandbox, und müsste dafür die Ausgabe von pi über eine ConPTY durchreichen. Bis dahin `harness net watch` im zweiten Fenster.
9. ✅ **Tests:**
   - Unit-Tests: Konfiguration, Bundle- und Merge-Logik gegen temporäre Repos, Domain-Muster, Gateway, `models.json`
   - VM-Tests: `podman_vm`, `base_image_vm`, `sync_vm`, `gateway_vm`, `instance_vm`
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

**Wunsch des Nutzers (15.10.):** Im Ordner nur noch `harness` sagen, den Rest erledigt harness im Hintergrund. Dazu:
- `harness` ohne Argumente startet die Standard-Harness.
- Die VM startet bei Bedarf über `harness-vmd` und stoppt nach einer Leerlaufzeit.
- Ist der Ordner noch kein Projekt, fragt harness einmal nach und legt es an. Ohne Git-Repository bietet es `git init` an.
- Nicht gekoppelt: Hinweis auf `harness login`.

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

Bausteine: WSS-Tunnel von `harnessd` zum Server, Projektfreigabe mit lokaler Bestätigung, **pi-RPC-Adapter** (`pi --mode rpc` über `podman exec -i`, Instanz und pi-Anbindung aus Phase 2 wiederverwendet), `AgentEvent`-Rendering im Chat (ARCHITECTURE.md §6). Für „wenige Klicks“ müssen außerdem der VM-Start ohne Admin-Fenster (Phase 2b) und der Setup-Assistent (Phase 3) stehen. Kopplung und LLM-Durchgang des Servers gibt es schon seit Phase 2 (Paket 7, eigene Geräte-Tokens statt Better-Auth-`deviceAuthorization`). Offen dazu: Verbrauch pro Gerät erfassen, TLS im Gateway für entfernte Server.

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
- Git-Worker mit Shim und Richtlinien wie in ARCHITECTURE.md §3.6, Checkpoints
- weitere Harnesse (opencode, Claude Code, Codex) und Adapter
- weitere Provider: Docker/Podman nativ unter Linux mit gVisor/krun, macOS, Windows Sandbox für Windows-Builds auf Rechnern mit mehr RAM
- Secret-Injection im Gateway, OIDC, Rate-Limits, Remote-Härtung
