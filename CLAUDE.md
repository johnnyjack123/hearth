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
- **VM für Phase 2 läuft:** Podman-Machine `harness` (Hyper-V, Fedora 44 arm64, rootless). Sie teilt noch den alten Austauschordner `%LOCALAPPDATA%\llm-harness\exchange` → `/mnt/exchange`; harness nutzt ihn seit dem 16.10. nicht mehr (Bundles per `podman cp` bzw. `podman exec cat`). Startspeicher 1 GB, dynamisch bis 4 GB. Container-Befehle aus Git Bash: `spikes/phase-0/pm.sh <podman-args>` (liest SSH-Port und -Nutzer aus der Machine-JSON). **Starten und Stoppen der VM** per Admin-PowerShell durch den Nutzer, bis er `harness vmd install` ausgeführt hat. Das hat er am 17.10. getan, seitdem gehört die Machine dem Dienst: `harness` startet die VM selbst, `harness vmd start|stop|restart` geht ohne Admin.
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

  Offen in Phase 2: Rückmeldung des Nutzers zum Relay in Windows Terminal. Remote `origin` = GitHub `johnnyjack123/llm-harness`. Seit dem 18.10. ein Feature-Branch pro Block (zuletzt `feat/research`), jeweils nach `main` gemergt.
- **Phase 4 (Web-Chat ↔ Umgebung) gebaut, 17./18.10.:**
  - Der Hintergrunddienst (`harness daemon`, gestartet von `harnessd.exe` bei der Anmeldung) hält einen WebSocket zum Server (`/api/device/tunnel`). `harness login` richtet ihn ein.
  - `harness init` meldet das Projekt sofort beim Server; ein Web-Chat wählt es unter „Umgebung“.
  - Nachrichten gehen dann an pi im RPC-Modus in der Instanz. Im Chat erscheinen jeder Vorbereitungsschritt, Werkzeugaufrufe, Rückfragen des Gateways sowie Kontext und Kosten.
  - Siehe Stolpersteine „Web-Chat ↔ Gerät“ und „Hintergrunddienst“.
- **Web-Fetch in normalen Chats, 18.10.:** Ohne Umgebung lädt das Modell Seiten über das Werkzeug `web_fetch` direkt im Server. Nur öffentliche Adressen; Hosts, die der Nutzer nicht selbst genannt hat, braucht eine Freigabe im Chat. Admin-Schalter „Webseiten laden“.
- **Block „Recherche“, 18.10.:**
  - Prompt-Injection-Funde mit Fundstelle und einem Satz, warum das gefährlich ist (Web und pi)
  - nur der Hauptinhalt von Seiten (Readability)
  - Websuche `web_search` mit Recherche-Schalter im Composer (SearXNG im Compose-Profil `search`, wahlweise Brave oder Tavily)

  Der echte Suchlauf steht noch aus; der Nutzer testet auf dem Pi.
- **TLS im Gateway, 17.10.:** `llm.harness` geht auch an einen `https://`-Server (rustls im Gateway, Feature `tls`). Server mit eigener CA über `[server] ca` in `config.toml`, Anleitung mit Caddy in `docs/deploy.md`.
- **Richtungsentscheidung:** Erster und vorerst einziger Sandbox-Provider ist **Podman + Hyper-V mit Linux-VM im Isoliert-Modus** (siehe ARCHITECTURE.md §3). Windows Sandbox wurde verworfen (RAM, kein Exec-I/O, Absturz; `docs/spikes/phase-0b-windows-sandbox.md`). Weitere Provider kommen später über die Provider-Schnittstelle. Bitte keinen zweiten Provider „nebenbei“ anfangen. Ein ungesicherter **Host-Modus** ist vorgemerkt, aber noch nicht dran.
- Nächstes: siehe PROGRESS.md „Next step“ (Test des Nutzers mit Websuche auf dem Pi, dann Zweitmeinung zu Warnungen und Stopp der VM im Leerlauf).
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
cargo test -p harness-core --test gateway_vm -- --ignored --nocapture     # Gateway: Voreinstellungen, Sperren, Rückfrage, llm.harness auch per https:// mit eigener CA (braucht Internet)
cargo test -p harness-core --test instance_vm -- --ignored --nocapture    # Instanz, Installation über das Gateway, models.json
cargo test -p harness-core -- --ignored --test-threads=1                   # alle VM-Tests nacheinander (~2 min)
cargo test -p harness-cli --test terminal_vm -- --ignored --test-threads=1  # harness shell/run pi in einem simulierten Terminal (Rückfragen, Strg+C, Größe)
node --test crates/harness-core/pi/guard.test.ts   # Muster des pi-Schutzes gegen Prompt-Injection (läuft auch in cargo test, wenn node da ist)
bash scripts/smoke-run.sh             # Phase 2 komplett: Server + Mock-LLM, login, run pi, shell, sync, merge, logout (~40 s)
bash scripts/smoke-connect.sh         # Phase 4 komplett: connect, Chat verknüpfen, pi per RPC, Rückfrage im Chat beantworten, sync (~1 min)
bash scripts/gateway-lock.sh          # images/gateway/Cargo.lock neu aus Cargo.lock (nach cargo update / neuen Gateway-Abhängigkeiten)
target/debug/harness init             # Ordner als Projekt anlegen (mit Git), beim Server melden, Dienst sicherstellen
target/debug/harness daemon [status|start|stop|install|uninstall]   # Hintergrunddienst für Web-Chats (Protokoll: ~/.harness/daemon.log)
target/debug/harness connect          # dasselbe im Terminal zum Zuschauen (nur ohne laufenden Dienst; bestätigt Chats bei confirm = true)
target/debug/harness project remote on|off [--confirm|--no-confirm]   # Projekt für Web-Chats an- oder abmelden
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

### Dateien schreiben (Claude Code)

