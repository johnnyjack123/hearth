# LLM Harness – Architektur

> Status: Entwurf · Stand 2026-10-10 · „LLM Harness“ und das CLI-Kommando `harness` sind Arbeitstitel.

## 1. Ziel

Ein System, das drei Dinge verbindet:

1. **Agents sicher im Auto-Mode laufen lassen.** Beliebige CLI-Harnesse (opencode, pi, Claude Code, Codex …) laufen in isolierten Linux-Containern. Gestartet wird per `harness <name>` direkt aus dem Projektordner.
2. **Klassischer Multi-User-Chat.** Ein selbst gehosteter Server mit anbieterunabhängigen LLMs und einer modernen Web-Oberfläche.
3. **Chat ↔ Umgebung.** Jeder Nutzer verknüpft eigene Rechner mit seinem Account. Einzelne Chats lassen sich explizit mit einer Container-Umgebung auf diesem Rechner verbinden und arbeiten dann agentisch darin.

Leitplanken:

- Windows zuerst, macOS und Linux werden unterstützt.
- Kein Chat hat Zugriff auf eine Umgebung, solange er nicht ausdrücklich verknüpft wurde.
- Alles Sicherheitsrelevante (Netzwerk, Git, Dateizugriff) lässt sich über die UI bedienen. Die Konfiguration liegt trotzdem als lesbare Dateien auf dem Host.
- Entworfen für Remote-Betrieb, zunächst nur lokal betrieben.

## 2. Überblick

```
Browser / Tauri-UI
        │  HTTPS + SSE
        ▼
Harness-Server (Docker) ─────────────► LLM-Anbieter (Anthropic, OpenAI, Google, Ollama …)
  Auth · Chats · LLM-Router · Geräte · Tunnel · LLM-Gateway
  SQLite oder Postgres
        ▲
        │  WSS-Tunnel (vom Rechner ausgehend, nur für verknüpfte Chats)
        │
harnessd (Hintergrunddienst auf dem Rechner des Nutzers)
  ├─ IPC ◄── Tauri-App (Verwaltung + Chat)
  ├─ IPC ◄── harness-CLI (Terminal)
  ├─ Git-Worker (führt geprüfte Git-Operationen auf dem Host aus)
  └─ Podman-API ──► Linux-VM (Hyper-V)
                      ├─ gateway     Egress-Proxy, einziger Weg ins Netz
                      ├─ projekt-a   ◄── Bind-Mount C:\…\projekt-a
                      └─ projekt-b   ◄── Bind-Mount C:\…\projekt-b
```

| Komponente | Technik | Aufgabe |
|---|---|---|
| Server | Node.js + TypeScript, läuft im Docker-Container | Accounts, Chats, LLM-Aufrufe, Geräteverwaltung, Tunnel |
| Web-UI | React-SPA | Wird vom Server ausgeliefert und ist gleichzeitig in der Tauri-App gebündelt |
| `harnessd` | Rust | Container, Netzwerk-Gateway, Git-Worker, Tunnel zum Server |
| Tauri-App | Tauri 2 | Oberfläche für `harnessd` plus Chat |
| `harness`-CLI | Rust | Startet Harnesse im Terminal, mit dem aktuellen Ordner als Projekt |
| Base-Image | Debian | Linux-Userland mit Toolchain für die Agents |

## 3. Laufzeitumgebung (Sandbox)

> **Richtungsentscheidung (12.10.2026, revidiert am selben Tag):** Erster und vorerst **einziger** Provider ist **Podman mit Hyper-V-Provider und Linux-VM**, im **Isoliert-Modus** (Projekte als Git-Arbeitskopie in Volumes der VM). `harness-core` wird gegen eine schmale Provider-Schnittstelle programmiert, damit später weitere Provider dazukommen können (Docker unter Linux, ggf. Windows Sandbox).
>
> Windows Sandbox wurde verworfen (Spike 0b, [`docs/spikes/phase-0b-windows-sandbox.md`](docs/spikes/phase-0b-windows-sandbox.md)):
> - Sie bootet ein vollständiges Windows mit Oberfläche, also ~1,5–2,5 GB RAM pro Instanz gegenüber 0,4–1 GB für die Linux-VM mit beliebig vielen Containern.
> - `wsb exec` liefert keine Ausgabe.
> - Der erste Start löste auf dem ARM64-Rechner einen Bluescreen aus.
>
> Auf diesem Rechner ist RAM das knappste Gut.

