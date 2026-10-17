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
- **VM für Phase 2 läuft:** Podman-Machine `harness` (Hyper-V, Fedora 44 arm64, rootless). Sie teilt noch den alten Austauschordner `%LOCALAPPDATA%\llm-harness\exchange` → `/mnt/exchange`; harness nutzt ihn seit dem 16.10. nicht mehr (Bundles per `podman cp` bzw. `podman exec cat`). Startspeicher 1 GB, dynamisch bis 4 GB. Container-Befehle aus Git Bash: `spikes/phase-0/pm.sh <podman-args>` (liest SSH-Port und -Nutzer aus der Machine-JSON). **Starten und Stoppen der VM** per Admin-PowerShell durch den Nutzer, bis er `harness vmd install` ausgeführt hat (gebaut am 17.10., Einrichtung steht aus). Danach startet `harness` die VM selbst, `harness vmd start|stop|restart` geht ohne Admin.
- Erste Harness für Phase 2: **pi** (`@earendil-works/pi-coding-agent`), LLM-Anbieter des Nutzers für pi: **DeepSeek** (`api.deepseek.com`).
- **Phase 2, Pakete 1–6 erledigt:**
  - Cargo-Workspace: `crates/harness-core`, `crates/harness-cli` (Binary `harness`) und `crates/harness-gateway`
  - Provider-Schnittstelle mit `PodmanProvider`
  - Images `images/base` und `images/gateway`
  - Konfiguration in `~/.harness` (`harness-core/src/config/`)
  - Git-Sync über Bundles (`harness-core/src/sync/`)
  - Netzwerk-Gateway pro Projekt mit Voreinstellungen, Ziel-IP-Prüfung und Rückfragen (`harness-core/src/gateway.rs`)
- **Pakete 7–9 gebaut, Smoke-Skript grün:**
  - **LLM-Zugang über den Server.** Keys liegen nur im Server. `harness login` koppelt den Rechner per Code-Bestätigung in der Web-App (`/device`). Das Gateway leitet `http://llm.harness/<Anbieter-ID>/…` mit dem Geräte-Token an `/api/llm/proxy/…` weiter, der Server setzt den Key ein.
  - **pi in der Instanz:** pi liegt im Tools-Volume, `models.json` zeigt auf `llm.harness` (`instance.rs`, `pi.rs`).
  - **CLI:** `run [harness] [-- args]`, `shell`, `sync`, `merge`, `stop`, `status` (`harness-cli/src/run.rs`).

  - **Rückfragen des Gateways im selben Terminal** (Relay über eine Pseudo-Konsole, `harness-cli/src/terminal/`) und **Hinweise an den Agent** im System-Prompt (`briefing.rs`).

  Offen in Phase 2: Rückmeldung des Nutzers zum Relay in Windows Terminal. Remote `origin` = GitHub `johnnyjack123/llm-harness`, Branch `feat/phase-2-sandbox`.
- **Richtungsentscheidung:** Erster und vorerst einziger Sandbox-Provider ist **Podman + Hyper-V mit Linux-VM im Isoliert-Modus** (siehe ARCHITECTURE.md §3). Windows Sandbox wurde verworfen (RAM, kein Exec-I/O, Absturz; `docs/spikes/phase-0b-windows-sandbox.md`). Weitere Provider kommen später über die Provider-Schnittstelle. Bitte keinen zweiten Provider „nebenbei“ anfangen. Ein ungesicherter **Host-Modus** ist vorgemerkt, aber noch nicht dran.
- Nächstes: Der Nutzer richtet `harness vmd install` ein (UAC). Phase 2c Stufe 1 (feste Regeln) ist fertig; Stufen 2 und 3 und die Einstellungen dazu folgen.
- **Rückfragen und Freigaben entscheidet nie etwas im Container**, auch nicht die Oberfläche von pi oder opencode. Sonst könnte sich ein gekaperter Agent selbst freigeben. Oberflächen sind `harness net watch`, später `harness run` selbst (Host-Prozess), die App und der Web-Chat.

## Arbeitsweise