- **Unicode-Escapes in Werkzeug-Eingaben:** Vierstellige Escapes wie `\u` + `200B` kommen als echtes, unsichtbares Zeichen in der Datei an, und `\\` wird zu `\`. Im Code deshalb immer die Klammerform `\u{200B}` schreiben (in Regex mit Flag `u`), und Zeilen mit vierstelligen Escapes nicht per Edit anfassen (der Vergleich scheitert), sondern per Skript nach Zeilennummer. `apps/server/test/source.test.ts` meldet rohe unsichtbare Zeichen und Steuerzeichen in allen versionierten Textdateien („Trojan Source“), auch aus Merges von Agents.

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

- **Keine Workspace-Vererbung; Abhängigkeiten nur für TLS** (rustls mit ring, webpki-roots, hinter dem Feature `tls`). Sonst bleibt alles Standardbibliothek. Das Gateway-Image kompiliert das Crate allein in der VM aus den in `image.rs` (`GATEWAY_IMAGE`) eingebetteten Quellen, mit `images/gateway/Cargo.lock` und `--locked`. Die Crates kommen dabei von crates.io. Neue Dateien unter `src/` dort eintragen, sonst schlägt `gateway_context_is_complete` fehl. Release-Profil steht im Containerfile (`CARGO_PROFILE_RELEASE_*`), weil Profile in Member-Crates ignoriert werden.
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
- Beim Nutzer eingerichtet am 17.10.: podman als SYSTEM mit dieser Umgebung startet die übernommene Machine ohne 9p-Freigabe in 22 s. podman meldet dabei `API forwarding listening on: npipe:////./pipe/docker_engine`; diese Pipe gehört jetzt SYSTEM, harness braucht sie nicht.
- Ohne laufenden harness-Server startet `harness run` trotzdem, mit Warnung. pi behält dann seine `models.json` vom letzten Mal, Anfragen ans Modell klappen erst wieder mit Server.

### Merge-Prüfung (`harness-core/src/review.rs`, `HostRepo::incoming`, CLI `review.rs`)

- Diff immer mit `--no-ext-diff --no-textconv --no-renames` und `core.quotePath=false`: Eine `.gitattributes` vom Agent darf keine Programme wählen, die beim Prüfen laufen. Die Patch-Größe ist begrenzt (32 MB, dann „zu umfangreich“).
- Im `-U0`-Patch hängt git bei Namen mit Leerzeichen einen Tab an die `+++`-Zeile; ungewöhnliche Namen kommen C-quotiert (`unquote`). Binärdateien haben keine `+++`-Zeile, sondern `Binary files … and b/<pfad> differ`.
- Regeln sind absichtlich grob und reden nicht mit dem Agent. Neue Regeln brauchen einen Test in `review.rs`, auch einen gegen Fehlalarme (`ordinary_changes_stay_quiet`). Ähnliche Paketnamen: Abstand 1 zu `POPULAR`; legitime Nachbarn (`preact`, `just` …) stehen selbst in der Liste.
- Fundtexte und Pfade stammen vom Agent: Ausgabe nur über `review::excerpt` bzw. `sanitize`. Richtungs- und Nullbreitenzeichen erscheinen dabei als `<U+202E>`.

### Server-Image (`apps/server/Dockerfile`, `.github/workflows/server-image.yml`, `docs/deploy.md`)

- Multi-Stage-Build mit Kontext Repo-Root, `pnpm deploy --prod` für die Produktions-Abhängigkeiten. **`pnpm deploy` kopiert den ganzen Paketordner**, also auch `apps/server/data` mit Entwicklungsdatenbank (verschlüsselte Keys) und `server-secret`. Deshalb schließt `.dockerignore` `**/data`, `*.db*` und `server-secret` aus. Nie entfernen.
- `better-sqlite3` 13 bringt Prebuilds für linux-x64/arm64 mit (glibc und musl), im Image wird nichts kompiliert.
- Workflow: amd64 auf `ubuntu-24.04`, arm64 nativ auf `ubuntu-24.04-arm` (geht auch im privaten Repo). Jede Architektur wird vor dem Push gestartet und geprüft, dann entsteht ein Multi-Arch-Manifest in GHCR. Ein neu hinzugefügter Workflow startete beim ersten Push nicht; `gh workflow run server-image.yml` half, danach lösen Pushes aus.
- Compose reicht `.env` per `env_file` an den Server. Leere Variablen gelten im Server als nicht gesetzt (`loadConfig`), weil Compose `${VAR:-}` leer übergibt.
- `.env*`-Dateien darf Claude Code hier nicht schreiben (Read-Deny-Regel des Nutzers). Die Vorlage steht in `docs/deploy.md`.

### Schutz gegen Prompt-Injection (`harness-core/pi/guard.ts`, `briefing.rs`)

- Die Erweiterung liegt im Tools-Volume unter `/opt/harness-tools/harness/pi-guard-<hash>.ts`, der Name folgt dem Inhalt. Nur der Installer-Container schreibt dort (`instance::write_tool_file`), Instanzen lesen nur. `harness run` lädt sie per `-e`, wenn die Harness-Definition `guard = "pi"` hat. Schlägt das Ablegen fehl, startet pi mit Hinweis trotzdem.
- Sie hängt sich an `tool_result`, warnt nur und blockiert nie. Eingebettete Anweisungen erkennen ist grundsätzlich unzuverlässig, wirklich schützt die Sandbox.
- pi (Node 24) lädt `.ts`-Erweiterungen direkt. `node --test` kann `guard.ts` ebenso direkt testen, weil nur `import type` vorkommt.
- **Test ohne echtes Modell:** Ein Mock im OpenAI-Format kann selbst Werkzeugaufrufe schicken (`tool_calls` im Stream, `finish_reason: "tool_calls"`). pi führt sie aus und schickt das Ergebnis zurück. Mit `PI_CODING_AGENT_DIR=/tmp/…` und eigener `models.json` läuft das in `harness shell`, ohne pis gemeinsames Volume zu berühren.
- **ureq und HTTPS:** ureq gab native-tls bisher eine eigene, leere Root-Liste mit, damit scheiterte jede HTTPS-Verbindung. `server::tls_config()` nutzt jetzt den Zertifikatsspeicher von Windows (`RootCerts::PlatformVerifier`); neue Clients bitte darüber bauen.