**Host-Modus (ungesichert, optional pro Projekt):** Die Harness läuft direkt auf dem Host im Projektordner, ohne VM und Container: volle Leistung, Windows-Tools, paralleles Bearbeiten durch den Nutzer. Es gibt **keine** Isolation, keine Netzwerkregeln und keinen Git-Worker. Er ist nur als bewusste Entscheidung des Nutzers mit deutlicher Warnung in der UI aktivierbar, nie als Standard und nie für verknüpfte Remote-Chats.

> **Hinweis zum Lesen von §3.3–3.8:** Einiges dort betrifft nur den **Direkt-Modus**, also eingebundene Projektordner, später unter Linux:
> - Bind-Mount von `/workspace`
> - read-only `.git`
> - versteckte und geschützte Pfade
> - Git-Shim und Git-Worker auf dem Host (§3.6)
>
> Für Phase 2 maßgeblich ist der **Isoliert-Modus** nach [`docs/ROADMAP.md`](docs/ROADMAP.md) Phase 2. Die Arbeitskopie liegt im Volume, der Austausch läuft über Git-Bundles in `/mnt/exchange`. Ein Git-Shim ist dort nicht nötig, weil der Agent sein eigenes `.git` hat. Weiterhin gelten Härtung (§3.3, ohne die Bind-Mount-Zeilen), Netzwerk (§3.5), Harness-Definitionen (§3.7) und Konfiguration (§3.8).

### 3.1 Schichten

```
Windows 11
└─ Hyper-V                          Hardware-Virtualisierung, keine Emulation
   └─ Linux-VM „harness“            Fedora CoreOS, verwaltet von Podman
      ├─ gateway                    Egress-Proxy
      ├─ harness-projekt-a          Debian + Toolchain + Harness, rootless
      └─ harness-projekt-b
```

`podman.exe` und `harnessd` laufen nativ unter Windows. Die Container laufen in der VM. Sie teilen sich deren Kernel, haben aber jeweils ein eigenes Dateisystem, eigene Prozesse und ein eigenes Netzwerk.

| Plattform | VM | Isolation der Container | Hinweise |
|---|---|---|---|
| Windows | Podman Machine mit Hyper-V-Provider. Fallback für Windows Home: WSL2-Provider | rootless Podman | Hyper-V braucht Admin-Rechte; einmalig über die Gruppe „Hyper-V-Administratoren“ lösen |
| macOS | Podman Machine (Apple Hypervisor oder libkrun) | rootless Podman | Später optional Apples `container` (eine VM pro Container) |
| Linux | keine | rootless Podman + gVisor (`runsc`) oder `krun` | Ohne VM teilen sich Container den Host-Kernel, deshalb eine zusätzliche Runtime |

`harnessd` verwaltet eine **eigene** Podman Machine namens `harness`, getrennt von anderen Podman-Setups des Nutzers. Podman unterstützt keine WSL- und Hyper-V-Machines gleichzeitig. Das muss der Setup-Assistent erkennen.

**Windows on ARM** ist eine Zielplattform. Alle Images werden multi-arch gebaut (`linux/amd64`, `linux/arm64`). Ob der Hyper-V-Provider von Podman auf ARM64 läuft, ist noch offen (Phase 0). Fallback ist der WSL2-Provider.

### 3.2 Base-Image

- Debian stable (slim) mit git, curl, build-essential, Python 3 + uv, Node LTS + npm/pnpm, ripgrep, fd, jq, gh.
- Ein unprivilegierter Benutzer `agent` (UID 1000).
- Ein `git`-Shim (siehe 3.6) vor dem echten git im `PATH`.
- `HTTP_PROXY`/`HTTPS_PROXY` sind auf das Gateway gesetzt.
- Harnesse liegen nicht im Image, sondern in einem gemeinsamen Tools-Volume (`/opt/harness-tools`). So lassen sie sich ohne Image-Neubau aktualisieren.
- Projektspezifische Erweiterungen kommen über ein eigenes `Containerfile` pro Umgebung (`FROM harness/base`).

### 3.3 Instanz = Projekt + Umgebung

Jedes Projekt bekommt einen dauerhaften Container `harness-<projekt>`. Nach einer Leerlaufzeit wird er gestoppt, beim nächsten Aufruf wieder gestartet.

| Pfad im Container | Quelle | Zweck |
|---|---|---|
| `/workspace` | Bind-Mount des Projektordners | Direkter Zugriff, kein Sync nötig |
| `/workspace/.git` | Bind-Mount, **read-only** | Git lesen geht, schreiben nur über den Git-Worker |
| `/workspace/node_modules`, `.venv`, `target` … | eigenes Volume | Performance; Linux-Binaries kollidieren nicht mit denen auf dem Host |
| versteckte Pfade (Standard: `.env`, `.env.*`) | überdeckt (leere Datei bzw. tmpfs) | Secrets sind für den Agent nicht sichtbar |
| geschützte Pfade (z. B. `.vscode`, `.github/workflows`, `.envrc`) | read-only | Dateien, die später auf dem Host ausgeführt werden, kann der Agent nicht ändern |
| `/home/agent` | Volume pro Projekt | Caches, Shell-History |
| Konfiguration der Harnesse (z. B. `~/.config/opencode`) | Volume pro Nutzer und Harness | Logins und Einstellungen, von allen Projekten gemeinsam genutzt |