- Sprache: Doku und UI-Texte **Deutsch** (UI zusätzlich Englisch), Code-Kommentare, Commits und Bezeichner **Englisch**.
- Nach jedem fertigen, geprüften Stück committen, in `main` mergen und beides pushen, ohne nachzufragen (Freigabe des Nutzers vom 14.10.). Nur Stände, die bauen und deren Tests grün sind. Konventionen siehe Skill `git-flow`.
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
target/debug/harness doctor           # Konfiguration, Machine, SSH, Freigaben, Base-Image
target/debug/harness image build      # Base- und Gateway-Image in der VM bauen (base|gateway, --pull, --no-cache)
target/debug/harness project add      # aktuelles Git-Repo als Projekt anlegen (show, list)
target/debug/harness net              # Netz des Projekts; start|stop, preset open, allow/deny <domain> [--always]
target/debug/harness net watch        # Live-Protokoll, beantwortet Rückfragen des Gateways (zweites Terminal)
target/debug/harness login [URL]      # Rechner mit dem Server koppeln (--no-browser in Tests); logout widerruft
target/debug/harness                  # ohne Befehl: Standard-Harness im Projekt des Ordners (legt Repo/Projekt nach Rückfrage an)
target/debug/harness vmd [status|start|stop|restart|install|uninstall]   # VM-Dienst; install/uninstall fragen per UAC
target/debug/harness review           # Merge-Prüfung (feste Regeln) ohne zu übernehmen; merge prüft immer, --ignore-findings übergeht schwere Funde
target/debug/harness run [pi] [-- …]  # Gateway, Instanz, Klon, pi installieren und verbinden, dann pi (TTY)
target/debug/harness shell            # Login-Shell in der Instanz; sync/merge holen die Commits des Agents
HARNESS_HOME=<ordner> target/debug/harness …   # andere Konfiguration als ~/.harness (zum Testen)
cargo test -p harness-core --test podman_vm -- --ignored --nocapture      # Provider gegen die echte VM (alpine)
cargo test -p harness-core --test base_image_vm -- --ignored --nocapture  # Base-Image im gehärteten Container
cargo test -p harness-core --test sync_vm -- --ignored --nocapture        # Git-Sync mit den echten Container-Skripten
cargo test -p harness-core --test gateway_vm -- --ignored --nocapture     # Gateway: Voreinstellungen, Sperren, Rückfrage (braucht Internet)
cargo test -p harness-core --test instance_vm -- --ignored --nocapture    # Instanz, Installation über das Gateway, models.json
cargo test -p harness-core -- --ignored --test-threads=1                   # alle VM-Tests nacheinander (~2 min)
cargo test -p harness-cli --test terminal_vm -- --ignored --test-threads=1  # harness shell/run pi in einem simulierten Terminal (Rückfragen, Strg+C, Größe)
bash scripts/smoke-run.sh             # Phase 2 komplett: Server + Mock-LLM, login, run pi, shell, sync, merge, logout (~40 s)
```

Postgres zum Testen (Git Bash, VM muss laufen): `spikes/phase-0/pm.sh run -d --name harness-pg -e POSTGRES_USER=harness -e POSTGRES_PASSWORD=harness -e POSTGRES_DB=harness_test -p 127.0.0.1:55432:5432 docker.io/library/postgres:17-alpine`, dann `TEST_POSTGRES_URL=postgres://harness:harness@127.0.0.1:55432/harness_test pnpm --filter @harness/server test`. Die Portweiterleitung über gvproxy funktioniert unter Hyper-V (14.10. geprüft). Danach `pm.sh rm -f harness-pg` und das anonyme Datenvolume löschen (`pm.sh volume ls`).