### TLS im Gateway (`harness-gateway/src/upstream/`, `pem.rs`, `server.rs`)

- **ring baut auf Windows ARM64 nicht** (braucht clang). TLS steckt deshalb hinter dem Feature `tls`, standardmäßig aus. `cargo build/test/clippy` auf dem Rechner bauen ohne, das Image mit `--features tls`. Ein Gateway ohne TLS lehnt `https://` mit Fehler ab und fällt nie auf HTTP zurück. Den TLS-Code prüft `.github/workflows/gateway.yml` (amd64 und arm64). Schneller geht es in einem Wegwerf-Container in der VM: `pm.sh run -d --name … docker.io/library/rust:1.97-slim-trixie sleep 7200`, Crate per `pm.sh cp`, dann `cargo test --features tls` (~15 s).
- **`images/gateway/Cargo.lock`** ist der Workspace-Lock, beschränkt aufs Gateway. Nach `cargo update` oder neuen Gateway-Abhängigkeiten: `bash scripts/gateway-lock.sh`. Der Test `gateway_lock_matches_the_workspace` meldet Abweichungen. Gleiche Versionen mit anderen Features sieht er nicht; das fängt der Workflow (baut den Image-Kontext mit `--locked`). Den ganzen Workspace-Lock nicht einbetten, sonst ist das Image nach jedem fremden Update „veraltet“. Bei einem Konflikt in `Cargo.lock` die Seite mit den Gateway-Versionen nehmen und cargo den Rest ergänzen lassen.
- Das Containerfile holt die Crates in einer eigenen Schicht (`cargo fetch --locked --target <host>`). Codeänderungen laden also nichts nach. Erster Bau: 34 s.
- **Steuerprotokoll:** `llm <url> <token> [<ca>]`, `<ca>` = Zertifikate als base64-DER, kommagetrennt. Geht wie das Token nur per `send_private`. Ein abgelehntes `llm` entfernt auch das alte Ziel.
- **`[server] ca` ersetzt die öffentlichen CAs** für diesen Server, auf dem Host und im Gateway. ureq mit native-tls kennt nur „nur diese Wurzeln“. Namen werden immer geprüft (SNI, IP-SANs bei IP-Adressen); abschalten lässt sich das absichtlich nicht. `ServerCa::load` liest PEM oder DER (`.cer`). `harness logout` entfernt nur `server.url`, `ca` bleibt stehen.
- `https://localhost` geht fürs Gateway nicht: Die VM erreicht den Server über `192.168.127.254`, und darauf ist kein Zertifikat ausgestellt. `server::llm_upstream` verweist dann auf `http://localhost`.
- **Relay mit TLS:** Beide Richtungen teilen sich eine `rustls::ClientConnection` hinter einem Mutex, der nie während Socket-I/O gehalten wird. Wer sendet, nimmt erst das Schreib-Lock des Sockets und gibt dann die Verbindung frei, sonst kämen Records in falscher Reihenfolge an. Handshake mit 15 s Timeout, danach ohne Lese-Timeout (Streams).
- Fehlgeschlagener Handshake: Grund `tls-failed` (Log, `harness net status`), Status 502, Ursache im Text der Fehlerseite (`UnknownIssuer`, „certificate not valid for name …“) und in `podman logs`.
- Cloudflare (example.com) bricht den Handshake ohne SNI ab (`HandshakeFailure`), wenn man per IP-Adresse kommt. Die Namensprüfung testet `gateway_vm` deshalb mit einem eigenen Node-HTTPS-Server im Projektnetz.
- **Testzertifikate** in `crates/harness-gateway/tests/fixtures/`: Wurzel, Zwischen-CA, Server für `harness.test`, `harness-tls-test`, 127.0.0.1 und ::1, 100 Jahre gültig, mit Schlüssel, nur für Tests. Neu erzeugen: `sh crates/harness-gateway/tests/fixtures/generate.sh`. In Git Bash braucht openssl `MSYS2_ARG_CONV_EXCL=/CN`, sonst wird `-subj` zum Pfad.
- **Das Image-Tag gilt für alle Checkouts:** `harness image build gateway` aus einem Branch oder Worktree macht die CLI anderer Checkouts „veraltet“. Vorher die ID merken (`pm.sh image inspect localhost/harness/gateway:latest`), danach zurück mit `pm.sh tag <ID> localhost/harness/gateway:latest`.

### Web-Chat ↔ Gerät (Phase 4: `apps/server/src/tunnel/`, `routes/agent.ts`, CLI `connect.rs`, `harness-core/src/pi_rpc.rs`, `tunnel.rs`)

