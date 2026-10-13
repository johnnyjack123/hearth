# Progress

_Last updated: 2026-10-13 · Phase 2 Pakete 1–6 fertig (Workspace, PodmanProvider, Base-Image, Konfiguration, Git-Sync, Netzwerk-Gateway), weiter mit Paket 7 (pi)_

## Where we are

Server (`apps/server`) und Web-App (`apps/web`) aus Phase 1 laufen unverändert. Das Repo liegt auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). `main` enthält Phase 1 und Phase 2 bis Paket 6, gearbeitet wird auf `feat/phase-2-sandbox`.

Der Rust-Workspace hat drei Crates:
- `harness-core`: Provider, Images, Konfiguration, Git-Sync und **neu** `gateway.rs` (Regeln aus der Konfiguration, Gateway-Container, Steuerung)
- `harness-cli` (Binary `harness`): `doctor`, `image build [base|gateway]`, `project`, **neu** `net [status|watch|allow|deny|preset|start|stop]`
- **neu** `harness-gateway`: eigener Egress-Proxy (nur std), läuft pro Projekt als `harness-gw-<projekt>`

Ein Gateway lässt sich per `harness net start` starten und per `harness net watch` beobachten und beantworten. Instanzen startet die CLI noch nicht (Paket 8), pi fehlt (Paket 7).

## Next step

Paket 7 der ROADMAP (Harness-Definition pi):

1. **Tools-Volume** `harness-tools` (gemeinsam, für Instanzen read-only unter `/opt/harness-tools`). Die Installation läuft in einem eigenen kurzlebigen Container mit Schreibzugriff, gehärtet und mit Netz über ein Gateway: `install` aus `harnesses/pi.toml` (`npm install -g @earendil-works/pi-coding-agent`), `NPM_CONFIG_PREFIX=/opt/harness-tools`.
2. **Login-Volume** `harness-cfg-pi` für `~/.pi` (`HarnessConfig::persist_paths`, `config_volume`).
3. Prüfen, welche Hosts pi beim Start und beim Login braucht (Log im Gateway: `harness net status`). DeepSeek ist über `llm_apis` immer erreichbar.
4. **Offene Frage klären:** Wie kommt der DeepSeek-Key zu pi (siehe unten)?