VM-Befehle für den Nutzer: nach `harness vmd install` einfach `harness vmd start|stop` (ohne Admin). Vorher bzw. nach `harness vmd uninstall` in einer Admin-PowerShell: `$env:CONTAINERS_MACHINE_PROVIDER='hyperv'` und dann `& "C:\Program Files\RedHat\Podman\podman.exe" machine start|stop harness`. Gehört die Machine dem Dienst, sieht der podman des Nutzers sie nicht mehr (seine JSON heißt dann `harness.json.moved-to-harness-vmd`).

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
- **Sicherheitsinvarianten sitzen im Provider, nicht beim Aufrufer:** Härtung ist in `podman/args.rs` fest verdrahtet. `create_container` prüft per `network inspect`, dass das Netz `internal` ist. Mounts sind nur als Volume darstellbar, Host-Ordner gar nicht, und alle Namen und Pfade werden vor dem Aufruf validiert (kein führendes `-`, keine Kommas in `--mount`). Das Gateway (Paket 6) braucht deshalb eine eigene Methode.
- `--init` (catatonit) läuft im Container als `/run/podman-init`, `/proc/1/comm` ist `podman-init`.
- **Keine Host-Ordner in der VM.** Daten gehen nur per `Provider::copy_into` (`podman cp`, Ziel im Container aufgelöst) und `copy_out` (`podman exec cat`, nur reguläre Dateien, Größenlimit) hin und her. Eine 9p-Freigabe bräuchte einen Dateiserver auf dem Host. Startet der privilegierte VM-Dienst die Machine, liefe der als SYSTEM und würde jeden Ordner servieren, auf den ein Prozess des Nutzers per Junction zeigt. `podman cp` schreibt mit `keep-id` als UID 1000. In `/tmp` ist nur 1 GB tmpfs, die Bundles liegen deshalb als `~/.harness-sync-*.bundle` im Home-Volume. Alte Klone zeigen mit dem Remote `host` noch auf `/mnt/exchange/in.bundle`, das Skript setzt die URL deshalb bei jedem Holen neu.
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

### Gateway (`crates/harness-gateway`)

- **Keine Abhängigkeiten, keine Workspace-Vererbung** in `Cargo.toml`. Das Gateway-Image kompiliert das Crate allein und offline in der VM, aus den in `image.rs` (`GATEWAY_IMAGE`) eingebetteten Quellen. Neue Dateien unter `src/` dort eintragen, sonst schlägt `gateway_context_is_complete` fehl. Release-Profil steht im Containerfile (`CARGO_PROFILE_RELEASE_*`), weil Profile in Member-Crates ignoriert werden.
- **Nach jeder Änderung am Gateway-Code (auch `cargo fmt`) erst `cargo build`, dann `harness image build gateway`.** Die CLI bettet die Quellen beim Kompilieren ein. Eine alte Binary baut also den alten Stand, und die VM-Tests melden dann „veraltet“. `ensure_running` ersetzt Gateway-Container aus älteren Images automatisch.
- **Sicherheitsinvariante:** Das Gateway verbindet sich nur mit Adressen, die `address::is_public` erlaubt. Das gilt in jedem Modus und auch für IP-Literale. Tests tauschen Filter und Resolver nur über `Options`; die Binary nutzt immer die Standards. `Host::parse` lehnt numerische Sonderformen (`127.1`, `0x7f000001`) ab.
- Steuerung nur über `harness-gateway ctl` im Gateway-Container (Unix-Socket in `/tmp/harness-gateway/`). Antworten auf eine Anfrage enden mit `ok` bzw. `error …`. `event …`-Zeilen können **vor** dem `ok` kommen.
- `podman exec -i` leitet stdin zeilenweise durch, aber das Ende von stdin kommt nicht immer an. `ctl` endet deshalb, sobald das Gateway die Verbindung schließt (`bye`). Beobachter ohne Ping für 45 s gelten als weg, ihre offenen Fragen werden dann abgelehnt.
- Im Base-Image steht `NO_PROXY=localhost,127.0.0.1,::1`. curl auf `localhost` geht deshalb nicht übers Gateway. In Tests `--noproxy ''` nutzen.
- Während ein `podman pull` läuft, blockieren andere podman-Befehle wie `podman images`, vermutlich wegen des Storage-Locks. Der Pull von `rust:1.97-slim-trixie` (271 MB) dauerte hier 39 Minuten.
- **`llm.harness`** ist die einzige Ausnahme vom Adressfilter: Ziel ist der vom Host gesetzte Server, der Agent wählt nur den Pfad unterhalb von `/api/llm/proxy` (`llm.rs::safe_path`, kein `..`, kein `%`). Das Token kommt nur per stdin (`send_private`), nie als `podman exec`-Argument, und steht weder in `Debug` noch im Status.