- **Protokoll** in `packages/shared/src/agent.ts` (zod) und gespiegelt in `harness-core/src/tunnel.rs` (serde). Änderungen immer an beiden Stellen und `TUNNEL_VERSION` erhöhen, wenn alte Gegenstellen sie missverstehen würden. Der Server prüft jede Nachricht des Geräts mit zod und schließt den Socket bei Unsinn (Code 4002). Unbekannte Server-Nachrichten ignoriert das Gerät (`ServerMessage::Unknown`).
- **Der Hub lebt im Speicher** (`DeviceHub`). Ein Neustart des Servers trennt alle Geräte; `harness connect` verbindet sich mit Backoff neu. Sitzungs-ID = Conversation-ID. Ein Gerät darf nur Ereignisse zu Sitzungen schicken, die der Server bei ihm geöffnet hat. Ein zweiter Socket desselben Geräts ersetzt den ersten (4000), ein entferntes Gerät wird sofort getrennt (4003).
- **Agent-Antworten sind normale AI-SDK-Nachrichten:** `ChunkMapper` macht aus Agent-Ereignissen UI-Chunks (`dynamic-tool`, `data-notice`, `data-approval` mit fester ID, damit die beantwortete Frage die offene ersetzt). Speichern, Laden und `useChat` bleiben dadurch unverändert. `AgentDataParts` muss ein `type`-Alias sein, kein `interface`: Die AI SDK verlangt einen Record-Typ, und mit Index-Signatur wären die Datenteile `unknown`.
- **Fehler einer laufenden Antwort** kommen als Codes (`device_lost`, `chat_unlinked` …), die Web-App übersetzt sie (`streamErrorText`). Fehlertexte des Geräts sind schon Deutsch und werden unverändert gezeigt.
- **Das Gerät entscheidet, was läuft:** Nur Projekte mit `[remote] enabled = true` und einer Harness mit `[adapter] kind = "pi-rpc"`, bei jeder Nachricht frisch aus den Projektdateien gelesen. Seit dem 18.10. ist `enabled = true, confirm = false` Standard (Entscheidung des Nutzers: Umgebungen sollen nach `harness init` ohne weitere Schritte im Chat verfügbar sein). Mit `confirm = true` fragt nur `harness connect` im Terminal; der Hintergrunddienst lehnt dann ab. `[remote]` liest `connect` direkt aus der Projektdatei (`project_config`), `Project` hat das Feld absichtlich nicht.
- **Gespeicherte Umgebungen:** Der Server merkt sich die gemeldeten Projekte (`deviceProject`, über Tunnel und `PUT /api/device/projects`). So kann ein Chat sie wählen, während der Rechner aus ist. Schreiben geht erst, wenn er online ist. Ob ein Projekt läuft, prüft nur das Gerät; der Server prüft nur, ob es verbunden ist.
- **Fortschritt:** Jeder Schritt bis zur Antwort ist ein `progress`-Ereignis, im Chat ein Datenteil `data-progress` mit fester ID. Er wird an Ort und Stelle aktualisiert: angekommen, VM, Gateway, Instanz, Arbeitskopie, Installation, Harness-Start, „denkt nach“ bis zum ersten sichtbaren Ereignis. Neue Schritte gehen über `progress::Progress` (Terminal oder Chat), nie über `println!` allein.
- **Kontext und Kosten:** Pro Antwort speichert `metadata.usage` Tokens, `contextTokens` (Eingabe und Ausgabe des letzten Modellaufrufs), `contextWindow` und `cost` in Dollar. Normale Chats rechnen mit dem Katalog von models.dev (`pricing.ts`, täglich geladen, nur die sechs genutzten Anbieter; Ollama zählt als kostenlos). Agents geben pis eigene Kosten weiter, das Kontextfenster kommt aus `get_state`/`set_model`. Ohne Katalog gibt es einfach keinen Preis, nie einen Fehler.
- **Nutzung pro Modellaufruf** (`metadata.usage.calls`, `callUsage` in `pricing.ts`; Agents: je `usage`-Ereignis von pi nur mit Gesamtkosten): Eingabe, davon Cache, Cache-Schreiben, Ausgabe, davon Reasoning, Kosten je Art, dazu `rates` (Preise je Mio. Token). Immer sichtbar (den Entwicklermodus gibt es seit dem 21.10. nicht mehr): als Tabelle hinter dem Beleg-Knopf unter jeder Antwort und als Summen im Tooltip des Kontext-Rings. `@ai-sdk/openai-compatible` liest Cache-Treffer aus `prompt_tokens_details.cached_tokens` und Reasoning aus `completion_tokens_details.reasoning_tokens`; DeepSeek und OpenRouter schicken beides. Cache-Schreiben kennt es nicht. Der Titel des Chats (eigener kleiner Aufruf) wird nicht mitgezählt.
- **pi im RPC-Modus:** `pi --mode rpc --session-id <conversation-id> [--name <Titel>]`, dazu Guard (`-e`) und Hinweise wie bei `harness run` (`run::ready_harness`). pi legt die Sitzung beim ersten Mal selbst an. Alle Projekte haben `/workspace` als cwd, pis Sitzungen liegen deshalb gemeinsam in `harness-cfg-pi`. Zeilen von pi nur an `\n` trennen (JSON darf U+2028 enthalten). Dialoge von pi-Erweiterungen (`select`, `confirm` …) lehnt harness sofort ab, sonst wartet pi bis zum Timeout.
- **Modellwahl:** Der Chat schickt Zugang und Modell mit, `PiModels::by_credential` übersetzt den Zugang in pis Anbieter-ID (`set_model`). Den bisherigen Verlauf bekommt pi nur, wenn seine Sitzung leer ist (`get_state` → `messageCount`).
- **Rückfragen des Gateways** laufen über einen eigenen `watch` pro Sitzung (`ApprovalBridge`). Die Antwort aus dem Chat nimmt denselben Weg wie in `harness net watch` (`net::decide`, „immer“ schreibt die allow-Liste). Im Terminal von `harness run` kann dieselbe Frage parallel beantwortet werden, die erste Antwort gilt.
- **Stop im Chat bricht den Agent ab** (`session.cancel` → pi `abort`). Das gilt auch für Neuladen oder Schließen des Tabs, weil der Server den Abbruch des Requests nicht von Stop unterscheiden kann. Fortsetzbare Streams gibt es noch nicht.
- `tungstenite` mit native-tls; ein Lese-Timeout von 100 ms lässt ausgehende Nachrichten zum Zug kommen (`WouldBlock`/`TimedOut` sind dann kein Fehler). Pings des Servers beantwortet tungstenite beim nächsten Lesen selbst.
- **Testen:** `scripts/smoke-connect.sh` (echte VM, Mock-LLM mit Werkzeugaufruf aus `scripts/mock-agent-llm.mjs`). Der Browser-E2E simuliert das Gerät mit dem `ws`-Paket des Servers (Nodes eigenes `WebSocket` kann keinen `Authorization`-Header). curl puffert Streams, die in eine Datei gehen: `-N`. Umlaute in curl-Argumenten gehen in Git Bash kaputt, Body deshalb aus einer Datei (`--data-binary @…`).
- Bei knappem Commit-Limit scheitert der Browser-E2E mit `ERR_INSUFFICIENT_RESOURCES` oder Timeouts beim Start, auch in unveränderten Schritten. Dann erst Speicher freimachen (parallele Builds), nicht den Test ändern.

### Web-Fetch in normalen Chats (`apps/server/src/webfetch/`, `routes/chat.ts`)

