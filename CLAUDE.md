# LLM Harness – Hinweise für Agenten

Selbst gehosteter Multi-User-LLM-Chat (Server + Web) plus geplanter Desktop-Client, der Chats mit isolierten Container-Umgebungen auf dem Rechner des Nutzers verbindet.

**Lesereihenfolge für neue Sitzungen:**
1. [`docs/PROGRESS.md`](docs/PROGRESS.md): wo wir stehen, nächster Schritt, offene Fragen
2. [`docs/ROADMAP.md`](docs/ROADMAP.md): Phasen, Arbeitspakete, Abnahmekriterien
3. [`ARCHITECTURE.md`](ARCHITECTURE.md): Gesamtbild und Begründungen
4. bei Bedarf `docs/spikes/`

## Stand

- **Phase 0 (Spike)** ist erledigt, Ergebnisse in [`docs/spikes/phase-0.md`](docs/spikes/phase-0.md). Der Hyper-V-Teil steht in §12.
- **Phase 1 (Server + Web-Chat)** ist umgesetzt: `apps/server`, `apps/web`, `packages/shared`. Reste stehen in ROADMAP.md.
- **VM für Phase 2 läuft:** Podman-Machine `harness` (Hyper-V, Fedora 44 arm64, rootless). Einzige Freigabe ist `%LOCALAPPDATA%\llm-harness\exchange` → `/mnt/exchange`. Startspeicher 1 GB, dynamisch bis 4 GB. Container-Befehle aus Git Bash: `spikes/phase-0/pm.sh <podman-args>` (liest SSH-Port und -Nutzer aus der Machine-JSON). **Starten und Stoppen der VM nur per Admin-PowerShell durch den Nutzer**, bis `harness-vmd` existiert.
- Erste Harness für Phase 2: **pi** (`@earendil-works/pi-coding-agent`), LLM-Anbieter des Nutzers für pi: **DeepSeek** (`api.deepseek.com`).
- **Phase 2, Pakete 1–5 erledigt:** Cargo-Workspace (`crates/harness-core`, `crates/harness-cli` → Binary `harness`), Provider-Schnittstelle mit `PodmanProvider`, Base-Image `images/base` (gebaut als `localhost/harness/base:latest`), Konfiguration in `~/.harness` (`harness-core/src/config/`), Git-Sync über Bundles (`harness-core/src/sync/`). CLI kann bisher `doctor`, `image build` und `project [show|add|list]`. Remote `origin` = GitHub `johnnyjack123/llm-harness`, Branch `feat/phase-2-sandbox`.
- **Richtungsentscheidung:** Erster und vorerst einziger Sandbox-Provider ist **Podman + Hyper-V mit Linux-VM im Isoliert-Modus** (siehe ARCHITECTURE.md §3). Windows Sandbox wurde verworfen (RAM, kein Exec-I/O, Absturz; `docs/spikes/phase-0b-windows-sandbox.md`). Weitere Provider kommen später über die Provider-Schnittstelle. Bitte keinen zweiten Provider „nebenbei“ anfangen. Ein ungesicherter **Host-Modus** ist vorgemerkt, aber noch nicht dran.
- Nächstes: Phase 2 ab Paket 6 (Gateway, pi, CLI-Befehle; danach `harness-vmd`).

## Arbeitsweise

- Sprache: Doku und UI-Texte **Deutsch** (UI zusätzlich Englisch), Code-Kommentare, Commits und Bezeichner **Englisch**.
- Commits nur, wenn der Nutzer darum bittet. Konventionen siehe Skill `git-flow`.
- TS/JS: JSDoc für jeden Export, geschweifte Klammern immer (Skill `ts-js-code-konventionen`). Kommentare erklären das *Warum* (Skill `code-comments`).
- Rust: Doc-Kommentar für jedes `pub`-Element (`harness-core` hat `#![warn(missing_docs)]`), `cargo fmt` und `cargo clippy --all-targets` ohne Warnungen. Fehlermeldungen sind UI-Text, also Deutsch.
- UI: **nur** semantische Tokens, keine festen Werte. Regeln in [`docs/design/DESIGN.md`](docs/design/DESIGN.md). Neue Primitive bekommen einen Eintrag auf der Design-Seite `/design` (nur im Dev-Modus). Skill `ui-component` beachten.
- **Neue Erkenntnisse und Stolpersteine hier unten eintragen**, Details in `docs/`.
- Große Downloads vorher kurz ansprechen, der Nutzer ist manchmal im Mobilfunknetz.
- Vor „fertig“: Typecheck, Tests **und** ein echter Lauf. UI-Änderungen mit dem E2E-Ablauf prüfen (siehe unten), nicht nur mit Typecheck.