Härtung bei jedem Containerstart:

```
--userns=keep-id --user agent
--cap-drop=ALL --security-opt no-new-privileges
--read-only --tmpfs /tmp
--pids-limit 1024 --memory <limit> --cpus <limit>
--network harness-net-<projekt>     # internes Netz, siehe 3.5
# kein Podman-/Docker-Socket, keine weiteren Host-Mounts
```

Versteckte Pfade werden beim Containerstart ausgewertet. Dateien, die danach auf dem Host neu entstehen, sind erst nach einem Neustart verborgen. Die UI weist darauf hin.

### 3.4 Dateizugriff

> **Stand nach Phase 0** ([Bericht, Abschnitt 12](docs/spikes/phase-0.md)): Unter Windows ist der Direkt-Modus nicht tragfähig. Die 9p-Freigabe des Hyper-V-Providers bricht `chmod`, `copyfile` und rekursives Löschen und damit git, npm und Builds. Unter WSL funktioniert sie, ist aber 30- bis 300-mal langsamer als ein Volume und gibt der VM ganz `C:\` frei. **Empfehlung (Bestätigung durch den Nutzer offen):** Standard je Plattform:
>
> | Plattform | Standard | Alternativ |
> |---|---|---|
> | Windows | **Isoliert-Modus** | Sync-Modus (später) |
> | Linux (nativ, ohne VM) | Direkt-Modus | Isoliert-Modus |
> | macOS (virtiofs) | noch zu messen | – |

**Direkt-Modus:** Der Ordner, in dem `harness` aufgerufen wird, wird nach `/workspace` gemountet. Agent und Nutzer arbeiten auf denselben Dateien. Es gibt nichts zu synchronisieren und damit auch keine Sync-Konflikte. Wenn beide gleichzeitig dieselbe Datei bearbeiten, gewinnt wie in zwei Editoren der letzte Schreibzugriff.

**Isoliert-Modus:** Der Agent arbeitet in einer eigenen Arbeitskopie in einem Volume der VM, auf dem Branch `agent/<name>`. Hinein kommt das Projekt per Git (Clone vom Host-Repo). Zurück kommen Commits über den Git-Worker, der den Agent-Branch ins Host-Repo holt. Der Nutzer merged über die UI, wann er will. Das ist volle Volume-Geschwindigkeit, und es gibt keinen gemeinsamen Index für Windows- und Linux-git. Uncommittete Änderungen sind erst nach einem Commit oder Checkpoint auf dem Host sichtbar.

**Sync-Modus (später):** wie Isoliert, zusätzlich spiegelt `harnessd` Dateiänderungen laufend vom Volume in einen Host-Ordner, nach dem Vorbild von Mutagen oder den Synchronized File Shares von Docker Desktop. Gedacht für alle, die Agent-Änderungen live in ihrer IDE sehen wollen.

Risiken im Direkt-Modus:

| Risiko | Gegenmaßnahme | Was übrig bleibt |
|---|---|---|
| Agent löscht oder beschädigt Dateien im Projekt | Git, automatische Checkpoints (Phase 5) | gering |
| Codeausführung auf dem Host über `.git/hooks` oder `.git/config` (`core.fsmonitor`, `core.sshCommand` …) | `.git` ist read-only, Git-Schreibzugriffe laufen nur über den Worker, und der führt keine Hooks aus | gelöst |
| Codeausführung auf dem Host über Dateien, die der Nutzer später ausführt (npm-Scripts, Makefile, IDE-Tasks, CI-Konfiguration) | geschützte Pfade, Diff-Ansicht in der UI | bleibt bestehen, wie bei jedem fremden Pull Request: vor dem Ausführen prüfen |
| Secrets aus dem Projektordner werden ausgelesen und nach außen geschickt | versteckte Pfade, Netzwerkrichtlinie | gering, solange keine Secrets ungeschützt im Ordner liegen |
| Container-Ausbruch in die VM | rootless Container, gehärtete Flags | Die VM sieht nur die freigegebenen Wurzelordner, nicht das ganze Benutzerverzeichnis |

Ergebnisse aus Phase 0 zu Bind-Mounts unter Windows:

- **Performance:** 9p kostet 2–3 ms pro Dateioperation (WSL wie Hyper-V). `git status` in einem Repo mit 2.866 Dateien braucht 5,5 s statt 0,02 s.
- **Funktion (Hyper-V):** `chmod`/`copyfile` → EPERM, rekursives Löschen → EIO.
- **Dateiänderungen:** inotify-Ereignisse vom Host kommen im Container nicht an, nur Polling funktioniert.
- **Gemeinsamer Index:** Windows- und Linux-git am selben `.git` erzwingen ein komplettes Rehash, sofern nicht `core.checkStat=minimal` und `core.trustctime=false` gesetzt sind (gehört ins Base-Image).
- **Zeilenenden:** CRLF und LF können sich vermischen, deshalb `.gitattributes` mit `* text=auto eol=lf`.
- **Freigaben:** Sie lassen sich nur bei `machine init` festlegen. Ohne Angabe gibt Podman `C:\` und das Benutzerprofil frei. Im Isoliert-Modus bekommt die Machine deshalb **keine** Freigaben.

### 3.5 Netzwerk

- Jede Instanz hängt in einem eigenen `--internal`-Netz ohne Route nach außen. Nur der `gateway`-Container ist zusätzlich mit einem externen Netz verbunden.
- Sämtlicher Verkehr läuft über das Gateway, einen HTTP(S)-Proxy mit `CONNECT`. Die Filterung nach Domain nutzt den CONNECT-Host bzw. SNI, TLS wird dafür nicht aufgebrochen. DNS löst nur das Gateway auf.
- Instanzen sehen sich gegenseitig nicht.

| Voreinstellung | Erlaubt | Einsatz |
|---|---|---|
| **Aus** | nichts | maximale Abschottung |
| **Entwicklung** (Standard) | Paketquellen (npm, PyPI, crates.io …), Git-Hoster, Doku-Seiten | normales Arbeiten |
| **Offen** | das gesamte öffentliche Internet | Web-Recherche |
| **Benutzerdefiniert** | eigene Allow- und Deny-Listen | Sonderfälle |

**Immer gesperrt,** auch bei „Offen“: der Host, das lokale Netz (RFC 1918), CGNAT (`100.64.0.0/10`, darunter Tailscale), Link-Local-Adressen inklusive Cloud-Metadaten (`169.254.169.254`) und andere Instanzen. Ausnahmen sind nur explizite Systemziele, etwa das LLM-Gateway des lokalen Servers. Das Gateway löst DNS selbst auf, prüft die **Ziel-IP** und verbindet sich mit genau dieser IP. Ein reiner Domain-Filter reicht nicht (Phase 0, 6.4).

**Vorsicht, gvproxy (Hyper-V):** `192.168.127.254` bzw. `host.containers.internal` wird auf `127.0.0.1` des Windows-Hosts weitergeleitet. Aus jedem normalen Container-Netz, auch aus dem Gateway selbst, sind damit Dienste erreichbar, die der Nutzer „nur lokal“ betreibt (gemessen: Harness-Server `:8080`, SMB `:445`). Instanzen hängen deshalb **ausschließlich** in internen Netzen, und das Gateway sperrt `192.168.127.0/24` ausdrücklich (Phase 0, 12.6).

Die UI zeigt ein Live-Protokoll aller Verbindungen. Blockierte Domains lassen sich dort mit einem Klick freigeben. Wichtig: Bei „Offen“ kann ein per Prompt-Injection manipulierter Agent alles nach außen schicken, was er im Container sieht. Daher die versteckten Pfade, und keine Secrets im Container.

Implementierung: ein eigener kleiner Proxy in Rust (tokio/hyper). Er lädt Richtlinien ohne Neustart nach und streamt sein Protokoll an `harnessd`. Später kann er TLS **nur** für LLM-API-Hosts terminieren, um API-Keys erst im Gateway einzusetzen (Secret-Injection).

### 3.6 Git-Worker

Git-Operationen, die etwas verändern, laufen nie im Container, sondern im Worker auf dem Host. Dort liegen auch die Zugangsdaten (Git Credential Manager). Der Agent bekommt sie nie zu sehen.

```
Agent: git commit -m "…"
  → git-Shim im Container: argv parsen, NICHT als Shell-String weiterreichen
  → strukturierte Anfrage {op: "commit", message: "…"} mit Instanz-Token
  → gateway → harnessd → Richtlinienprüfung
  → Ausführung auf dem Host mit gehärteter Konfiguration
  → Ergebnis bzw. verständliche Fehlermeldung zurück an den Agent