### Server-Kopplung und LLM-Durchgang (`apps/server`)

- **Geräte-Tokens sind keine Better-Auth-Sitzungen.** Sie gelten nur in `/api/device` und `/api/llm` (`requireDevice`), nie in `requireUser`. Ein Test prüft, dass ein Geräte-Token an `/api/conversations` usw. 401 bekommt.
- Routen mit eigener Auth (`/api/pairing`, `/api/device`, `/api/llm`) stehen in `app.ts` **vor** dem `api`-Router mit `requireUser`. Hono-Middleware greift in Registrierungsreihenfolge.
- **Honos CSRF-Schutz** behandelt Anfragen ohne Content-Type wie `text/plain` und lehnt sie ohne passende Origin ab. CLI und Gateway schicken keine Origin. Die Token-Routen sind deshalb ausgenommen (`TOKEN_ROUTES` in `app.ts`, `/api/devices` mit Cookie bleibt geschützt). Honos `HTTPException` wurde früher von `handleError` zu 500. Jetzt wird daraus eine JSON-Antwort mit dem richtigen Status.
- Der Durchgang (`routes/llm.ts`) streamt Request und Response. `Content-Encoding` und `Content-Length` der Antwort fallen weg, weil `fetch` schon entpackt hat. Erlaubte Pfade je Protokoll stehen in `ALLOWED_PATHS`. Anthropic und Google hängen die Version selbst an, deshalb kürzt `proxyRoot` deren Basis-URL.
- Rust-Seite: `ureq` mit `native-tls` (SChannel), weil rustls/ring auf `aarch64-pc-windows-msvc` einen C-Compiler bräuchte. `keyring` bleibt auf 3.6, Version 4 hat eine neue API.
- **`harness login` gegen die Dev-Server:** Server mit `PUBLIC_URL=http://localhost:5173` starten, dazu `pnpm --filter @harness/web dev`. Der Bestätigungslink kommt aus `PUBLIC_URL`. Ohne die Variable zeigt er auf `:8080/device`, und dort liefert der Server nur `apps/web/dist` aus, falls vorhanden. Der Build vom 10.10. kennt `/device` noch nicht. CLI und Gateway nutzen weiter `http://localhost:8080` (geprüft am 15.10.).
- **`vite build` (Rolldown) braucht viel Speicher.** Bei knappem Commit-Limit bricht er mit „memory allocation of … bytes failed“ ab. Am 15.10. waren von 32 GB Commit-Limit (RAM plus Auslagerungsdatei) nur noch ~2 GB frei. Für die Entwicklung reicht der Dev-Modus.
- Git Bash: `$!` ist eine MSYS-PID, `taskkill` braucht die Windows-PID. Testserver deshalb über den Port beenden (`Get-NetTCPConnection -LocalPort … | Stop-Process`).

### Instanzen und pi (`instance.rs`, `pi.rs`, CLI `run.rs`)

