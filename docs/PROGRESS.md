# Progress

_Last updated: 2026-10-12 · Phase 2 Pakete 1–5 fertig (Workspace, PodmanProvider, Base-Image, Konfiguration, Git-Sync), weiter mit Paket 6 (Gateway)_

## Where we are

Server (`apps/server`) und Web-App (`apps/web`) aus Phase 1 laufen unverändert. Das Repo liegt auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). `main` enthält Phase 1 und Phase 2 bis Paket 5, gearbeitet wird auf `feat/phase-2-sandbox`.

`crates/harness-core` enthält:
- Provider-Schnittstelle und `PodmanProvider`
- das eingebettete Base-Image
- Konfiguration und Projektnamen
- **neu** `sync/`: Git-Sync über Bundles im Austauschordner, Host-Seite gehärtet, Merge mit Trockenlauf

Die CLI (`crates/harness-cli`) kann `doctor`, `image build` und `project [show|add|list]`. Instanzen, Gateway, pi und die Befehle `run`/`sync`/`merge` fehlen noch. Die CLI kann also noch keinen Agent starten. Sync und Merge sind als Bibliotheksfunktionen fertig und in der VM getestet.

## Next step

Paket 6 der ROADMAP (Netzwerk-Gateway):

1. **Neue Provider-Methode `create_gateway`.** `create_container` lässt bewusst nur interne Netze zu. Das Gateway hängt im internen Projektnetz (Alias `gateway`, Port 8888, so im Base-Image gesetzt) und in einem normalen Netz (z. B. `harness-egress`). Gehärtet wie Instanzen.
2. **Gateway-Image** aus `spikes/phase-0/proxy/` (tinyproxy) als `images/gateway/`, eingebettet wie `images/base` (`image.rs`).
3. **Allowlist** aus `Project` (`network`, `allow`, `deny`) erzeugen:
   - Voreinstellung „development“: npm, PyPI, GitHub inklusive `codeload`/`objects.githubusercontent.com` und `api.deepseek.com`.
   - Prüfen, von wo corepack pnpm lädt.
   - `deny` gewinnt.
   - Die Konfiguration darf nicht über den Austauschordner laufen, den der Agent beschreiben kann. Besser per `podman cp` vor dem Start oder als eigenes Volume.
4. **Ein Gateway pro Projekt** (`harness-gw-<name>`) ist mit tinyproxy (~2 MB RAM) am einfachsten, weil jedes Projekt eigene Regeln hat.
5. **Grenze von tinyproxy:** keine Prüfung der aufgelösten Ziel-IP (Phase 0, 6.4). „open“ deshalb bis zum Rust-Gateway (Phase 3) ablehnen oder deutlich warnen.
6. **Abnahme:** `curl https://example.com` im Container → 403, `npm view react version` geht, `api.deepseek.com` erreichbar.

Danach Paket 7 (pi im Tools-Volume, `~/.pi` im Volume `harness-cfg-pi`) und 8 (`run`, `shell`, `status`, `stop`, `sync`, `merge`).

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alle Zeilen ✓, solange die VM läuft).

## In flight / half-finished

- `PodmanProvider::exec` mit `tty: true` ist gebaut, aber **nicht interaktiv getestet**. Das kommt mit `harness shell` (Paket 8) und muss der Nutzer in Windows Terminal ausprobieren. Ctrl+C und Fenstergröße sind ungeprüft.
- Das echte `~/.harness` des Nutzers ist noch nicht angelegt. Probeläufe liefen mit `HARNESS_HOME` im Scratchpad.

## Decisions made this session

Paket 5 (Sync):
- **Host-Git läuft immer ohne Hooks**, mit fsmonitor aus, ohne Submodule-Rekursion und mit `fsckObjects`. Alles aus dem Container gilt als nicht vertrauenswürdig. Ein husky-Hook-Ordner im Arbeitsbaum würde sonst nach dem Merge Agent-Code auf dem Host ausführen. Preis: Eigene Hooks des Nutzers (z. B. `post-merge` → `npm install`) laufen bei `harness merge` nicht.
- **In-Bundle mit `HEAD --branches --tags` statt `--all`:** Stash und Remote-Tracking-Refs bleiben auf dem Host.
- **Out-Bundle inkrementell** (`--not --remotes=host`), automatischer Rückfall auf ein vollständiges Bundle, wenn der Host die Voraussetzungen nicht hat. Den Bundle-Kopf liest der Host selbst, ohne Git-Meldungen zu parsen.
- **Agent-Branch auf dem Host:** nur Fast-Forward (`--force` für Umgeschriebenes), Compare-and-swap, nie, solange er ausgecheckt ist.
- **Merge nur bei sauberem Arbeitsbaum** (getrackte Dateien), Konflikte vorher per `merge-tree` erkannt. Bei Konflikt bleibt das Repo unverändert.
- **Remote im Klon heißt `host`**, nicht `origin`, damit ein Agent nicht versucht, „nach origin zu pushen“.

Pakete 1–4 (Kurzfassung, Details in ROADMAP):
- podman-CLI über SSH statt REST. Sicherheitsinvarianten im Provider erzwungen. `keep-id:uid=1000` + `--init`.
- Base-Image eingebettet mit Content-Hash. Node 24 aus dem offiziellen Image. pnpm über corepack zur Laufzeit. `NODE_USE_ENV_PROXY=1`.
- Konfiguration mit `deny_unknown_fields`. Projekte dürfen den Konfigurationsordner weder enthalten noch in ihm liegen. Preset beim Anlegen explizit. `deny` gewinnt.
- Tools-Volume für Instanzen nur lesbar (Paket 7). LLM-Anbieter für pi: DeepSeek.

