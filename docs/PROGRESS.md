# Progress

_Last updated: 2026-10-12 · Phase 2 Pakete 1–3 fertig (Cargo-Workspace, PodmanProvider, Base-Image), weiter mit Paket 4 (Konfiguration)_

## Where we are

Server (`apps/server`) und Web-App (`apps/web`) aus Phase 1 laufen unverändert. Neu ist der Rust-Workspace mit `crates/harness-core` (Provider-Schnittstelle, `PodmanProvider`, eingebettetes Base-Image) und `crates/harness-cli` (Binary `harness`). `harness doctor` prüft Machine-JSON, Austauschordner, SSH/VM und Base-Image. `harness image build` baut `localhost/harness/base:latest` in der VM (Erstbau 58 s, mit Cache 7 s). Instanzen, Sync, Gateway und pi gibt es noch nicht. Die CLI kann also noch keinen Agent starten.

## Next step

Paket 4 der ROADMAP (Konfiguration), danach 5 (Git-Sync):

1. `crates/harness-core/src/config.rs`: `%USERPROFILE%\.harness\config.toml`, `projects/<name>.toml`, `harnesses/<name>.toml` mit `serde` + `toml` lesen und mit `toml_edit` schreiben (Kommentare bleiben erhalten). Format wie ARCHITECTURE.md §3.8, aber `mode = "isolated"`. Projektname aus dem Ordnernamen ableiten und mit `provider::validate_resource_name` prüfen bzw. bereinigen. Neue Crates `toml`/`toml_edit` sind ein kleiner Download.
2. Instanz-Logik (Namen `harness-<projekt>`, Netz `harness-net-<projekt>`, Volumes für Workspace/Home/Tools) auf `Provider` aufsetzen. Der Austauschordner bekommt pro Projekt einen Unterordner (`MountSource::Exchange("<projekt>")`), damit Projekte die Bundles der anderen nicht sehen.
3. Git-Sync (Paket 5) mit Bundles über `ExchangeDir::put_file`, Tests gegen temporäre Repos.

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alle vier Zeilen ✓, solange die VM läuft).

## In flight / half-finished

- `PodmanProvider::exec` mit `tty: true` ist gebaut, aber **nicht interaktiv getestet**. Aus der Agent-Shell geht das nicht, das kommt mit `harness shell` (Paket 8) und muss der Nutzer in Windows Terminal ausprobieren. Ctrl+C und Fenstergröße sind ungeprüft.
- Doku-Änderungen und der gesamte Rust-Code sind **uncommittet** (Branch `feat/phase-1-chat`), ebenso die Doku-Änderungen der Sitzung davor.

## Decisions made this session

- **podman-CLI statt bollard/REST:** Das ist der verifizierte Weg aus dem Spike (`pm.sh`). Es braucht keine Named Pipe und keinen eigenen SSH-Client. Preis: ~350–500 ms pro Aufruf.
- **Sicherheitsinvarianten im Provider erzwungen** (nicht konfigurierbar): Härtung nach ARCHITECTURE §3.3, nur interne Netze (per `network inspect` geprüft), Mounts nur als Volume oder Austausch-Unterordner, alle Namen und Pfade validiert, Managed-Label auf allem. Rootful-Machines werden abgelehnt.
- **`--userns=keep-id:uid=1000,gid=1000` + `--user 1000:1000`** statt `--user agent`: Das funktioniert unabhängig von der UID des VM-Nutzers und auch für Projekt-Images ohne Nutzer `agent`.
- **`--init` (catatonit vom VM-Host)** statt tini im Image, damit verwaiste exec-Prozesse auch bei eigenen Projekt-Images aufgeräumt werden.
- **Image-Kontext ins Binary eingebettet** (`include_str!`) mit Content-Hash-Label, damit die CLI ohne Repo-Checkout baut und `doctor` veraltete Images erkennt.
- **Node aus `node:24-trixie-slim` kopiert** statt NodeSource oder Tarball: Major gepinnt, multi-arch, ohne fremde apt-Quelle. Node 24, weil es LTS ist und corepack noch mitbringt.
- **pnpm wird nicht ins Image gebacken:** corepack lädt es beim ersten Aufruf ins Home-Volume, weil die Root-FS read-only ist und Projekte eigene pnpm-Versionen pinnen.
- **`gh` nicht im Image** (ARCHITECTURE §3.2 nennt es): Es gibt keine Zugangsdaten im Container, und die ROADMAP-Liste enthält es nicht.
- **Git-Identität im Image:** `harness agent <agent@harness.invalid>`, damit Agent-Commits im Host-Repo erkennbar sind.
- **LLM-Anbieter für pi: DeepSeek** (`api.deepseek.com`, vom Nutzer gewählt) → Gateway-Allowlist in Paket 6.

## Open questions for the user

- Committen? Vorschlag: zwei Commits auf `feat/phase-1-chat` oder einem neuen Branch `feat/phase-2-sandbox`: `docs: …` für die Doku der Vorsitzung, `feat(core): add podman provider and base image` für den Rust-Code. Und `feat/phase-1-chat` nach `main` mergen?
- Für pi mit DeepSeek: API-Key per pi-Login im Home-Volume (`~/.pi`) oder später über das LLM-Gateway des Servers (Phase 4)? Für Phase 2 reicht der pi-Login.