- **undici (pi) tunnelt auch `http://`-Ziele per `CONNECT`.** `curl` schickt dagegen absolute URLs. Das Gateway nimmt deshalb `CONNECT llm.harness:80` an und liest die HTTP-Anfrage im Tunnel selbst (`read_tunneled_head`). Andere `http://`-Ziele laufen als normaler Tunnel nach den Regeln.
- **Eingebaute Harness-Definitionen werden nicht mehr nach `~/.harness/harnesses/` kopiert.** Eine Kopie friert sie ein und verpasst neue Felder. Genau so fehlte `llm = "pi"`, und pi hatte keine `models.json`. Eine Datei dort ersetzt die eingebaute vollständig.
- Jede Instanz hängt die Volumes der Standard-Harness des Projekts **und** der angeforderten ein, sonst würde `harness shell` neben `harness run` den Container neu anlegen. Geänderte Mounts oder ein neues Base-Image legen die Instanz neu an (Label `io.llm-harness.mounts`). Daten liegen in Volumes.
- Ein frisches Volume, das in ein anderes Volume eingehängt wird (`~/.pi` im Home-Volume), gehört mit `keep-id` dem Agent-Nutzer (geprüft). Am Base-Image ist dafür nichts nötig.
- Installation der Harnesses im kurzlebigen Container `harness-install-<projekt>` (Tools-Volume beschreibbar, `npm_config_cache=/tmp/…`). Erlauben die Regeln die npm-Registry nicht, gibt es eine Freigabe für 15 Minuten. pi: 118 Pakete in ~12 s.
- `pi::write_models` übergibt die Datei als Argument statt über stdin, weil `podman exec -i` das Ende von stdin nicht zuverlässig weitergibt. `harness run` reicht stdin ohne Terminal deshalb nicht durch, sonst wartet pi im Druckmodus ewig.
- **`models.json` gehört dem Nutzer, harness verwaltet nur seine Einträge.** Erkennbar sind sie an `baseUrl` unter `http://llm.harness/` plus Dummy-Key (`pi::is_ours`). `pi::wire` ersetzt nur diese, `pi::unwire` entfernt sie nach `logout`. Eine kaputte Datei wird als `models.json.bak` gesichert. Das Volume `harness-cfg-pi` teilen alle Projekte, auch `scripts/smoke-run.sh` schreibt hinein und räumt per `logout` wieder auf.
- Nach `harness logout` lehnt der Server das Token sofort ab (401), auch für Gateways, die noch laufen (geprüft).
- **`harness.exe` ist gesperrt, solange der Nutzer `harness run` nutzt.** `cargo build` meldet dann „Zugriff verweigert (os error 5)“. Die laufende Datei lässt sich umbenennen (`mv target/debug/harness.exe target/debug/harness.exe.old`), danach baut cargo neu. Die Sitzung des Nutzers läuft dabei weiter. Den Prozess nie beenden.
- `scripts/smoke-run.sh` nicht laufen lassen, während der Nutzer pi nutzt. Es ändert `models.json` im gemeinsamen Volume `harness-cfg-pi`, und danach fehlt dort die Anbindung an seinen Server bis zum nächsten `harness run`.
- `harness sync` holt nur Commits auf `agent/<projekt>`. Uncommittete Dateien und einen anderen ausgecheckten Branch meldet es (`sync::workspace_status`).
- **Hinweise an den Agent** (`briefing.rs`): `harness run` hängt sie über `prompt_flag` der Harness-Definition an deren System-Prompt (pi: `--append-system-prompt`, landet in pis `<addendum>`, ~500 Token). Geprüft mit einem Mock-LLM, das die Anfragen mitschreibt.
- **pi ruft von sich aus `pi.dev` auf:** Versionsprüfung, Telemetrie und vor allem beim Start der interaktiven Oberfläche (nicht mit `--print`) dreimal für die Modellliste, sobald Anbieter eingerichtet sind. Mit Rückfragen im Terminal hieße das: bei jedem Start eine Frage. Nur `PI_OFFLINE=1` stellt alles ab (geprüft am 16.10.), Modellanfragen laufen weiter. `[env]` in `pi.toml` setzt es zusammen mit `PI_SKIP_VERSION_CHECK` und `PI_TELEMETRY=0`. pi nutzt dann seine mitgelieferte Modellliste, neue Modelle kommen also mit einem neueren pi. Neue Harnesses vorher auf solche Aufrufe prüfen: interaktiv starten, dann `harness net status`.

### VM-Dienst (`crates/harness-vmd`, CLI `vmd.rs`, `vm.rs`, Kern `vmd.rs`)