```

- Lesende Befehle (`status`, `diff`, `log`, `show`, `blame`) laufen direkt im Container gegen das read-only `.git` (`GIT_OPTIONAL_LOCKS=0`).
- Schreibende Befehle kennt der Shim über eine Allowlist aus Unterbefehlen und Flags. Alles Unbekannte lehnt er mit einem Hinweis ab, welche Befehle erlaubt sind. Agents kommen damit gut zurecht.
- Der Shim heißt `git`. Harnesse brauchen also keine Anpassung.
- Bei Ausführungen auf dem Host sind Hooks deaktiviert (`core.hooksPath` auf ein leeres Verzeichnis) und `core.fsmonitor` ist aus. Erlaubte Protokolle sind eingeschränkt, Submodule-Operationen gesperrt. Hintergrund: Ein pre-commit-Hook würde vom Agent geschriebene Linter-Konfiguration auf dem Host ausführen. Hooks können optional *im Container* laufen.

Richtlinien pro Projekt, in der UI einstellbar:

| Operation | Standard |
|---|---|
| commit, add, stash | erlauben |
| Branch anlegen | erlauben |
| Branch wechseln | nachfragen (im Direkt-Modus ändert das auch die Arbeitskopie des Nutzers) |
| push auf `agent/*` | erlauben |
| push auf andere Branches | nachfragen |
| force-push, `reset --hard`, `clean -fd` | verbieten |
| `config`, Submodule, Remotes ändern | verbieten |

„Nachfragen“ löst eine Desktop-Benachrichtigung mit Diff bzw. Commit-Liste aus. Jede Operation landet im Audit-Log.

### 3.7 Harnesse

Harnesse werden **ohne Code** über TOML-Dateien definiert:

```toml
# ~/.harness/harnesses/pi.toml
name = "pi"
description = "Minimaler Coding-Agent mit vielen LLM-Anbietern"
install = "npm install -g @earendil-works/pi-coding-agent"
command = ["pi"]
persist = ["~/.pi"]            # Logins/Einstellungen, pro Nutzer geteilt

[adapter]                      # optional, nur für die Anbindung an den Web-Chat
kind = "pi-rpc"
command = ["pi", "--mode", "rpc"]
```

Ablauf von `harness opencode` in `C:\Projekte\foo`:

1. Projekt anhand des aktuellen Ordners suchen. Ist es unbekannt, kurz bestätigen lassen („Neues Projekt, Umgebung *default*, Netzwerk *Entwicklung* – OK?“) und eine Projektdatei anlegen.
2. Prüfen, ob der Ordner unter einer freigegebenen Wurzel liegt. Falls nicht, anbieten, ihn hinzuzufügen.
3. Image und Instanz sicherstellen, also bauen bzw. starten, Harness bei Bedarf installieren.
4. `podman exec -it -w /workspace harness-foo opencode`. Das TTY wird direkt durchgereicht, inklusive Farben und Resize.

Weitere Befehle: `harness shell`, `harness status`, `harness stop`, `harness update <harness>`, `harness open` (öffnet das Projekt in der Desktop-App).

LLM-Zugang der Harnesse: entweder deren eigener Login (persistiert im Volume) oder, bei verknüpften Chats, das LLM-Gateway des Servers (siehe 6.4).

### 3.8 Konfiguration auf dem Host

Alles liegt lesbar in `~/.harness/` (Windows: `%USERPROFILE%\.harness\`). Die Dateien sind die maßgebliche Quelle. Die UI liest und schreibt sie mit `toml_edit`, das Kommentare und Formatierung erhält. `harnessd` beobachtet die Dateien, und Fehler in manuellen Änderungen werden in der UI angezeigt.

```
~/.harness/
├─ config.toml                  Server-URL, freigegebene Wurzelordner, Standards
├─ environments/
│  └─ default/
│     ├─ environment.toml       Base-Image, Ressourcen, Setup-Befehle
│     └─ Containerfile          optional
├─ harnesses/                   opencode.toml, pi.toml, claude.toml, codex.toml …
├─ projects/                    eine Datei pro Projekt
└─ logs/                        Netzwerk- und Git-Audit-Logs
```

```toml
# ~/.harness/projects/foo.toml
path = 'C:\Projekte\foo'
environment = "default"
default_harness = "opencode"
remote_access = false            # Darf ein verknüpfter Chat diese Umgebung nutzen?

[files]
mode = "direct"                  # direct | isolated
hidden = [".env", ".env.*"]
read_only = [".vscode", ".github/workflows", ".envrc"]
container_volumes = ["node_modules", ".venv"]

[network]
preset = "development"           # off | development | open | custom
allow = []
deny = []

[git]
commit = "allow"                 # allow | ask | deny
switch_branch = "ask"
push = { agent_branches = "allow", other = "ask", force = "deny" }

[resources]
cpus = 4
memory = "8g"
```

**Regel:** Sicherheitsrelevante Einstellungen liegen **nie** im Projektordner. Der ist für den Agent beschreibbar, und sonst könnte er seine eigenen Richtlinien ändern.

## 4. Server

| Thema | Entscheidung |
|---|---|
| Laufzeit | Node.js LTS + TypeScript, HTTP-Framework Hono |
| Datenbank | Kysely: SQLite (better-sqlite3) oder Postgres (pg). Der Dialekt ergibt sich aus `DATABASE_URL` |
| Auth | Better Auth: E-Mail/Passwort, Rollen (Admin/Nutzer), Admin-Bereich, später OIDC. Das Device-Authorization-Plugin übernimmt das Pairing (Version ≥ 1.6.11 wegen CVE-2026-45337) |
| LLM | Vercel AI SDK mit Anthropic, OpenAI, Google, Mistral und OpenAI-kompatiblen Anbietern (Ollama, LM Studio, OpenRouter, vLLM) |
| Streaming | SSE zum Browser, WebSocket für den Geräte-Tunnel |
| Secrets | API-Keys der Nutzer mit AES-256-GCM verschlüsselt in der DB. Der Schlüssel wird per HKDF aus dem Server-Secret abgeleitet (`HARNESS_SECRET` oder eine beim ersten Start erzeugte Datei im Datenverzeichnis) |

Mehrbenutzerbetrieb:

- **Jeder Nutzer hinterlegt seine eigenen API-Keys.** Modelle werden pro Key direkt beim Anbieter abgefragt. Es gibt keine Server-Keys.
- Admins verwalten Nutzer, den Registrierungsmodus (offen, geschlossen, später: Einladung) und Server-Einstellungen. Die Keys der Nutzer sehen sie nicht.
- Eigene Basis-URLs (Ollama, OpenAI-kompatibel) dürfen Admins immer setzen, normale Nutzer nur, wenn der Admin das freigibt. Sonst könnten Nutzer den Server Anfragen ins interne Netz schicken lassen (SSRF).
- Der erste registrierte Nutzer wird automatisch Admin.
- Nutzer sehen nur ihre eigenen Chats, Keys und Geräte.

Regeln für die Portabilität zwischen SQLite und Postgres:

- IDs als Text (UUIDv7), Zeitstempel als Epoch-Millisekunden, JSON als Text, der in der App geparst wird.
- Kein dialektspezifisches SQL außerhalb eines kleinen Adapter-Moduls.
- Migrationen mit dem Schema-Builder von Kysely.
- CI testet gegen **beide** Datenbanken.

Grobes Datenmodell: Nutzer, Sitzungen und Accounts (Better Auth), `providers`, `models`, `conversations`, `messages` (Parts als JSON), `devices`, `chat_links`, `usage`.

Deployment:

- Docker-Image (multi-stage) plus `compose.yaml` mit den Profilen `sqlite` (Standard, Volume `/data`) und `postgres`.
- Konfiguration komplett über Umgebungsvariablen.
- Für Remote-Betrieb hinter einen Reverse Proxy mit TLS.
- Ohne verknüpfte Geräte ist der Server ein ganz normaler Chat-Server. Die Gerätefunktionen bleiben einfach ungenutzt.

## 5. Desktop

- **`harness-vmd` (Windows-Dienst):** Podmans Hyper-V-Provider verlangt für jeden Machine-Befehl Admin-Rechte (Phase 0, Abschnitt 12). Diese Aufgabe übernimmt ein kleiner privilegierter Dienst, den der Installer einmal einrichtet:
  - Er besitzt die VM-Konfiguration (Freigaben, Ressourcen) in `%ProgramData%`, die nur Admins ändern können, und prüft jede Freigabe gegen eine Regel.
  - Nach außen bietet er nur wenige Befehle über eine Named Pipe an, die ausschließlich der eingerichtete Nutzer erreicht: anlegen, starten, stoppen, Status.
  - Er setzt dynamischen Speicher mit kleinem Startwert, räumt nach fehlgeschlagenen Starts auf und startet die VM nach Standby neu.
  - Entschieden gegen eine geplante Aufgabe: Ein elevierter Podman würde eine Konfiguration aus dem Benutzerprofil lesen, die jeder Nutzerprozess ändern kann. Das wäre eine Rechteausweitung.
- **`harnessd`:** Rust-Dienst, startet bei der Anmeldung oder bei Bedarf. Er ist der einzige Prozess, der Podman steuert, und hält Git-Worker, Gateway-Steuerung und Tunnel. IPC läuft über eine Named Pipe (Windows) bzw. einen Unix-Socket.
- **Tauri-App:** Oberfläche für `harnessd` und den Chat. Wird das Fenster geschlossen, bleibt nur das Tray-Symbol, und die WebView wird freigegeben. Terminal-Sitzungen und verknüpfte Chats laufen über `harnessd` weiter.
- **Ohne Server:** Umgebungsverwaltung und Terminal-Modus funktionieren vollständig. Für den Chat braucht es einen Login auf einem Server.
- **Setup-Assistent:**
  1. Virtualisierung prüfen
  2. Podman installieren (`winget`)
  3. Nutzer zu den „Hyper-V-Administratoren“ hinzufügen (einmal UAC)
  4. Machine `harness` anlegen
  5. Base-Image bauen
  6. Wurzelordner wählen
- **RAM** (Schätzung, in Phase 0 messen): `harnessd` etwa 10–30 MB. Die App mit offenem Fenster etwa 80–150 MB, vor allem WebView2. Die VM belegt RAM dynamisch.

## 6. Chat ↔ Umgebung

### 6.1 Pairing

1. Die Desktop-App fordert einen Gerätecode an (RFC 8628).
2. Der Nutzer bestätigt den Code im Web.
3. Die App erhält ein Geräte-Token. Es lässt sich im Web unter „Meine Geräte“ widerrufen.

### 6.2 Tunnel

`harnessd` baut eine WSS-Verbindung zum Server auf, ausgehend, also ohne Port-Freigaben. Er akzeptiert **nur** diese Nachrichtentypen:

| Richtung | Nachricht |
|---|---|
| Server → Gerät | `projects.list` (nur Projekte mit `remote_access = true`), `session.open`, `session.input`, `session.cancel`, `session.close` |
| Gerät → Server | `hello`, `projects`, `session.event`, `session.closed` |

Es gibt **kein** generisches „Befehl ausführen“ und keinen direkten Dateizugriff für den Server. Selbst ein kompromittierter Server kann daher nur Harness-Sitzungen in freigegebenen Projekten steuern, und diese laufen in der Sandbox.

### 6.3 Verknüpfung

1. Ein neuer Chat ist **nicht** verknüpft. Er läuft als klassischer Chat über den Server (Modus A) und hat keine Tools für Umgebungen.
2. „Mit Umgebung verbinden“ im Chat-Kopf führt zur Auswahl: Gerät, dann Projekt (nur freigegebene), dann Harness (nur solche mit Adapter).
3. Die erste Verknüpfung eines Chats mit einem Projekt muss in der Desktop-App bestätigt werden. Optional lässt sich „für eigene Chats automatisch erlauben“ einstellen.
4. Die Verknüpfung ist im Chat sichtbar und jederzeit lösbar. Die Desktop-App zeigt alle aktiven Verknüpfungen und hat einen Not-Aus.

### 6.4 Agent im Container

- Ein verknüpfter Chat startet eine Harness im Container im Maschinenmodus. Der Adapter in `harnessd` übersetzt deren Ausgaben in einheitliche `AgentEvent`s. Der Server speichert sie und streamt sie an die UI. Kontextverwaltung, Tools und Edits übernimmt die Harness.
- **Erster Adapter: pi** (`pi --mode rpc`, JSONL über stdin/stdout). Das passt direkt auf `podman exec -i`, ein Port ist nicht nötig. **Zweiter Adapter:** opencode (`opencode serve`, HTTP/OpenAPI).
- **LLM-Zugang über das LLM-Gateway des Servers:**
  - Die Harness bekommt pro Anbieter eine Basis-URL des Servers (`/llm/anthropic/*`, `/llm/openai/*` …) und ein kurzlebiges Sitzungs-Token.
  - Der Server reicht die Anfragen 1:1 durch und setzt dabei den API-Key des Nutzers ein, dem der Chat gehört.
  - Ergebnis: dieselben Modelle wie im Web-Chat, keine Anbieter-Keys im Container, Verbrauch pro Nutzer erfasst.
- Wird ein bereits laufender Chat verknüpft, bekommt die Harness den bisherigen Verlauf als Kontext im ersten Prompt.
- `AgentEvent`-Typen: `text.delta`, `reasoning.delta`, `tool.start`, `tool.output`, `tool.end`, `file.changed`, `usage`, `error`, `turn.end`. Die UI rendert Tool-Aufrufe als Terminal-Blöcke bzw. Diffs.

## 7. Frontend

- **Stack:** React 19 + TypeScript + Vite, Tailwind CSS v4, shadcn/ui (Radix), Motion für Animationen, TanStack Router und Query, Shiki für Code, Markdown-Streaming, xterm.js, Diff-Ansicht.
- **Eine SPA für beide:** Sie wird vom Server ausgeliefert und in der Tauri-App gebündelt. Desktop-Funktionen werden per Capability-Erkennung eingeblendet.
- **Design:** hell und dunkel, ruhige Animationen (Nachrichten einblenden, Streaming-Cursor, Layout-Übergänge). `prefers-reduced-motion` wird beachtet.
- **Desktop-Ansichten:**
  - Übersicht: Instanzen, Status, Ressourcen
  - Projekt mit Tabs: Übersicht, Netzwerk (Voreinstellung, Listen, Live-Protokoll), Git (Richtlinien, offene Anfragen, Audit-Log), Dateien (versteckte und geschützte Pfade), Harnesse, Logs
  - Chats, Geräte/Server, Einstellungen

## 8. Repository-Struktur

```
llm_harness/
├─ apps/
│  ├─ server/          Hono, Kysely, Better Auth, AI SDK
│  ├─ web/             React-SPA
│  └─ desktop/         Tauri 2 (Frontend = apps/web)
├─ crates/
│  ├─ harness-core/    Konfiguration, Provider (Podman-CLI über SSH), Instanzen, Richtlinien
│  ├─ harnessd/        Dienst: IPC, Git-Worker, Tunnel, Adapter
│  ├─ harness-cli/     Kommando `harness`
│  └─ gateway/         Egress-Proxy (läuft als Container in der VM)
├─ packages/
│  ├─ shared/          API-Typen und Schemas (Server ↔ Web)
│  └─ protocol/        Tunnel-Nachrichten, AgentEvent (Typen aus Rust per ts-rs generiert)
├─ images/
│  ├─ base/            Containerfile
│  └─ shims/           git-Shim
├─ spikes/             Wegwerf-Experimente (Phase 0)
└─ docs/
   ├─ design/          Design-System: Regeln, Tokens, Komponenten
   └─ spikes/          Ergebnisse der Spikes
```

pnpm-Workspaces für TypeScript, ein Cargo-Workspace für Rust. Ein eigenes `packages/ui` ist nicht nötig, weil die Desktop-App `apps/web` direkt als Frontend nutzt. Das Design-System liegt in `apps/web/src/styles` bzw. `apps/web/src/components/ui`, die Regeln dazu in `docs/design/DESIGN.md`.

## 9. Phasen

Der Phasenplan mit Status, Arbeitspaketen und Abnahmekriterien steht in [`docs/ROADMAP.md`](docs/ROADMAP.md). Der aktuelle Arbeitsstand für die nächste Sitzung steht in [`docs/PROGRESS.md`](docs/PROGRESS.md).

## 10. Abgrenzung und Mehrwert

| Bestehende Lösung | Kann | Fehlt für dieses Ziel |
|---|---|---|
| Docker Sandboxes | CLI-Agents in microVMs, Netzwerk-Allowlist | an Docker Desktop gebunden, kein Chat, keine Git-Richtlinien |
| Claude Code Remote Control | lokale Claude-Code-Sitzung aus Browser oder App steuern | nur Claude Code, keine Sandbox, nicht selbst gehostet |
| opencode serve/web | Browser-UI für eine opencode-Sitzung | nur eine Harness, keine Sandbox-Verwaltung, kein Multi-User |
| OpenHands | Web-Chat + Agent in Docker-Sandbox | eigener Agent statt freier Harness-Wahl |
| Open WebUI, LibreChat | selbst gehosteter Multi-User-Chat mit vielen Anbietern | keine lokalen Sandboxes |

Was es so noch nicht gibt:

- Die **Kombination** aus selbst gehostetem Multi-User-Chat, vom Nutzer selbst verknüpften lokalen Sandboxes, freier Wahl der Harness und einer UI für Netzwerk- und Git-Richtlinien.
- Windows zuerst und ohne Docker Desktop.

Der stärkste eigenständige Teil ist der Sandbox-Manager (Phasen 2–3). Der klassische Chat-Server überschneidet sich am meisten mit bestehenden Projekten. Dort lohnt es sich, keine Feature-Parität anzustreben.

## 11. Offene Punkte und Risiken

- **9p-Performance unter Windows** ist das Hauptrisiko für den Direkt-Modus. Fallback: Sync-Modus.
- Freigegebene Wurzelordner der Podman Machine nachträglich ändern.
- Admin-Rechte beim Hyper-V-Provider so wenig wie möglich anfordern.
- Die Schnittstellen der Harnesse (pi-RPC, opencode-API) ändern sich. Adapter müssen versioniert und getestet werden.
- Remote-Betrieb: TLS, Rate-Limits, Rotation der Geräte-Tokens, Audit.
- Endgültigen Namen festlegen.
