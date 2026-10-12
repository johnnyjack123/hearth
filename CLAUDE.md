# LLM Harness – Hinweise für Agenten

Selbst gehosteter Multi-User-LLM-Chat (Server + Web) plus geplanter Desktop-Client, der Chats mit isolierten Container-Umgebungen auf dem Rechner des Nutzers verbindet. **Gesamtbild und Entscheidungen: [`ARCHITECTURE.md`](ARCHITECTURE.md). Zuerst lesen.**

## Stand

- **Phase 0 (Spike)** ist erledigt, Ergebnisse in [`docs/spikes/phase-0.md`](docs/spikes/phase-0.md). WSL-Provider gemessen, Hyper-V-Provider teilweise, siehe dort Abschnitt 12.
- **Phase 1 (Server + Web-Chat)** ist umgesetzt: `apps/server`, `apps/web`, `packages/shared`.
- **Richtungsentscheidung:** Erster und vorerst einziger Sandbox-Provider ist **Windows Sandbox** (siehe ARCHITECTURE.md §3). Linux-Provider (Docker, Hyper-V + Linux) folgen später über eine Provider-Schnittstelle. Bitte keinen zweiten Provider „nebenbei“ anfangen.
- Nächstes: Phase 0b (Spike Windows Sandbox: parallele Instanzen, Netzwerk-Allowlist, Exec/PTY, Ordner-Performance, RAM), danach Phase 2 (Sandbox-Kern, `harnessd`).

## Arbeitsweise

- Sprache: Doku und UI-Texte **Deutsch** (UI zusätzlich Englisch), Code-Kommentare, Commits und Bezeichner **Englisch**.
- Commits nur, wenn der Nutzer darum bittet. Konventionen siehe Skill `git-flow`.
- TS/JS: JSDoc für jeden Export, geschweifte Klammern immer (Skill `ts-js-code-konventionen`). Kommentare erklären das *Warum* (Skill `code-comments`).
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
```

Postgres zum Testen: `podman run -d --name harness-pg -e POSTGRES_USER=harness -e POSTGRES_PASSWORD=harness -e POSTGRES_DB=harness_test -p 127.0.0.1:55432:5432 docker.io/library/postgres:17-alpine`, dann `TEST_POSTGRES_URL=postgres://harness:harness@127.0.0.1:55432/harness_test`.

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
- Wenig freier RAM (~3 GB): Die WSL-VM nimmt sich bis zu 7,7 GB, unabhängig von der Podman-Einstellung. Nicht benötigte Machines stoppen.

### Testen ohne den Browser des Nutzers

`pnpm --filter @harness/web e2e` startet einen isolierten Stack und klickt den Chat-Ablauf in einem Headless-Browser durch:
- Mock-LLM im OpenAI-Format auf :8091
- Server auf :8090 mit Wegwerf-DB
- Vite im Dev-Modus (StrictMode) auf :5174
- Browser: installiertes Edge/Chrome via `puppeteer-core`, nichts wird heruntergeladen

Screenshots landen in `apps/web/e2e/.output/`. Bei UI-Änderungen laufen lassen und die Screenshots ansehen. Den Chrome des Nutzers nicht ohne Rückfrage fernsteuern.