- **Der Dienst läuft als SYSTEM und darf nichts ausführen oder lesen, was ein Nutzerprozess ändern kann.**
  - podman bekommt eine von Grund auf neue Umgebung (`env_clear`): `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `ProgramData`, `TEMP` und `CONTAINERS_CONF` zeigen in `C:\ProgramData\llm-harness\vmd`. Allein über eine `containers.conf` ließe sich sonst `helper_binaries_dir` setzen, und podman würde diese Programme als SYSTEM starten. Auch `%ProgramData%\containers\containers.conf` liest podman, und dort dürfen normale Nutzer Ordner anlegen.
  - `vmd.toml` erlaubt podman nur unter Program Files und als `user_sid` nur `S-1-5-21-…` (die SID landet im SDDL der Pipe).
- **Der Installer sichert `C:\ProgramData\llm-harness` samt Besitzer ab:** `icacls /setowner Administratoren`, `/reset`, dann `/inheritance:r` mit SYSTEM und Admins voll, der Nutzer nur lesen. Ein Ordner, den ein Nutzerprozess vorher angelegt hat, gehört danach nicht mehr ihm.
- **Übernahme der Machine:** Die JSON (ohne `Mounts`) und `harness.ign` kommen in `vmd\home\.config\containers\podman\machine\hyperv\`. Die eigene JSON des Nutzers wird umbenannt. Disk (VHDX) und SSH-Key bleiben im Profil des Nutzers: Hyper-V-Admins können ohnehin jede VHDX einhängen, und den Key liest podman nur. `MachineConfig::default_path` bevorzugt die Kopie des Dienstes.
- Pipe `\\.\pipe\llm-harness-vmd`: SDDL `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;<Nutzer-SID>)`, `PIPE_REJECT_REMOTE_CLIENTS`. Die erste Instanz entsteht mit `FILE_FLAG_FIRST_PIPE_INSTANCE`, damit niemand den Namen vorher besetzt. Eine Anfrage pro Verbindung, JSON pro Zeile. `status` wartet nicht auf einen laufenden Start.
- **„Läuft“ heißt: SSH grüßt** (`vmd::ssh_answers` liest `SSH-`). Ein offener TCP-Port reicht nicht, gvproxy nimmt Verbindungen auch an, wenn sshd in der VM hängt (14.10.). Dann startet der Dienst die VM neu.
- **Vor jedem Start** (PowerShell, Name nur über Umgebungsvariable): gespeicherten Zustand verwerfen, `AutomaticStopAction ShutDown`, Startspeicher klein (`Set-VMMemory`), Ignition-KVP-Reste entfernen. Nach einem gescheiterten Start: hart ausschalten, `podman machine stop` (setzt podmans „starting“ zurück), Reste entfernen.
- Ausgaben von Kindprozessen werden nur kurz nach deren Ende noch gelesen (`OUTPUT_GRACE`): Ein Enkelprozess (bei `cmd` gesehen, bei podman denkbar: gvproxy) hält die Pipe sonst ewig offen.
- Log: `C:\ProgramData\llm-harness\vmd\vmd.log`. Zum Testen als Admin: `harness-vmd console`.
- **Ungeprüft bis zur Einrichtung durch den Nutzer:** podman als SYSTEM mit dieser Umgebung, die übernommene JSON, der Start ohne 9p-Freigabe. Ohne Admin-Rechte geprüft sind die Konfiguration, die Pipe mit echter Nutzer-SID, die Timeouts und dass sich Dienst und Installer ohne Erhöhung verweigern.

### Merge-Prüfung (`harness-core/src/review.rs`, `HostRepo::incoming`, CLI `review.rs`)

- Diff immer mit `--no-ext-diff --no-textconv --no-renames` und `core.quotePath=false`: Eine `.gitattributes` vom Agent darf keine Programme wählen, die beim Prüfen laufen. Die Patch-Größe ist begrenzt (32 MB, dann „zu umfangreich“).
- Im `-U0`-Patch hängt git bei Namen mit Leerzeichen einen Tab an die `+++`-Zeile; ungewöhnliche Namen kommen C-quotiert (`unquote`). Binärdateien haben keine `+++`-Zeile, sondern `Binary files … and b/<pfad> differ`.
- Regeln sind absichtlich grob und reden nicht mit dem Agent. Neue Regeln brauchen einen Test in `review.rs`, auch einen gegen Fehlalarme (`ordinary_changes_stay_quiet`). Ähnliche Paketnamen: Abstand 1 zu `POPULAR`; legitime Nachbarn (`preact`, `just` …) stehen selbst in der Liste.
- Fundtexte und Pfade stammen vom Agent: Ausgabe nur über `review::excerpt` bzw. `sanitize`. Richtungs- und Nullbreitenzeichen erscheinen dabei als `<U+202E>`.

### Terminal-Relay (`harness-cli/src/terminal/`, `harness-core/src/pty.rs`)

- **`harness run`/`shell` mit Terminal (Windows):** podman läuft auf einer eigenen Pseudo-Konsole (ConPTY), die harness ans echte Terminal weiterreicht. Nur so kann harness Rückfragen des Gateways im selben Fenster zeigen. `HARNESS_DIRECT_TERMINAL=1` schaltet das ab, dann hängt podman direkt am Terminal, Rückfragen gehen dann über `harness net watch`.
- **Die innere Pseudo-Konsole verlangt beim Start win32-input-mode** (`ESC[?9001h`). Das äußere conhost übernimmt das, danach kommen Tasten bei harness als `ESC[Vk;Sc;Uc;Kd;Cs;Rc_` an statt als Zeichen. Die Rückfrage entschlüsselt das (`screen::KeyReader`), sonst wird alles roh weitergereicht. Folgen können über zwei Lesevorgänge verteilt sein, ein einzelnes ESC am Ende ist deshalb noch kein Esc.
- **pi läuft standardmäßig im Vollbild** (alternativer Bildschirm, `tuiMode = "fullscreen"`). Die Rückfrage nutzt dann keinen eigenen alternativen Bildschirm, sondern zeichnet darüber. Danach erzwingt eine kurze Größenänderung, dass pi neu zeichnet. Läuft das Programm im normalen Bildschirm, schaltet harness selbst auf den alternativen und danach zurück (`screen::AltScreen` verfolgt den Zustand).
- Vor dem Start schiebt harness den Fensterinhalt per Zeilenumbrüchen in den Scrollback, weil die Pseudo-Konsole oben links zu zeichnen beginnt.
- **Diagnose:** `HARNESS_TERMINAL_LOG=<datei>` schreibt den rohen Verkehr mit (enthält jede Taste, also nur bei Bedarf). Ein Test fährt harness selbst in einer Pseudo-Konsole (`tests/terminal_vm.rs`, `TERMINAL_VM_LOG=<datei>` für die Ausgabe des simulierten Terminals). Das simulierte Terminal beantwortet keine Abfragen (Farben, Kitty-Tastatur). pi kommt trotzdem hoch.
- Shell-Escapes in Python-Heredocs aus Git Bash gehen oft kaputt (`\\x1b` wird zum echten ESC). Für Code mit Backslashes das Edit-Werkzeug oder eine Skriptdatei nehmen.
- **RAM der VM:** Sie startet mit 1 GB und wächst nur, wenn Windows Speicher frei hat. Während der pi-Läufe am 14.10. nahm die VM danach keine SSH-Verbindungen mehr an: TCP ging, der Banner blieb leer, gvproxy lief. Vermutlich war der Speicher knapp. Abhilfe war ein Neustart der VM.

### Testen ohne den Browser des Nutzers

`pnpm --filter @harness/web e2e` startet einen isolierten Stack und klickt den Chat-Ablauf in einem Headless-Browser durch:
- Mock-LLM im OpenAI-Format auf :8091
- Server auf :8090 mit Wegwerf-DB
- Vite im Dev-Modus (StrictMode) auf :5174
- Browser: installiertes Edge/Chrome via `puppeteer-core`, nichts wird heruntergeladen

Screenshots landen in `apps/web/e2e/.output/`. Bei UI-Änderungen laufen lassen und die Screenshots ansehen. Den Chrome des Nutzers nicht ohne Rückfrage fernsteuern.