- **Nur öffentliche Adressen** (`address.ts`, Port von `harness-gateway/src/address.rs`), nur Ports 80/443, keine Zugangsdaten in URLs. `parseTarget` vergleicht den Host wie geschrieben mit dem, was der WHATWG-Parser daraus macht; `127.1`, `0x7f000001`, `2130706433`, `%31…` usw. werden so abgelehnt.
- **DNS:** `http.request` mit `agent: false` und eigenem `lookup` (`fetch.ts`). Ist eine Antwort nicht öffentlich, wird der Host abgelehnt; der Socket bekommt genau die geprüften Adressen (kein Rebinding). Kein Pooling, sonst entfiele der Lookup. Weiterleitungen werden von Hand verfolgt, jeder Hop wird neu geprüft.
- **Wann gefragt wird:** Hosts aus den eigenen Nachrichten des Nutzers (URL oder bloße Domain, `www.` egal, Dateinamen wie `README.md` zählen nicht) und mit „für diesen Chat“ erlaubte Hosts laden ohne Frage, alles andere über `data-approval` mit `url`. Die Erlaubnis steht in der gespeicherten Antwort (`grantedHosts`), verschwindet also mit Regenerieren. Offene Fragen liegen nur im Speicher (`FetchApprovals`), Ende des Requests oder 5 Minuten ohne Antwort heißt Nein. Die Rückfrage-Route beantwortet zuerst offene Web-Fetch-Fragen und reicht sonst an das Gerät (`hub.decide`) weiter.
- **Fehler** von `web_fetch` haben die Form `[code:detail] English text` (`parseWebFetchError` in `@harness/shared`). `describeStreamError` reicht sie durch, weil `toUIMessageStream` auch Werkzeugfehler über `onError` schickt; ohne das wäre jeder Werkzeugfehler „The response could not be generated.“.
- **Gespeicherte `tool-web_fetch`-Teile** gehen als echte Werkzeugaufrufe ans Modell (`convertToModelMessages` mit `tools`, damit `toModelOutput` greift, und `ignoreIncompleteToolCalls` für per Stop abgebrochene Aufrufe). Ist Web-Fetch aus, macht `webFetchAsText` Text daraus, sonst lehnen Anbieter Werkzeugaufrufe ohne Werkzeugdefinition ab. Agent-Werkzeuge (`dynamic-tool`) werden dagegen immer Text (`agentToolsAsText`).
- Die Muster gegen Prompt-Injection und die Suche mit Fundstellen (`findInjections`) sind aus `crates/harness-core/pi/guard.ts` kopiert (`injection.ts`), weil pi die Datei allein lädt. Ein Server-Test lädt `guard.ts` per dynamischem `import()` (bleibt so aus dem Typecheck, pis Typen fehlen hier) und vergleicht die Funde beider auf denselben Texten; neue Regeln an beiden Stellen eintragen. Die Sätze, warum ein Fund gefährlich ist, stehen für pi in `guard.ts` (`KINDS`), fürs Web in `i18n` (`webFetch.why`).
- **Unsichtbare Zeichen** (Nullbreite, Soft Hyphen, BOM) sind allein kein Fund: Seiten wie Hugging Face setzen sie massenhaft als Umbruchstellen zwischen Listeneinträge. Die Muster laufen deshalb auf dem Text ohne sie (`visibleText`, Positionen werden zurückgerechnet). Ein Fund entsteht nur, wenn sie eine Wendung tarnen („ig\u{200B}nore …“) oder eine Kette von 4 und mehr bilden. `html_comment` bleibt innerhalb eines Kommentars; vorher reichte ein Treffer über leere Svelte-Marken `<!---->` hinweg bis in sichtbaren Text („AI & ML interests“).
- Fundstellen enthalten unsichtbare Zeichen so, wie sie gefunden wurden. Sichtbar machen erst die Oberflächen: `injection-findings.tsx` (Chips, Tag-Zeichen entschlüsselt) bzw. `reveal()` im Guard (`<U+200B>`, `<versteckt: "…">`, auch Steuerzeichen, damit keine Escape-Folgen ins Terminal kommen). Hinweise vom Gerät: erste Zeile ist die Meldung, weitere Zeilen zeigt der Chat aufklappbar (`AgentNotice`).
- Tests tauschen Adressfilter, Resolver und Ports nur über `AppDeps.webFetch` bzw. `createTestServer(kind, { webFetch })`. Der Browser-E2E startet `apps/server/test/e2e-server.ts` (wie `src/index.ts`, nur 127.0.0.1:8091 für die Testseite des Mock-LLM zusätzlich). Der Server startet über `startServer()` in `src/server.ts`; neue Abhängigkeiten (Hub, Preise) dort verdrahten, nicht in `index.ts`.
- HTML → Markdown mit turndown 7.2 (reines JS). Skripte und Styles werden vorher per Regex entfernt, damit die Umwandlung auf dem Raspberry Pi schnell bleibt. Tabellen werden Markdown-Tabellen (eigene Regeln in `convert.ts`, kein GFM-Plugin).
- **Hauptinhalt:** `@mozilla/readability` auf einem `linkedom`-DOM, danach turndown (`pageToMarkdown`). Rückfall auf die ganze Seite, wenn Readability weniger als 500 Zeichen **oder weniger als 20 % des Seitentexts** liefert. Den Anteil braucht es für Startseiten: Bei heise und SPIEGEL griff Readability einen Werbekasten (Anteil 0,12 bzw. 0,07), Artikel und Doku lagen bei 0,27–0,96 (gemessen am 18.10.). Klassen wie `promo`, `sidebar`, `comment` wirft Readability vorab weg; Fixtures in `test/fixtures/webfetch/` deshalb mit neutralen Klassen bauen, wenn sie solche Fälle nachstellen. Das Modell kann mit `fullPage: true` die ganze Seite holen. Seiten von 1–2 MB brauchen hier ~1 s, auf dem Pi vermutlich mehr.
- **Überschriften vor Readability herauslösen** (`liftHeadings` in `convert.ts`): MediaWiki (Wikipedia) packt jede Überschrift mit „Bearbeiten“-Links in ein `div`, Readability wirft solche linklastigen Blöcke samt Überschrift weg. Bei de.wikipedia.org/wiki/Kaffee blieben 0 von 37 Überschriften, damit gab es keine Gliederung für die Abschnittsauswahl. Herausgelöst wird nur, wenn das `div` neben Überschrift und Links höchstens 10 Zeichen enthält.
- Offen: Ein gekapertes Modell könnte Markdown-Bilder mit Daten in der URL schreiben. In Produktion blockt die CSP (`img-src 'self' data: blob:`), der Vite-Dev-Server sendet keine CSP.