Danach Paket 8: `run`, `shell`, `status`, `stop`, `sync`, `merge`. `run`/`shell` rufen `gateway::ensure_running` vor dem Instanzstart, `stop` stoppt Instanz und Gateway.

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor` (alle Zeilen ✓, solange die VM läuft).

## In flight / half-finished

- `PodmanProvider::exec` mit `tty: true` ist gebaut, aber **nicht interaktiv getestet**. Das kommt mit `harness shell` (Paket 8) und muss der Nutzer in Windows Terminal ausprobieren. Ctrl+C und Fenstergröße sind ungeprüft.
- Das echte `~/.harness` des Nutzers ist noch nicht angelegt. Probeläufe liefen mit `HARNESS_HOME` im Scratchpad.

## Decisions made this session

Paket 6 (Gateway):
- **Eigener Rust-Proxy statt tinyproxy**, aus Phase 3 vorgezogen. Der Nutzer wollte „alles erlauben“ und Rückfragen wie bei Claude Code. tinyproxy kann weder Ziel-IPs prüfen (dann wäre „open“ ein Weg zu Host und LAN) noch Anfragen festhalten.
- **Nur Standardbibliothek, ein Thread pro Verbindung** (statt tokio/hyper wie in ARCHITECTURE geplant). Damit baut das Image offline in der VM aus eingebetteten Quellen, ohne crates.io, und braucht ~2 MB RAM. Gegen Cross-Compiling auf dem Host entschieden (Nutzer fragte danach): Die CLI bliebe sonst nicht eigenständig, und jede VM-Architektur bräuchte ein eigenes Target. Kosten: einmalig `rust:1.97-slim-trixie` in der VM (~860 MB auf der Platte).
- **IP-Prüfung in jedem Modus**, nicht nur bei „open“. Das Gateway verbindet sich mit genau der geprüften Adresse, deshalb hilft DNS-Rebinding nicht.
- **`off` = nur LLM-APIs.** Mit gar keinem Netz wäre jede Harness nutzlos. `llm_apis` steht global in `config.toml` (Standard DeepSeek), weil der Anbieter am Nutzer hängt, nicht am Projekt.
- **Rückfragen nur, wenn jemand zuschaut.** Ohne `harness net watch` kommt sofort 403 mit Hinweis, statt den Agent 2 Minuten hängen zu lassen.
- **Laufzeit-Freigaben leben im Gateway** und verfallen beim Stopp. Nur „immer“ bzw. `--always` schreibt in die Projektdatei. „nein“ sperrt bis zum Stopp, damit ein wiederholender Agent den Nutzer nicht mit Fragen flutet.
- **Steuerung per `podman exec … ctl` zu einem Unix-Socket** statt über einen Port. Die VM braucht keinen offenen Port, und Instanzen kommen nicht an die Steuerung.
- Gateway läuft als `nobody` (kein `keep-id`, keine Mounts), 128 MB, 512 PIDs (Threads zählen mit).

## Open questions for the user

- **In Klärung (13.10.), blockiert Paket 7:**
  - Wie kommt der API-Key zu pi, sodass Agent-Läufe aus dem Web-Chat und aus dem Terminal denselben Key aus der Web-App nutzen?
  - Wo erscheinen Rückfragen: in der App, im Web-Chat und bei `harness run pi` im Terminal?

  Der Nutzer möchte die Rückfragen in der Tauri-App bzw. im Web-Chat. Im CLI-Modus sollen sie wie bei opencode direkt im Terminal erscheinen.
- Host-Ausführung: Der Nutzer hat noch keinen konkreten Bedarf und möchte, dass sie sich später gut ergänzen lässt (Entwurf in ROADMAP Phase 5).

## Known issues / debt

- **Hoch:** VM-Start nur per Admin-PowerShell (→ Phase 2b). Nach einem gescheiterten Start von Hand aufräumen, siehe `spikes/phase-0.md` §12.2. Die vsock-Registry-Einträge **nicht** löschen.
- **Mittel:** ~350–500 ms pro podman-Aufruf (neue SSH-Verbindung). `gateway::ensure_running` braucht ~4,3 s beim Anlegen und ~2,4 s, wenn das Gateway schon läuft (etwa 8 Aufrufe: Image-Status, zwei Netze, Container, Regeln). Sync hin ~0,6 s, zurück ~0,7 s. Abhilfe: libpod-REST über einen gehaltenen SSH-Tunnel, oder Prüfungen zusammenfassen.
- **Mittel:** Rückfragen hält das Gateway bis zu 120 s fest. pip bricht nach 15 s ab (`--timeout`), npm und Node-`fetch` warten länger. Bei pip ggf. erst freigeben, dann wiederholen.
- **Mittel:** `in.bundle` enthält jedes Mal die ganze Historie. Bei großen Repos inkrementell machen (`--not` mit dem Stand des letzten Exports).
- **Mittel:** CLI-Texte nur Deutsch, Englisch fehlt (Web-UI ist zweisprachig). Die Fehlerseite des Gateways ist zweisprachig.
- **Mittel:** kein Dockerfile/compose für den Server, keine ESLint/Prettier-Konfiguration, kein README (ROADMAP „Phase 1 – Reste“). Web-Haupt-Chunk ~955 kB.
- **Niedrig:** Image-Builds hinterlassen `<none>`-Images in der VM (Build-Stufe ~860 MB, alte Versionen). Platz ist genug (36 GB frei); später `harness image prune`.
- **Niedrig:** `harness net watch` druckt Protokollzeilen während einer offenen Frage erst nach der Antwort. Strg+C beendet ohne `bye`, das Gateway merkt es dann erst nach 45 s.
- **Niedrig:** Host-Key der VM per Trust on first use. Bei neuem Host-Key mit gleichem Port `%LOCALAPPDATA%\llm-harness\podman\harness-<port>\` löschen.
- **Niedrig:** Podman-Hilfsprozesse laufen im Kontext des Admin-Fensters. Schließt der Nutzer es, ist das Verhalten ungeprüft.
- Rechner-Stabilität: Bluescreen `0xA0` beim Start der Windows Sandbox, Abbruch der WSL-VM bei Standby. Nutzer vor riskanten Virtualisierungs-Starts fragen.

## Verification state

- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: ohne Warnungen.
- `cargo test`: 96 Tests grün.
  - `harness-core`: 68 Unit-Tests, neu u. a. Regeln je Voreinstellung, Gateway-Argumente, Konfig-Änderungen mit erhaltenen Kommentaren, vollständig eingebetteter Gateway-Kontext.
  - `harness-gateway`: 22 Unit-Tests für Muster, Adressfilter (öffentlich/privat, IPv6, mapped/NAT64/6to4), HTTP-Parser, Protokoll, Freigaben, Rückfragen und Timeouts.
  - `harness-gateway`, 6 TCP-Tests auf Loopback:
    - Tunnel und Weiterleitung, letztere mit bereinigtem Kopf
    - Sperre privater Adressen mit dem echten Filter
    - Rückfrage: einmal erlauben, dann ablehnen
    - Zuschauer weg ergibt sofort 504
- VM-Tests nacheinander (`cargo test -p harness-core -- --ignored --test-threads=1`): alle 4 grün (`podman_vm`, `base_image_vm`, `sync_vm`, neu `gateway_vm`). In der VM bleibt danach nur das gemeinsame Netz `harness-egress`.
- `gateway_vm`, Ergebnisse:
  - development: `https://example.com` → 403, `api.deepseek.com` → 401 (erreichbar), `npm view react version` → 19.3.0, `pnpm --version` → 12.11.2 (corepack lädt von `registry.npmjs.org`), ohne Proxy kein DNS
  - open: example.com → 200. Gesperrt (403) blieben `host.containers.internal:8080`, `192.168.127.254`, `192.168.127.1`, `10.88.0.1`, `169.254.169.254`, `gateway:8888`, `localhost`, `[::1]`. Die deny-Liste gilt auch hier.
  - Rückfrage über `watch`: Frage nach 358 ms, „einmal“ → 200. Ohne Zuschauer sofort 403, nach `allow` → 200.
