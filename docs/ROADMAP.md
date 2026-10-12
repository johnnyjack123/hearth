# Roadmap

> Stand 12.10.2026. Architektur und Begründungen: [`ARCHITECTURE.md`](../ARCHITECTURE.md). Aktueller Arbeitsstand: [`PROGRESS.md`](PROGRESS.md).
> Grundsatz: **ein** Sandbox-Provider (Podman + Hyper-V + Linux-VM, Isoliert-Modus), bis er im Alltag trägt. Weitere Provider kommen erst danach.

| Phase | Inhalt | Status |
|---|---|---|
| 0 | Spike Podman WSL und Hyper-V (I/O, Netz, Sicherheit) | ✅ erledigt, [`spikes/phase-0.md`](spikes/phase-0.md) |
| 0b | Spike Windows Sandbox | ✅ abgebrochen und verworfen, [`spikes/phase-0b-windows-sandbox.md`](spikes/phase-0b-windows-sandbox.md) |
| 1 | Server + Web-Chat (Multi-User, Keys pro Nutzer, Streaming) | ✅ im Kern fertig, Reste siehe unten |
| **2** | **Sandbox-Kern + `harness`-CLI (Terminal-Modus mit pi)** | **🔨 in Arbeit**, Pakete 1–5 erledigt |
| 2b | Windows-Dienst `harness-vmd` (VM-Lebenszyklus ohne Admin-Fenster) | offen |
| 3 | Desktop-App (Tauri) + Rust-Gateway + UI für Netz- und Git-Regeln | offen |
| 4 | Chat ↔ Umgebung (Pairing, Tunnel, pi-RPC-Adapter, LLM-Gateway) | offen |
| 5 | Ausbau (Host-Modus, Sync-Modus, weitere Harnesse und Provider …) | offen |

## Phase 1 – Reste

- [ ] Server-`Dockerfile` (multi-stage, multi-arch) und `compose.yaml` (Profile `sqlite`/`postgres`). War geplant, ist **nicht** gebaut.
- [ ] ESLint- und Prettier-Konfiguration anlegen. Die Pakete sind installiert, es gibt aber keine Config-Dateien. Danach `pnpm lint` grün machen.
- [ ] `README.md` (Kurzbeschreibung, Start, Verweise).
- [ ] Postgres-Testlauf nach den letzten Server-Änderungen wiederholen (Regenerate, `/providers/:id/models`). Zuletzt grün waren 46/46 im Spike, seitdem nur SQLite (30/30).
- [ ] Bundle-Größe der Web-App: Haupt-Chunk ~955 kB (295 kB gzip), per `manualChunks` aufteilen.
- [ ] Branch `feat/phase-1-chat` nach `main` mergen (Nutzer fragen).

## Phase 2 – Sandbox-Kern und `harness`-CLI

**Ziel:** Im Ordner eines Git-Repos auf dem Host `harness run pi` aufrufen. pi läuft dann in einem Container in der Linux-VM, auf einer Arbeitskopie im Volume, mit Netz nur über das Gateway. Danach holt `harness sync` die Commits als Branch `agent/<projekt>` ins Host-Repo, und `harness merge` führt sie automatisch zusammen, wenn es konfliktfrei geht.

**Abnahme:** In einem Test-Repo legt `harness run pi` die Instanz an und startet pi interaktiv. pi committet eine Änderung, `harness sync` und `harness merge` bringen sie konfliktfrei in den Host-Branch. Ein `curl example.com` im Container wird vom Gateway geblockt, `npm view react version` geht durch. Alles läuft **ohne** Admin-Rechte, solange die VM läuft.

Arbeitspakete in sinnvoller Reihenfolge:

1. ✅ **Cargo-Workspace:** `Cargo.toml` im Repo-Root, `crates/harness-core` (Bibliothek) und `crates/harness-cli` (Binary `harness`). Toolchain `aarch64-pc-windows-msvc` ist installiert. RAM ist knapp, deshalb `jobs = 2` in `.cargo/config.toml` und `debug = "line-tables-only"`. Im Spike stürzte rustc bei Release-Builds ab, vermutlich wegen RAM. Der Debug-Build braucht ~22 s.
2. ✅ **Provider-Schnittstelle** in `harness-core` (`provider.rs`), zunächst nur `PodmanProvider` (`podman/`). Verifiziert mit `tests/podman_vm.rs` gegen die echte VM. **Offen:** exec mit TTY ist gebaut, aber noch nicht interaktiv getestet (kommt mit `harness shell` in Paket 8).
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
   - **Offen für Paket 8:** CLI-Befehle `sync` und `merge`. `in.bundle` ist jedes Mal vollständig, bei großen Repos ggf. inkrementell machen.