## Befehle

```sh
pnpm install                          # pnpm 12; Build-Skripte nur per allowBuilds in pnpm-workspace.yaml
pnpm --filter @harness/server dev     # API auf :8080 (SQLite in apps/server/data/)
pnpm --filter @harness/web dev        # Vite auf :5173, proxied /api → :8080 (HARNESS_API_URL überschreibt)
pnpm --filter @harness/server test    # Vitest; mit TEST_POSTGRES_URL zusätzlich gegen Postgres
pnpm -r typecheck
pnpm --filter @harness/web build
pnpm --filter @harness/web e2e        # Browser-E2E gegen isolierten Stack (siehe unten)

cargo build                           # Rust-Workspace; 2 Jobs per .cargo/config.toml (RAM)
cargo test                            # Unit-Tests
cargo fmt; cargo clippy --all-targets
target/debug/harness doctor           # Konfiguration, Machine, SSH, Austauschordner, Base-Image
target/debug/harness image build      # Base-Image in der VM bauen (--pull, --no-cache)
target/debug/harness project add      # aktuelles Git-Repo als Projekt anlegen (show, list)
HARNESS_HOME=<ordner> target/debug/harness …   # andere Konfiguration als ~/.harness (zum Testen)
cargo test -p harness-core --test podman_vm -- --ignored --nocapture      # Provider gegen die echte VM (alpine)
cargo test -p harness-core --test base_image_vm -- --ignored --nocapture  # Base-Image im gehärteten Container
cargo test -p harness-core --test sync_vm -- --ignored --nocapture        # Git-Sync mit den echten Container-Skripten
cargo test -p harness-core -- --ignored --test-threads=1                   # alle VM-Tests nacheinander (~40 s)
```

Postgres zum Testen (Git Bash, VM muss laufen): `spikes/phase-0/pm.sh run -d --name harness-pg -e POSTGRES_USER=harness -e POSTGRES_PASSWORD=harness -e POSTGRES_DB=harness_test -p 127.0.0.1:55432:5432 docker.io/library/postgres:17-alpine`, dann `TEST_POSTGRES_URL=postgres://harness:harness@127.0.0.1:55432/harness_test pnpm --filter @harness/server test`. Ob die Portweiterleitung über gvproxy unter Hyper-V funktioniert, ist ungetestet.

VM-Befehle für den Nutzer (Admin-PowerShell): `$env:CONTAINERS_MACHINE_PROVIDER='hyperv'` und dann `& "C:\Program Files\RedHat\Podman\podman.exe" machine start|stop harness`.

## Stolpersteine (bitte ergänzen)

### Bibliotheken (Versionen sind neuer als die meisten Trainingsdaten)

- **AI SDK 7** (`ai@7`, `@ai-sdk/*@4`): `system` heißt jetzt `instructions`, `onFinish` heißt `onEnd`. `result.toUIMessageStreamResponse()` ist veraltet, stattdessen `createUIMessageStreamResponse({ stream: toUIMessageStream({ stream: result.stream, … }) })`. System-Nachrichten in `messages` werden abgelehnt. Doku liegt im Paket unter `node_modules/ai/docs/`, Migrationshinweise in `08-migration-guides/`. **Vor dem Schreiben dort nachlesen.**
- **`useChat` darf die `Chat`-Instanz nicht selbst besitzen.** Beim Unmount ruft es `chat.stop()` auf, und das passiert in React StrictMode schon beim simulierten Unmount direkt nach dem ersten Mount. `sendMessage` verwirft die Nachricht dann **stillschweigend**. Chats deshalb immer über `apps/web/src/features/chat/chat-store.ts` (`getChat`) erzeugen und mit `useChat({ chat })` verwenden.
- **TanStack Query:** `invalidateQueries` lädt nur Queries mit aktiven Beobachtern neu. Route-Guards nutzen `ensureQueryData`, das auch veraltete Cache-Werte zurückgibt. Nach Login oder Logout deshalb `fetchQuery({ ...sessionQuery, staleTime: 0 })`, sonst gibt es eine Redirect-Schleife.
- **Better Auth 1.7:** Migrationen (`getMigrations(options)`) laufen **vor** `betterAuth(options)`, sonst warnt die Schemaprüfung beim Start. Tabellen und Spalten sind camelCase, unsere eigenen Tabellen auch.
- **Kysely 0.29:** `Migrator` und Co. kommen aus `kysely/migration`. Migrationen müssen portabel bleiben (SQLite + Postgres): bigint-Epoch-ms, JSON als Text, kein Raw-SQL.
- **TypeScript ist auf `~6.0` gepinnt**, weil typescript-eslint TS 7 (die Go-Version) noch nicht unterstützt.
- **Tailwind v4:** Die Standardpalette ist absichtlich abgeschaltet (`--color-*: initial`). Streamdown nutzt intern `red-*` und `black`, dafür gibt es Aliase in `globals.css`. Token-Namen in `@theme inline` dürfen sich nicht selbst referenzieren, deshalb heißen die Quelltokens `--font-family-*` und `--easing-*`.
- **Streamdown v2:** Plugins `@streamdown/code|math|mermaid` sind getrennte Pakete, dazu `@source` in `globals.css`. Shiki nutzt WASM, die CSP braucht deshalb `'wasm-unsafe-eval'`.