- Probelauf der CLI mit `HARNESS_HOME` im Scratchpad:
  - `project add`, `net`, `net start` (4,3 s), `preset open`/`development`, `allow --minutes`, `allow --always` (Projektdatei behält Kommentare), `deny`, `status`, `stop`, `doctor` (6/6 ✓, meldet ein veraltetes Gateway-Image richtig)
  - `net watch` per Skript: Ein Container im Projektnetz ruft `example.net` und `example.edu` auf. Antwort „e“ → 200, „n“ → 403, danach ohne Zuschauer weiter 403.
- Phase 1 seit dem 11.10. nicht erneut geprüft (unverändert): `pnpm -r typecheck` grün, Server-Tests 30/30 (SQLite), E2E 7/7.

---

## Log

### 2026-10-13 (Phase 2, Paket 6)
- `crates/harness-gateway`: `pattern.rs`, `address.rs`, `policy.rs`, `control.rs`, `http.rs`, `gateway.rs`, `proxy.rs`, `main.rs` (`serve`, `ctl`). Image `images/gateway` (Bau in der VM mit `rust:1.97-slim-trixie`).
- `harness-core`: `gateway.rs`, `Provider::create_gateway` und `exec_piped`, `GATEWAY_IMAGE`, `llm_apis` und `ask` in der Konfiguration, `DomainPattern` kommt jetzt aus dem Gateway-Crate.
- CLI: `harness net …`, `image build [base|gateway]`, `doctor` prüft das Gateway-Image.
- Host-Ausführung mit Einzelbestätigung als Idee in ROADMAP Phase 5 aufgenommen.

### 2026-10-12 (Phase 2, Pakete 1–5)
- Cargo-Workspace, Provider, Base-Image, Konfiguration (`~/.harness`), Git-Sync über Bundles. Nutzer wählt DeepSeek für pi.
- Remote `origin` eingerichtet, `feat/phase-1-chat` und `feat/phase-2-sandbox` (bis Paket 5) nach `main` gemergt und gepusht.

### 2026-10-12
- Windows-Sandbox-Spike verworfen, zurück zu Hyper-V + Linux. Machine `harness` angelegt, Phase 0 abgeschlossen.

### 2026-10-11
- Chat-Bug (useChat + StrictMode → `chat-store.ts`) und Login-Schleife behoben, E2E-Test und `CLAUDE.md` angelegt.

### 2026-10-10
- Architektur geplant, Server und Web-App (Phase 1) gebaut, WSL-Spike durchgeführt.