## Open questions for the user

- Für pi mit DeepSeek: API-Key per pi-Login im Home-Volume (`~/.pi`) oder später über das LLM-Gateway des Servers (Phase 4)? Für Phase 2 reicht der pi-Login.

## Known issues / debt

- **Hoch:** VM-Start nur per Admin-PowerShell (→ Phase 2b). Nach einem gescheiterten Start von Hand aufräumen, siehe `spikes/phase-0.md` §12.2. Die vsock-Registry-Einträge **nicht** löschen.
- **Mittel:** ~350–500 ms pro podman-Aufruf (neue SSH-Verbindung). Sync hin ~0,6 s, zurück ~0,7 s. `harness run` vermutlich 3–5 s. Falls störend: libpod-REST über einen gehaltenen SSH-Tunnel.
- **Mittel:** `in.bundle` enthält jedes Mal die ganze Historie. Bei großen Repos inkrementell machen (`--not` mit dem Stand des letzten Exports).
- **Mittel:** CLI-Texte nur Deutsch, Englisch fehlt (Web-UI ist zweisprachig).
- **Mittel:** kein Dockerfile/compose für den Server, keine ESLint/Prettier-Konfiguration, kein README (ROADMAP „Phase 1 – Reste“). Web-Haupt-Chunk ~955 kB.
- **Niedrig:** Host-Key der VM per Trust on first use. Bei neuem Host-Key mit gleichem Port `%LOCALAPPDATA%\llm-harness\podman\harness-<port>\` löschen.
- **Niedrig:** Podman-Hilfsprozesse laufen im Kontext des Admin-Fensters. Schließt der Nutzer es, ist das Verhalten ungeprüft.
- Rechner-Stabilität: Bluescreen `0xA0` beim Start der Windows Sandbox, Abbruch der WSL-VM bei Standby. Nutzer vor riskanten Virtualisierungs-Starts fragen.

## Verification state

- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: ohne Warnungen.
- `cargo test`: 62/62 Unit-Tests grün, davon 13 neu für Sync. Abgedeckt:
  - Round-Trip mit Fast-Forward, auseinanderlaufende Branches mit Merge-Commit
  - Konflikt lässt das Repo unverändert, uncommittete Arbeit bleibt unangetastet, detached HEAD wird abgelehnt
  - umgeschriebener Branch nur mit `force`, ausgecheckter Branch wird nicht bewegt
  - fehlende Voraussetzungen → vollständiges Bundle, kaputte Bundles werden abgelehnt
  - **Hooks laufen nicht**: Kontrolltest beweist, dass Hooks hier grundsätzlich laufen. Geprüft für `.git/hooks` und für einen Hook-Ordner im Arbeitsbaum, jeweils `post-merge` und `reference-transaction`.
- VM-Tests, nacheinander (`cargo test -p harness-core -- --ignored --test-threads=1`): alle grün.
  - `podman_vm`: Härtung, Netz, Volumes, Austauschordner, Lebenszyklus.
  - `base_image_vm`: Werkzeuge, Rechte, gitconfig, Proxy-Variablen.
  - `sync_vm`, neu, mit den echten Container-Skripten:
    - Klonen, Agent-Commit, Rückweg (Autor „harness agent“), Fast-Forward
    - Container sieht nur `in.bundle`
    - zweite Runde mit Fetch und Merge-Commit
  - In der VM ist danach nichts übrig.
- Vorab von Hand geprüft: `git bundle create` direkt auf 9p, `cp`/`mv` und `clone` aus dem Bundle funktionieren.
- `harness doctor` 5/5 ✓ (mit `HARNESS_HOME`).
- Phase 1 seit dem 11.10. nicht erneut geprüft (unverändert): `pnpm -r typecheck` grün, Server-Tests 30/30 (SQLite), E2E 7/7.

---

## Log

### 2026-10-12 (Phase 2, Paket 5)
- Paket 4 committet, Remote `origin` (GitHub) eingerichtet, `feat/phase-1-chat` gelöscht, `feat/phase-2-sandbox` nach `main` gemergt (Fast-Forward), beides gepusht.
- `harness-core/src/sync/`:
  - `bundle.rs`: Bundle-Kopf lesen.
  - `host.rs`: `HostRepo` mit Export, Import und Merge, gehärtet.
  - `mod.rs`: Container-Skripte, `push_to_workspace`, `pull_from_workspace`, `sanitize`.
- `find_on_path` nach `paths.rs` verschoben. Neue Fehler `Error::Git` und `Error::Sync`.
- Paket 5 committet, nach `main` gemergt (Fast-Forward), beides gepusht.

### 2026-10-12 (Phase 2, Paket 4)
- `harness-core`: `config/`, `project.rs`, Pfadvergleich, Vorlagen in `defaults/`. CLI: `project [show|add|list]`, `doctor` prüft die Konfiguration.

### 2026-10-12 (Phase 2, Pakete 1–3)
- Cargo-Workspace, `provider.rs`, `podman/`, `image.rs`, `images/base`. CLI: `doctor`, `image build`. Nutzer wählt DeepSeek für pi.

### 2026-10-12
- Windows-Sandbox-Spike verworfen, zurück zu Hyper-V + Linux. Machine `harness` angelegt, Phase 0 abgeschlossen.

### 2026-10-11
- Chat-Bug (useChat + StrictMode → `chat-store.ts`) und Login-Schleife behoben, E2E-Test und `CLAUDE.md` angelegt.

### 2026-10-10
- Architektur geplant, Server und Web-App (Phase 1) gebaut, WSL-Spike durchgeführt.