## Known issues / debt

- **Hoch:** VM-Start nur per Admin-PowerShell (→ Phase 2b). Nach einem gescheiterten Start muss von Hand aufgeräumt werden, siehe `spikes/phase-0.md` §12.2. Die vsock-Registry-Einträge **nicht** löschen.
- **Mittel:** ~350–500 ms pro podman-Aufruf (neue SSH-Verbindung). `harness run` braucht geschätzt 6–10 Aufrufe, also 3–5 s Start. Falls störend: libpod-REST über einen gehaltenen SSH-Tunnel.
- **Mittel:** CLI-Texte nur Deutsch, Englisch fehlt (Web-UI ist zweisprachig).
- **Mittel:** kein Dockerfile/compose für den Server, keine ESLint/Prettier-Konfiguration, kein README (ROADMAP „Phase 1 – Reste“). Web-Haupt-Chunk ~955 kB.
- **Niedrig:** Host-Key der VM per Trust on first use (`ssh-keyscan` beim ersten Aufruf). Der Port ist nur an `127.0.0.1` gebunden. Bei neuem Host-Key mit gleichem Port `%LOCALAPPDATA%\llm-harness\podman\harness-<port>\` löschen.
- **Niedrig:** Podman-Hilfsprozesse laufen im Kontext des Admin-Fensters. Schließt der Nutzer es, ist das Verhalten ungeprüft.
- Rechner-Stabilität: ein Bluescreen `0xA0` beim Start der Windows Sandbox, ein Abbruch der WSL-VM bei Standby. Virtualisierung auf diesem ARM64-Rechner mit Vorsicht behandeln, Nutzer vor riskanten Starts fragen.

## Verification state

- `cargo build` (22 s), `cargo fmt --check`, `cargo clippy --all-targets`: ohne Warnungen. `cargo test`: 24/24 Unit-Tests grün.
- `tests/podman_vm.rs` (alpine, echte VM): grün. Geprüft:
  - Härtung: UID/GID 1000, `CapEff` 0, `NoNewPrivs` 1, Root read-only, `pids.max` 1024, PID 1 `podman-init`, `/tmp` beschreibbar und ausführbar
  - kein IP-Zugriff, kein externes DNS
  - Ablehnung eines nicht-internen Netzes und von `--privileged` als Name
  - Volume, Workdir, Env mit Sonderzeichen
  - Austauschordner in beide Richtungen, read-only-Mount
  - exec-Exit-Codes, Stop/Start mit erhaltenen Daten
  - aufgeräumt auch nach einem fehlgeschlagenen Lauf
- `tests/base_image_vm.rs` (gehärtete Instanz, Login-Shell): grün. Geprüft:
  - git 2.47.3, Node 24.21.0, npm 11.19.0, corepack 0.36.0, Python 3.13.5, uv 0.12.24, rg 14.1.1, fd 10.2.0, jq 1.7, gcc 14.2, make 4.4.1, curl 8.14.1
  - frische Volumes gehören `agent`, Skeleton im Home
  - npm-Prefix `~/.local`, Git-Systemconfig, Commit als „harness agent“
  - Login-Shell-PATH, Proxy-Variablen
- Von Hand (Wegwerf-Container im normalen Netz):
  - pnpm 12.11.2 über corepack, ohne Auto-Pin
  - Node-`fetch` nutzt den Proxy nur mit `NODE_USE_ENV_PROXY=1` (mit: `ENOTFOUND proxy-check.invalid`, ohne: 200 direkt)
  - `uv venv` funktioniert
- `harness doctor`: 4/4 ✓. In der VM sind keine Test-Container, -Volumes oder -Netze übrig.
- Phase 1 seit der letzten Sitzung nicht erneut geprüft (unverändert): `pnpm -r typecheck` grün, Server-Tests 30/30 (SQLite), E2E 7/7.

---

## Log

### 2026-10-12 (Phase 2, Pakete 1–3)
- Cargo-Workspace (`Cargo.toml`, `.cargo/config.toml` mit 2 Jobs), clippy und rustfmt zur Toolchain hinzugefügt.
- `harness-core`: `provider.rs` (Trait, Specs, Validierung, `ExchangeDir`), `podman/` (Machine-JSON, Argument-Builder mit fester Härtung, Provider), `image.rs` (eingebetteter Kontext, Hash, Build).
- `harness-cli`: `doctor`, `image build`.
- `images/base`: Containerfile, gitconfig, profile.sh. Image in der VM gebaut und in zwei VM-Tests verifiziert.
- Nutzer hat DeepSeek als LLM-Anbieter für pi gewählt.

### 2026-10-12
- Windows-Sandbox-Spike verworfen (keine Exec-Ausgabe, Bluescreen), zurück zu Hyper-V + Linux. Machine `harness` mit Austauschordner und 1-GB-Startspeicher angelegt.
- Phase 0 abgeschlossen, Doku (`ROADMAP.md`, `PROGRESS.md`, ARCHITECTURE §3/§9) aktualisiert.

### 2026-10-11
- Chat-Bug (useChat + StrictMode → `chat-store.ts`) und Login-Schleife behoben, E2E-Test und `CLAUDE.md` angelegt.

### 2026-10-10
- Architektur geplant, Server und Web-App (Phase 1) gebaut, WSL-Spike durchgeführt.