### Websuche und Recherche-Modus (`apps/server/src/websearch/`, Composer `research-toggle.tsx`)

- **Anbieter** hinter `SearchBackend` (`providers.ts`): SearXNG (`/search?format=json`), Brave, Tavily. Treffer werden vereinheitlicht: nur http(s), jede URL einmal, Titel und Ausschnitt als Text ohne Tags, begrenzt. Fehler sind `WebSearchError` mit Code wie bei Web-Fetch (`[rejected:403] …`), die Web-App übersetzt sie (`webSearch.errors`).
- **Die SearXNG-Adresse darf privat sein** (Compose-Dienst `http://searxng:8080`), sie kommt vom Admin. Die Suche läuft deshalb nicht über den Adressfilter von Web-Fetch, die gefundenen Seiten dagegen immer.
- **Keys** liegen in der Tabelle `setting` unter `searchKey.<anbieter>`, verschlüsselt mit der `SecretBox` (Kontext `setting:searchKey.<anbieter>`). `ServerSettings.search.keys` enthält nur Hinweise (`…abcd`). `PublicSettings.webSearch` sagt der Web-App, ob es den Schalter gibt.
- **Recherche-Modus:** `research` im Chat-Request; der Server speichert es in `metadata.research` der Nutzernachricht, so öffnet der Chat mit dem Schalter wie zuletzt. In Recherche bis zu 12 Schritte.
- **Prompt bleibt für den Cache fest:** System-Prompt und Werkzeugliste hängen nur von den Server-Einstellungen ab, nie vom Schalter oder Datum. `web_search` ist definiert, sobald ein Anbieter eingerichtet ist, und lehnt bei ausgeschalteter Recherche ab (`research_off`). Datum und Schalterstand stehen als **Hinweis** vor der Nutzernachricht (`[LLM Harness: Today is … Research mode is on.]`), nur wenn sich etwas geändert hat (`chatNote`). Der Hinweis wird in `metadata.note` (dazu `metadata.date`) gespeichert und von `withNotes` immer gleich vorangestellt, damit spätere Anfragen exakt denselben Anfang haben. System-Nachrichten im Verlauf lehnt die AI SDK ab, deshalb ist er Teil der Nutzernachricht. Das Datum ist das des Servers (Docker: UTC, außer `TZ` ist gesetzt).
- **`KnownUrls`** (`known-urls.ts`): URLs aus Suchtreffern und Links geladener Seiten laden ohne Rückfrage, auch ihre Weiterleitungen. Varianten, die nichts hinzufügen, zählen mit: http/https, mit oder ohne `www.` und Schrägstrich am Ende, weniger Parameter. Was etwas hinzufügt, fragt (`…/a?b=1&x=geheim`, längerer Pfad, anderer Port). Hosts gelten nie als bekannt. Links nur aus dem Text, den das Modell bekam (nach `truncate`). Die Quellen sind Suchtreffer und alle gespeicherten Werkzeugteile des Chats. Fragt eine URL im Recherche-Modus doch, steht in der Karte warum, mit der ähnlichsten gefundenen URL (`approval.research.similar`).
- Im Suchblock tragen gelesene Treffer „gelesen, Quelle n“, gezählt wie die Quellen unter der Antwort (`readPages` in `web-search.tsx`, Vergleich über Host und Pfad).
- Fehlt ein Werkzeug im Request (Recherche aus, Web-Fetch aus), werden alte Aufrufe zu Text (`serverToolsAsText`), sonst lehnen Anbieter die Historie ab.
- **Hilfsmodelle über OpenRouter** (`apps/server/src/helpers/`):
  - Jev reiht die Treffer, das Embedding-Modell hilft bei Abschnitten. Admins stellen in den Sucheinstellungen `helpers` ein (an/aus, Modell-IDs, Standard `typesafe/jev-1.13` und `qwen/qwen3-embedding-8b`).
  - Der Key ist der OpenRouter-Zugang des chattenden Nutzers (`loadHelpers`): der Zugang des Chats, wenn er OpenRouter ist, sonst sein ältester OpenRouter-Zugang. Ohne einen solchen gibt es keine Hilfsmodelle, alles läuft wie vorher.
  - Jev: `POST <origin>/api/alpha/decisions` mit `state` und Fragen `{type: 'score', instructions, criteria: [Stufen, schlechteste zuerst]}`; zurück kommen `answers.<name>.score` (gewichtete Stufe) und `usage.cost`. Alle Treffer gehen in **einen** Aufruf.
  - Scheitert ein Hilfsmodell (Fehler, 10 s Zeitlimit), gilt die Reihenfolge des Anbieters mit Domain-Liste. Der Chat bricht nie ab.
  - Liegt Jevs beste Relevanz unter 30 %, sagt der Modelltext „Search again with other terms“. Bei echten Tests (21.10.) gab Jev ganzen Trefferlisten aus Shop- oder Wörterbuchseiten Werte um 0, wenn die Suchmaschine die Anfrage missverstanden hatte.
  - Jev auf echten Treffern (21.10., 8 Fragen): 0,4–0,75 s und ~0,01–0,02 Cent pro Suche (20 Treffer ≈ 5 000 Token, weil jede Frage einzeln formuliert ist).
  - Der OpenRouter-Account des Nutzers erlaubt nur bestimmte Anbieter: Perplexity- und Voyage-Embeddings scheitern dort mit 404 „No allowed providers“.
  - Kosten der Hilfsaufrufe stehen in `usage.helpers` (mit `model` und `purpose`) und zählen zu `usage.cost`. Der Titel eines neuen Chats zählt jetzt auch dazu (`assignTitle` gibt seine Nutzung zurück, der Server wartet beim Speichern bis zu 3 s).
  - Tests richten die Hilfsmodelle über `AppDeps.helpers.apiUrl` auf Platzhalter, im Browser-E2E auf `mock-llm.mjs`.