### Windows / Umgebung

- Rechner: Windows 11 Pro **ARM64**. Images müssen multi-arch sein.
- Windows PowerShell 5.1 liest `.ps1` ohne BOM als ANSI, deshalb Skripte **ASCII-only** halten.
- Podman scheitert an der `~/.ssh/known_hosts` des Nutzers (Zeilen ohne Host-Muster). Nicht anfassen, stattdessen `spikes/phase-0/pm.sh` (eigene known_hosts, Verbindung explizit).
- Port-Weiterleitungen immer an `127.0.0.1` binden (`-p 127.0.0.1:X:Y`). Ohne Angabe wird unter Windows teils nur `[::1]` bedient.
- **Podman Hyper-V-Provider:** jeder Machine-Befehl braucht Admin-Rechte. Der Startspeicher muss klein gesetzt werden (`Set-VMMemory`). Nach einem abgebrochenen Start bleiben Ignition-KVP-Items hängen, die per WMI entfernt werden müssen. vsock-Registry-Einträge **nicht löschen**, sie stammen aus `init`. Details und Snippets: `docs/spikes/phase-0.md` Abschnitt 12.
- **Hyper-V-9p-Freigaben sind für Entwicklung unbrauchbar:** `chmod`/`copyfile` → EPERM, rekursives Löschen → EIO, also kein git, npm oder Build. Projekte gehören in Volumes in der VM (Isoliert-Modus).
- **gvproxy leitet `192.168.127.254` / `host.containers.internal` auf `127.0.0.1` des Windows-Hosts weiter.** Aus normalen Container-Netzen sind damit lokale Dienste des Nutzers erreichbar. Instanzen nur in `--internal`-Netzen betreiben.
- **WSL-Provider:** Die VM sieht ganz `C:\` mit Schreibrechten, und Interop ist aktiv. Das ist nur ein Fallback (Windows Home). Bind-Mounts über 9p sind langsam (~2 ms pro Dateizugriff).
- **Windows Sandbox:** Der erste Start (12.10.2026) hat den Rechner mit Bluescreen `0xA0` INTERNAL_POWER_ERROR abgestürzt, vermutlich ein Fehler der Energieverwaltung auf dem ARM64-Rechner beim Start der Virtualisierung. **Sandboxes nie ungefragt starten**, vorher den Nutzer bitten, seine Arbeit zu speichern. Die CLI `wsb` kommt erst mit der Store-Version (nach dem ersten Start). `wsb exec` liefert keine Ausgabe, nur den Exit-Code. Die Sandbox bootet ein vollständiges Windows mit Oberfläche, also ~1,5–2,5 GB RAM pro Instanz.
- Wenig freier RAM (~3 GB): Die WSL-VM nimmt sich bis zu 7,7 GB, unabhängig von der Podman-Einstellung. Nicht benötigte Machines stoppen.

### Sandbox-Kern (Rust, Podman, Base-Image)

- **`harness-core` steuert `podman.exe` per CLI**, nicht per bollard/REST: `--url ssh://core@127.0.0.1:<Port>/run/user/<UID>/podman/podman.sock --identity <Key>`, dazu `HOME`/`USERPROFILE` auf `%LOCALAPPDATA%\llm-harness\podman\<machine>-<port>\`. Dort liegt eine eigene `known_hosts`, die beim ersten Aufruf per `ssh-keyscan.exe` (Windows-OpenSSH) gefüllt wird (Trust on first use). Port, Nutzer, UID und Key stammen immer aus der Machine-JSON.
- **Jeder podman-Aufruf kostet ~350–500 ms**, weil jedes Mal eine neue SSH-Verbindung aufgebaut wird. Aufrufe sparsam einsetzen. Falls `harness run` zu träge wird: libpod-REST über einen gehaltenen SSH-Tunnel.
- **Sicherheitsinvarianten sitzen im Provider, nicht beim Aufrufer:** Härtung ist in `podman/args.rs` fest verdrahtet. `create_container` prüft per `network inspect`, dass das Netz `internal` ist. Mounts sind nur als Volume oder als Unterordner des Austauschordners darstellbar, und alle Namen und Pfade werden vor dem Aufruf validiert (kein führendes `-`, keine Kommas in `--mount`). Das Gateway (Paket 6) braucht deshalb eine eigene Methode.
- `--init` (catatonit) läuft im Container als `/run/podman-init`, `/proc/1/comm` ist `podman-init`.
- Austauschordner (9p) und `--userns=keep-id:uid=1000,gid=1000` vertragen sich: Container (UID 1000) kann lesen und schreiben, Host sieht die Dateien sofort. Für einzelne Dateien reicht das, für Arbeitsbäume nicht (siehe 9p oben).
- **`images/base/*` ist per `include_str!` in `harness-core/src/image.rs` eingebettet.** Neue Dateien dort eintragen, sonst schlägt der Test `base_context_is_complete` fehl. Das Image trägt das Label `io.llm-harness.context-hash`, und `doctor` meldet „veraltet“, sobald sich der eingebettete Kontext ändert.
- **Node-`fetch` ignoriert `HTTP(S)_PROXY` ohne `NODE_USE_ENV_PROXY=1`** (im Image gesetzt, verifiziert). Die LLM-SDKs von pi nutzen `fetch`. Ohne die Variable würde pi das Gateway umgehen bzw. im internen Netz scheitern.
- **Debians `/etc/profile` überschreibt `PATH` in Login-Shells** (`bash -l`) für Nicht-root-Nutzer. `images/base/profile.sh` (→ `/etc/profile.d/harness.sh`) fügt `/opt/harness-tools/bin` und `~/.local/bin` wieder ein.
- Root-Dateisystem der Instanzen ist read-only: `npm i -g` geht nach `~/.local` (`NPM_CONFIG_PREFIX`), pnpm lädt corepack beim ersten Aufruf nach `~/.cache` (Home-Volume). `COREPACK_ENABLE_AUTO_PIN=0` verhindert, dass corepack `packageManager` in die `package.json` des Nutzers schreibt.
- Node 24 bringt corepack noch mit, ab Node 25 fehlt es. Beim Wechsel auf Node 26 corepack per npm installieren.
- **Konfiguration (`~/.harness`):** lesen mit `toml` 1.x (serde), schreiben nur über `toml_edit::DocumentMut` + `ConfigDir::update_project`, damit Kommentare bleiben. Alle Tabellen haben `deny_unknown_fields`. Neue Schlüssel also im Modell (`config/model.rs`) **und** in der Vorlage (`harness-core/defaults/`) ergänzen. Ein Test prüft, dass Vorlage und Code-Standards übereinstimmen. Tests und Probeläufe mit `HARNESS_HOME`, nicht im echten Profil des Nutzers.
- **Git auf dem Host nur über `sync::HostRepo`:** Es setzt `core.hooksPath` auf einen leeren Ordner, schaltet fsmonitor und Submodule-Rekursion ab und schaltet `fsckObjects` ein. Was aus dem Container kommt (Bundles, gemergte Dateien, Ausgaben), ist nicht vertrauenswürdig. Ein Hook-Ordner im Arbeitsbaum (husky) würde sonst nach dem Merge Agent-Code auf dem Host ausführen. Container-Text vor der Anzeige durch `sync::sanitize` (Terminal-Escapes).
- **Container-Skripte** (`sync/mod.rs`) bekommen Parameter nur als `$1`, `$2`, nie per Textersetzung, und setzen `PATH=/usr/bin:/bin`, weil `~/.local/bin` dem Agent gehört. Ergebnis als `result=…`-Zeile, nur diese wird ausgewertet.
- `git rev-list … --exclude=<muster> --branches`: Das Muster steht **ohne** `refs/heads/`, sonst schließt es nichts aus.
- Host-Git ist hier 2.49 (`merge-tree --write-tree` braucht ≥ 2.38), und `core.autocrlf=true` ist global gesetzt. Tests setzen Identität und `autocrlf` deshalb lokal im Test-Repo.

### Testen ohne den Browser des Nutzers

`pnpm --filter @harness/web e2e` startet einen isolierten Stack und klickt den Chat-Ablauf in einem Headless-Browser durch:
- Mock-LLM im OpenAI-Format auf :8091
- Server auf :8090 mit Wegwerf-DB
- Vite im Dev-Modus (StrictMode) auf :5174
- Browser: installiertes Edge/Chrome via `puppeteer-core`, nichts wird heruntergeladen

Screenshots landen in `apps/web/e2e/.output/`. Bei UI-Änderungen laufen lassen und die Screenshots ansehen. Den Chrome des Nutzers nicht ohne Rückfrage fernsteuern.