6. **Netzwerk:**
   - pro Projekt ein `--internal`-Netz
   - ein Gateway-Container in internem **und** normalem Netz, vorerst tinyproxy aus `spikes/phase-0/proxy/`
   - Voreinstellung „Entwicklung“: npm, PyPI, GitHub inklusive `codeload`/`objects.githubusercontent.com` **plus die LLM-APIs, die pi braucht**. Der Nutzer verwendet **DeepSeek** (`api.deepseek.com`). Weitere Anbieter (z. B. `api.anthropic.com`, `api.openai.com`, `openrouter.ai`) nur auf Wunsch
   - corepack lädt pnpm 12 beim ersten Aufruf als Binary herunter („Downloading the pnpm 12.11.2 binary“). Prüfen, von welchem Host (npm-Registry oder GitHub), und in die Allowlist aufnehmen
   - Gateway-Container braucht eine eigene Provider-Methode, weil `create_container` nur interne Netze zulässt. Netzwerk-Alias `gateway`, Port 8888 (so im Base-Image als `HTTP(S)_PROXY` gesetzt)
   - Instanzen **nie** im normalen Netz, weil gvproxy dort Host-localhost durchreicht
7. **Harness-Definition pi:**
   - `harnesses/pi.toml`, Installation `npm install -g @earendil-works/pi-coding-agent` in ein gemeinsames Tools-Volume
   - Login und Einstellungen (`~/.pi`) in einem Volume pro Nutzer
   - Start mit `podman exec -it -w /workspace … pi`
8. **CLI-Befehle:** `run <harness>`, `shell`, `status`, `stop`, `sync`, `merge`, `doctor` (prüft VM, SSH, Image und Gateway).
9. **Tests:** Rust-Unit-Tests für Konfiguration, Bundle- und Merge-Logik (gegen temporäre Repos) und Allowlist-Parsing, dazu ein Smoke-Skript gegen die echte VM.
10. **Doku:** `CLAUDE.md` (Befehle, Stolpersteine) und `PROGRESS.md` aktualisieren.

## Phase 2b – `harness-vmd`

Anforderungen stehen in `spikes/phase-0.md` §12.3:
- Konfiguration in `%ProgramData%`
- Named Pipe nur für den eingerichteten Nutzer
- dynamischer Speicher mit kleinem Startwert
- Aufräumen nach gescheitertem Start (Ignition-KVP, vsock-Registry)
- Watchdog nach Standby

Der Installer richtet den Dienst einmal mit UAC ein.

## Phase 3 – Desktop-App und Gateway

- Tauri 2 als Hülle, Frontend ist `apps/web` mit Desktop-Funktionen per Capability-Erkennung
- Prozess `harnessd` (IPC) für CLI und App gemeinsam
- Rust-Gateway: eigene DNS-Auflösung, Ziel-IP-Prüfung, Sperre für private Bereiche, CGNAT, Link-Local und `192.168.127.0/24`, Live-Protokoll, Voreinstellungen
- UI für Netz-Allowlist, Git-Regeln, Projekte und Harnesse (ARCHITECTURE.md §5, §7)

## Phase 4 – Chat ↔ Umgebung

Pairing per Device Flow (Better Auth `deviceAuthorization`), WSS-Tunnel, Projektfreigabe mit lokaler Bestätigung, **pi-RPC-Adapter** (`pi --mode rpc` über `podman exec -i`), `AgentEvent`-Rendering im Chat, LLM-Gateway im Server mit den Keys des Nutzers (ARCHITECTURE.md §6).

## Phase 5 – Ausbau (ohne feste Reihenfolge)

- **Host-Modus** (ungesichert, Harness direkt auf dem Host, ARCHITECTURE.md §3)
- **Sync-Modus** (Live-Spiegelung des Volumes in einen Host-Ordner)
- Git-Worker mit Shim und Richtlinien wie in ARCHITECTURE.md §3.6, Checkpoints
- weitere Harnesse (opencode, Claude Code, Codex) und Adapter
- weitere Provider: Docker/Podman nativ unter Linux mit gVisor/krun, macOS, Windows Sandbox für Windows-Builds auf Rechnern mit mehr RAM
- Secret-Injection im Gateway, OIDC, Rate-Limits, Remote-Härtung