- **Domain-Listen** (`domains.ts`): `web_search` hat `kind` (facts, learning, experiences, tech, news). Der Server holt 20 Treffer, bevorzugte Quellen der Art rücken 5 Plätze vor, Pinterest 5 zurück; bei Gleichstand gilt die Reihenfolge des Anbieters. Das Modell bekommt 10, bevorzugte sind markiert. Listen-Einträge treffen die Domain samt Subdomains, ein Eintrag mit Punkt vorn (`.gov`) eine Endung. Die Listen stehen im Code; bearbeiten in der Web-App gibt es noch nicht.
- **Abschnitte statt ganzer Seite** (`webfetch/sections.ts`): `web_fetch` nimmt `focus` (wonach das Modell sucht), `grep` (Wörter für genaue Zahlen) oder `sections` (Nummern aus der Gliederung eines früheren Aufrufs). Vorrang: `sections`, dann `grep`, dann `focus`. System-Prompt (`WEB_FETCH_INSTRUCTIONS`) und Werkzeugbeschreibung sagen: `focus` für Informationen zu einem Thema, `grep` für genaue Zahlen, die ganze Seite, wenn beides nichts bringt.
  - Seiten bis 6000 Zeichen kommen auch mit `focus` oder `grep` ganz. Sonst wird an Überschriften geteilt und jeder Teil über 2000 Zeichen (~500 Token) **rekursiv halbiert**: Schnitt nahe der Mitte (mittlere Hälfte), bevorzugt an Leerzeilen, dann Zeilen, Satzenden, Leerzeichen, nie in einem Codeblock, solange es anders geht. So bekommen auch Seiten ohne erkennbare Überschriften gleich große Abschnitte. Eine Überschrift direkt über einer anderen bleibt beim nächsten Abschnitt.
  - **Wortformen in BM25** (`webfetch/stem.ts`): Kleinschreibung, Umlaute gefaltet (ä → a, ß → ss), Stammform per CISTEM (Deutsch, eigene Umsetzung, Variante ohne Groß-/Kleinschreibung) oder Porter (Englisch, Paket `stemmer`). Die Sprache kommt aus Füllwörtern der Seite. Wortteile zählen halb: Ein Stamm oder Wort von mindestens 4 Buchstaben am Anfang oder Ende eines anderen („lager“ in „lagerung“, „bohnen“ in „kaffeebohnen“). Exakter Treffer und Wortteile sind **getrennte Begriffe mit eigener Seltenheit**, sonst macht „kaffee“ (überall) den seltenen Treffer „kaffeepflanz“ wertlos. Füllwörter der Anfrage („wie“, „man“, „the“) fallen weg. Echter Vergleich auf Wikipedia, Node-, MDN- und Python-Doku (20 Fragen): alt 15/20 richtig gewählt, neu 19/20; 30–100 ms pro großer Seite.
  - **Kürzere Vektoren** (1024 statt 4096 bei Qwen3, Matryoshka-trainiert): auf 9 Fragen derselbe Abschnitt auf Platz 1, Top 5 zu 4–5/5 gleich, Rangkorrelation 0,94–0,98. 256 Dimensionen verlieren sichtbar (Antwort zweimal nur auf Platz 2–3).
  - **Anhänge** (Einzelnachweise, Literatur, Weblinks, References …, `BACK_MATTER`) zählen nicht mit, außer der Fokus fragt nach Quellen. Sie stecken voller Themenwörter und landeten bei Embeddings sonst oben. Weil sie gar nicht eingebettet werden, halbieren sich bei Wikipedia Token und Kosten.
  - **Tempo der Embeddings:** Die Antwortgröße bremst, nicht die Rechenzeit. Qwen3 liefert 4096 Zahlen pro Abschnitt, als JSON 14,7 MB für einen Wikipedia-Artikel (8–15 s). Deshalb `encoding_format: 'base64'`, `dimensions: 1024` (nur Qwen3- und OpenAI-3-Modelle, `SHORTER_VECTORS`) und Teilaufrufe zu 32 Texten parallel. Ergebnis: 1,2–3,7 s, gleiche Trefferquote. Fällt ein Teilaufruf aus, zählen dessen Abschnitte nur nach Stichworten.
  - Echter Vergleich (21.10., 9 Fragen auf Wikipedia, Node-, MDN- und Python-Doku): BM25 allein findet die Antwort 8/9, Embeddings und Kombination 9/9. Embeddings helfen bei Synonymen, Umschreibungen und deutscher Frage auf englischer Seite. `text-embedding-3-small` ist kaum schneller (1,5 s) und kostet das Doppelte, `bge-m3` und `qwen3-embedding-4b` sind nicht besser.
- **`grep`** (`webfetch/grep.ts`): Wörter oder Wendungen, getrennt durch `|`, wörtlich und ohne Groß-/Kleinschreibung. Kein Regex vom Modell, weil Backtracking den Server blockieren könnte. Zurück kommen die passenden Zeilen mit der Überschrift darüber und bei Tabellenzeilen Kopfzeile plus Trenner. Lange Absätze werden auf ±200 Zeichen um die Treffer gekürzt, höchstens 6000 Zeichen. Bei Wikipedia Dresden kamen aus 377 000 Zeichen 5 300 an, die Einwohnerzahl in der ersten Zeile. Achtung: `matchAll` startet beim `lastIndex` eines globalen Regex; vor jedem Durchlauf zurücksetzen.
  - Rangfolge: BM25 (immer, kostenlos) und mit OpenRouter-Key zusätzlich Embeddings (höchstens 200 Abschnitte pro Aufruf, die Frage mit `Instruct: …\nQuery:`-Präfix für Qwen3-Embedding), zusammengeführt per Reciprocal Rank Fusion. Das Modell bekommt ~8000 Zeichen in Seitenreihenfolge (`[§n]`-Zeilen) und die Gliederung.
  - Die Gliederung steht **innerhalb** der Marker und geht in deren Hash ein, weil Überschriften Seitentext sind. Die Prüfung auf Prompt-Injection läuft immer über die ganze Seite.
  - Embedding-Kosten aus `usage.cost`, sonst Preis aus `<apiUrl>/models?output_modalities=embeddings` (24 h im Speicher), `purpose: 'sections'`. Das Modell ist in den Sucheinstellungen änderbar (`helpers.embeddingModel`).
- **Eindampfen langer Chats** (`src/condense.ts`, Schwellen in den Server-Einstellungen unter `condense`, Admin-Karte „Lange Chats eindampfen“). Die neue Nutzernachricht bekommt `metadata.condensed`, wenn eine der Bedingungen gilt:
  - Der Kontext nach der letzten Antwort liegt bei mindestens `atTokens` (Standard 200 000; ohne `usage` geschätzt mit 4 Zeichen je Token) oder bei `atWindowShare` des Kontextfensters (Standard 50 %; der kleinere Wert gilt).
  - Die letzte Antwort ist mindestens `afterIdleHours` alt (Standard 6 h, 0 = nie); dann ist der Cache meist kalt.

  In jedem Fall müssen seit der letzten Markierung mindestens 8000 Zeichen an Seiten und Ausschnitten zusammenkommen. Die Schwellen sind hoch, weil Cache-Treffer bei DeepSeek 1/50 kosten: 200 000 Token im Cache kosten pro Modellaufruf ~0,06 Cent, der Neuaufbau nach dem Eindampfen einmal deutlich mehr.
  - Alle `web_fetch`- und `web_search`-Ergebnisse **vor** der letzten Markierung erhalten im Speicher `condensed: true` (`condenseHistory`). `modelText` macht daraus URL plus Titel (in Markern), `searchModelText` Titel und URLs ohne Ausschnitte und ohne Warnung. Gespeichert bleibt alles, der Chat zeigt weiter alles.
  - Die Markierung wird einmal entschieden und gespeichert. Folgeanfragen bauen deshalb denselben Anfang, und der Cache des Anbieters greift wieder, bis der Chat erneut über die Schwelle wächst. Ein Test vergleicht die Prompts zweier Anfragen.
  - Gekürzte Seiten lädt `web_fetch` ohne Rückfrage neu (`condensedPages`, angefragte und endgültige URL, exakt). Dieselbe URL trägt nichts Neues aus dem Chat.
  - Die Antwort trägt `metadata.condensed` ebenfalls, damit der Chat sofort einen Hinweis zeigt (`CondensedNote` in `messages.tsx`); die Nutzernachricht im Browser kennt die Metadaten des Servers erst nach dem Neuladen.
- **Tests:** `websearch.test.ts` mit Stand-ins für alle drei Anbieter und einem Modell nach Drehbuch (`createScriptedLlm`, mit `inputTokens` für lange Chats), dazu `sections.test.ts` und `condense.test.ts`. Im Browser-E2E spielt `mock-llm.mjs` SearXNG, Jev und das Embedding-Modell und recherchiert bei „Recherchiere“ (Suche, Testseite, dann das dort verlinkte Handbuch mit `focus`).
- **Compose:** Profil `search`, die SearXNG-Konfiguration steht inline unter `configs.content` (Compose ≥ 2.23). `docker compose --profile search config` prüft die Datei lokal; die Docker-CLI ist installiert, die Engine läuft nicht.

### Hintergrunddienst (`harness-cli/src/daemon.rs`, `harnessd.rs`, `connect.rs`)

- **`harnessd.exe`** ist ein zweites Binary im Paket `harness-cli` mit `windows_subsystem = "windows"`, also ohne Konsolenfenster. Es teilt keinen Code mit `harness`. Es startet `harness.exe daemon run` mit `CREATE_NO_WINDOW`; die Kinder (podman, ssh) erben diese fensterlose Konsole, deshalb öffnet sich nirgends ein Fenster. Ausgabe geht nach `<Konfiguration>/daemon.log` (ab 5 MB rotiert). Nach einem Absturz startet es neu, mit Backoff bis 2 Minuten. Exit 0 (gestoppt oder läuft schon) und 3 (nicht gekoppelt) beenden es.
- **Autostart** über `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, Wert `llm-harness`, ohne Admin. `harness login` richtet ihn ein, aber nur ohne `HARNESS_HOME`: Testläufe dürfen keinen Dienst hinterlassen.
- **Eine Verbindung pro Konfiguration:** benannter Mutex `Local\llm-harness-daemon-<hash der Konfiguration>`. Zwei Verbindungen desselben Geräts würden sich auf dem Server ständig gegenseitig ersetzen (4000). `harness connect` verweigert deshalb, solange der Dienst läuft. Gestoppt wird über ein benanntes Ereignis (`…-stop-<hash>`), das die Schleife alle 100 ms prüft.
- Im Protokoll stehen Zeilen mit UTC-Zeit (`utc_stamp`, ohne Datums-Bibliothek). `println!` in `run.rs`/`login.rs` landet dort ohne Zeit, das ist gewollt.
- **TLS:** Der Tunnel nutzt bei `[server] ca` einen eigenen `native_tls::TlsConnector` nur mit dieser CA, wie die HTTP-Anfragen. Ohne CA nimmt `tungstenite::connect` den Windows-Speicher.
- Test des echten Dienstes: `scripts/smoke-connect.sh` mit `harness daemon start` unter `HARNESS_HOME` (eigener Mutex, eigenes Protokoll).
- **Der laufende Dienst sperrt `target\debug\harness.exe` und `harnessd.exe`**, der Autostart zeigt in der Entwicklung genau dorthin. `cargo build` scheitert dann mit „Zugriff verweigert“. Beide Dateien umbenennen (wie bei `harness run`), nicht den Dienst des Nutzers beenden. Der Nutzer holt die neue Version mit `harness daemon stop` und `harness daemon start`.

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
