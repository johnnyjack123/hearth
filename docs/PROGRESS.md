# Progress

_Last updated: 2026-11-05 · Release v0.1.17: alle hohen und mittleren Funde der Sicherheitsprüfung behoben (H1–H3, M1–M8), Anbieter bleibt je Chat (Prompt-Cache), Fortschrittsliste ausblendbar, Erinnerungen von Modellen als Vorschläge, Warnung bei uncommitteten Änderungen im Projektordner; davor v0.1.16: Preise je Aufruf, Versionen, Kostendeckel_

## Where we are

Repo auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). Gearbeitet wird auf Feature-Branches (zuletzt `feat/app-catalog`), die nach jedem geprüften Stück per `--no-ff` in `main` gemergt und gepusht werden (Freigabe des Nutzers). `feat/phase-4-chat-env` ist seit dem 18.10. nicht mehr der Arbeitsbranch.

**Terminal:** Im Projektordner startet `harness` (bzw. `harness run pi`) Gateway, Instanz und pi in der VM. Rückfragen des Gateways kommen im selben Terminal. `harness sync`/`merge` holen die Commits des Agents; `merge` prüft sie vorher (Regeln, OSV).

**Web-Chat:**
1. `harness login <Server>` koppelt den Rechner und richtet den Hintergrunddienst ein; `harness init` meldet ein Projekt.
2. Im Chat „Umgebung“ → Gerät → Projekt: pi arbeitet in der VM. Schritte, Werkzeugaufrufe, Rückfragen, Kontext und Kosten erscheinen im Chat. Warnungen des pi-Schutzes zeigen aufklappbar Fundstelle und Grund.
   - **Fortschritt sichern** im Umgebungs-Popover:
     - committet, was der Agent offen gelassen hat, holt es, prüft es (feste Regeln, OSV, Jev) und mergt es in den ausgecheckten Branch
     - schwere Funde halten an; eine Karte im Chat zeigt Funde und den Befehl `harness merge --approve <ID>`
     - **Autosync** macht das nach jeder Antwort selbst
   - Ein neuer Chat kann direkt mit Umgebung starten: Knopf „Umgebung“ auf der Startseite oder „Neuer Chat in dieser Umgebung“ im Popover eines verknüpften Chats.
   - Das Umgebungs-Popover und Einstellungen → Geräte zeigen den VM-Zustand, was sie gerade nutzt und wann sie im Leerlauf stoppt. „VM stoppen“ geht aus dem Browser, die Leerlaufzeit (Standard 30 min, „nie“ möglich) ist pro Gerät einstellbar. Der Hintergrunddienst stoppt die VM danach selbst.
3. **Ohne Umgebung** lädt das Modell Seiten über `web_fetch` im Server, als Hauptinhalt (Readability). Fremde Hosts brauchen eine Freigabe. Bei Verdacht auf Prompt-Injection zeigt die Warnung aufklappbar jede Fundstelle mit Ausschnitt und einem Satz, warum das gefährlich ist.
4. **Recherche-Schalter** im Composer (nur wenn ein Admin unter Administration → Server → Websuche einen Anbieter gespeichert hat): Das Modell sucht mit `web_search` und liest Treffer und Links geladener Seiten ohne Rückfrage. Unter der Antwort stehen die gelesenen Seiten als Quellen.
   - Treffer werden nach Art der Frage gewichtet (Domain-Listen) und mit OpenRouter-Key von Jev gereiht (Relevanz in Prozent).
   - `web_fetch` kann mit `focus` nur passende Abschnitte holen (BM25, mit OpenRouter-Key plus Embeddings), für genaue Zahlen holt das Modell die ganze Seite.
   - System-Prompt und Werkzeuge bleiben für den Cache fest, Datum und Schalter kommen als Hinweis vor der Nachricht.
   - `grep` holt für genaue Zahlen nur die Zeilen mit bestimmten Wörtern (mit Überschriften und Tabellenköpfen).
   - Lange Chats (ab 200 000 Token, 50 % des Kontextfensters oder nach 6 h Pause; in der Web-App einstellbar) bekommen ältere Seiten und Suchen nur noch gekürzt; der Chat sagt das an.
   - Tokens und Kosten stehen immer hinter dem Beleg-Knopf: eine Tabelle je Aufruf des Chat-Modells, eine für die Hilfsmodelle (Jev, Embeddings, Titel), dann die Gesamtsumme. Summen auch im Tooltip des Kontext-Rings.
   - Das Modell nennt am Ende jedes Absatzes seine Quellen; der Chat zeigt sie als Badge (Host, „+n“), beim Zeigen oder Klicken mit Liste. Links im Text sind echte Links. Links, die auf keine gelesene Seite und keinen Treffer des Chats zeigen, listet die Antwort als „Ungeprüfte Links“.
   - Die Karte einer geladenen Seite sagt, ob Abschnitte nach Bedeutung („semantisch“) oder nur nach Stichworten gewählt wurden oder ob das Modell die ganze Seite las.

**Einrichtung:** `harness setup` prüft den Rechner und holt nach, was fehlt (Hyper-V, git, Podman, VM-Dienst und VM, Images, Server, Autostart, PATH). Das Release v0.1.0 auf GitHub hat dafür je Architektur eine Datei `llm-harness-setup-<arch>.exe`, die die Programme mitbringt und setup startet. Daneben liegen die ZIPs für `scripts/install.ps1`.

**pi im Web-Chat und im Terminal** hat `web_fetch`: Der Server lädt die Seite, das Gateway fragt vorher nach dem Host wie bei jeder Verbindung.

**Tray-Symbol:** `harnessd` zeigt Verbindung und VM, öffnet Web-App und Protokoll, stoppt die VM, beendet den Dienst und meldet Rückfragen und angehaltene Merges als Windows-Benachrichtigung.

**Server des Nutzers:** Docker-Compose mit Postgres auf seinem Raspberry Pi, `http://192.168.1.20:8085`. Das Image baut GitHub bei jedem Push auf `main`. Der Windows-Rechner war am 18.10. mit diesem Pi-Server gekoppelt (`harness doctor`).

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor`, dann `bash scripts/smoke-connect.sh` (~1 min). Browser-E2E nur bei freiem Speicher (siehe Known issues).

## Next step

0. **Wünsche vom 6.11.** (v0.1.18): Optionsmenü im Composer, Antwortlänge, Erinnerungen als Hintergrund, Fundstellen im Zusammenhang, Jev mit Umgebung tiefer Fundstellen, leere Zeile im Eingabefeld. Offen aus derselben Nachricht: Kosten des Agents im Projekt `image-3d-effekt` (zweiter Lauf 0,19 $ statt 0,09 $, wenig Cache-Treffer), Analyse folgt.
   **Weg zu 1.0** (ROADMAP): 1, 2 und 4 erledigt (4./5.11., v0.1.17). Als Nächstes: Backup und Umzug des Servers dokumentieren (der Nutzer zieht vom Pi auf den Heimserver um), Neuinstallation auf frischem Windows, eine zweite Sicherheitsprüfung des neuen Stands durch einen frischen Agenten, dann RC. Ungeprüft aus v0.1.17: Smoke-Tests und Gateway-Image in der VM (VM war vom Agent des Nutzers belegt), M8 mit einem zweiten Windows-Konto, M3 mit echtem Git LFS. Beim Projekt `image-3d-effekt` des Nutzers liegt in `stash@{0}` Arbeit vom August, die nie committet war (konfliktträchtig in `agents.md` und `HomeScreen.kt`).

1. **Block „Umgebung im Web-Chat“ samt Tray-App** (Plan des Nutzers vom 23.10., ROADMAP.md Phase 3): Vorschlag für die Reihenfolge
   1. ✅ Zustand von Gerät und VM im Chat, VM im Browser stoppen, Leerlaufzeit einstellbar, Stopp im Leerlauf im Dienst (23.10.). Der Nutzer holt es mit `cargo build`, `harness daemon stop`, `harness daemon start`; der Server auf dem Pi braucht das neue Image (`docker compose pull`, dann `docker compose up -d --no-deps server`). Die Reihenfolge ist egal (Protokoll-Aushandlung).
   2. ✅ neuer Chat direkt mit Umgebung (23.10.)
   3. ✅ „Fortschritt sichern“, Freigabe per ID, Autosync, Jev für den Diff (23./24.10.)
   4. ✅ Tray-Symbol für `harnessd` (24.10.)
   5. ✅ Installer: `harness setup`, Windows-Builds, `install.ps1` (24.10.)
   6. ✅ `web_fetch` für pi (24.10.), fortsetzbare Antworten (25.10.): Neuladen beendet keine Antwort mehr, nur „Stopp“.
   Test des Nutzers mit dem Release v0.1.1 am 25.10.: Einrichtung, Projekt, Verbinden und Agent liefen ohne Probleme; Übernehmen testet er noch. Nachgereicht am selben Tag: `harness init <name>`, `harness project list/remove`, Recherche-Schalter auch in Umgebungs-Chats (`web_search` für pi) und ein gemerkter Schalter für alle Chats.
   Denktiefe und Zusammenfassen im Web-Chat für normale Chats gebaut (25.10., Hilfsagent): Regler mit pis Stufen, Zusammenfassen automatisch und per Knopf im Kontext-Ring.
   Am 25.10. außerdem:
   - `harness vmd clean` räumt die VM auf (alte Image-Stände, Bau-Images, Reste gelöschter Projekte) und lässt den VM-Dienst die Festplatte verkleinern; neue Builds entfernen ihren Vorgänger selbst. Das Verkleinern ist ungeprüft, bis der Nutzer den neuen Dienst installiert hat.
   - Jev-Schalter pro Gerät (Einstellungen → Geräte).
   - Jev bewertet Prompt-Injection-Warnungen (Knopf an der Warnung, automatisch per Einstellung).
   - `harness ssh <projekt>` (Alias von `shell`), `micro` und `nano` im Base-Image; `harness init [pfad] [name]`; Tray-Benachrichtigungen der Smoke-Tests stummgeschaltet.
   - Abgleich mit pi in verknüpften Chats (Tunnel-Protokoll 4): Die Denktiefe des Chats geht mit jeder Nachricht an pi (`set_thinking_level`, „Standard“ heißt pis Mittel). „Verlauf jetzt zusammenfassen“ im Kontext-Ring lässt pi seinen Verlauf zusammenfassen (RPC `compact`). pis eigene Zusammenfassungen erscheinen als Trennlinie in der Antwort, ihre Kosten als Hilfsaufruf. Ältere Geräte bekommen keine Denktiefe und sehen pis Zusammenfassungen als Hinweis; Zusammenfassen auf Wunsch verlangt ein aktuelles harness.
   - Verkleinern der VM-Festplatte im Leerlauf (26.10.): Vor dem Stopp im Leerlauf trimmt der Hintergrunddienst in der VM, der VM-Dienst verkleinert danach die Datei, wenn sie seit dem letzten Mal um 1 GiB gewachsen ist (höchstens einmal am Tag). Beim Test zeigte sich, dass `harness vmd clean` mit dem Dienst bis 0.1.4 schon beim Trimmen scheiterte (`ssh` fehlt dem Dienst, und als SYSTEM lehnt Windows-OpenSSH den Schlüssel des Nutzers ab); jetzt trimmt die CLI. Release v0.1.5.
   - Test des Nutzers mit v0.1.5 (27.10.): `harness vmd clean` verkleinerte die Festplatte von 9,1 auf 6,7 GB. Der Neustart der VM direkt danach scheiterte einmal, und die Fehlermeldung verdeckte den Erfolg; behoben (`compact` meldet das Verkleinern trotzdem), kommt mit dem nächsten Release. Die VM startete danach normal, Images und Volumes waren heil.
   **App-Katalog** (ROADMAP Phase 3, „Plan in Schritten“):
   1. ✅ Kern und CLI (27.10.): Rezepte C/C++, Rust, Go, Java, Deno, Bun, SQLite; ein Volume pro Installation unter `/opt/harness-apps/<app>`; `[apps] enabled` (Standard `["c"]`); `harness app list|on|off|install|update|remove`. `build-essential` ist aus dem Base-Image raus (698 → 454 MB). Wunsch des Nutzers dazu: `vmd clean` behält node/uv und sichert ungesicherte Arbeit gelöschter Projekte als Git-Bundle (`project remove` auch).
   2. ✅ Browser (28.10.): Einstellungen → Geräte „Programme je Projekt“, im Umgebungs-Popover „Programme: …“ mit Schaltern im Dialog; Tunnel-Protokoll 5. Dabei behoben: `harness daemon stop` brauchte ~30 s (gestaute Wakes im Leerlauf-Wächter).
   ✅ **Wünsche des Nutzers vom 28.10.:** Copy-Buttons gehen über `http://` (Rückfall ohne Zwischenablage-API), Mathe mit `\(…\)`/`\[…\]` wird gerendert, Werkzeug `calculate` (math.js im Server, Admin-Schalter „Taschenrechner“). Den Rechner für pi hat der Nutzer am 30.10. nach hinten geschoben (ROADMAP Phase 5).
   **Release v0.1.6 (29.10.):** Der Nutzer meldete „Client zu alt“ im Browser mit dem neuesten Release; v0.1.5 war vom 27.10. und kannte Protokoll 5 noch nicht. Nebenbei: `cargo fmt` aus Schritt 2 nachgeholt, der Gateway-Workflow war daran gescheitert.
   3. ✅ **3a, eigene Programme aus dem Browser** (30.10.): Einstellungen → Geräte → „Eigene Programme“ (apt, npm, pip oder Skript, „Braucht“), Vorlagen pro Konto auf dem Server, Tunnel-Protokoll 6 (`apps.templates`), Kopie auf dem Rechner in `server-apps.json`. Geänderte Vorlagen installieren beim nächsten Start neu, gelöschte werden in allen Projekten ausgeschaltet. Geprüft: Rust-Tests, Server 196/196, Browser-E2E 15/15 (neue Bilder 43–45), Smoke-Test 82 grün, alle vier Arten in der echten VM.
   ✅ **3b, Programmdateien vom Rechner** (30.10.): Browser fragt (Protokoll 7), der Rechner prüft die Datei und legt einen Antrag an, freigeben im Tray oder mit `harness app approve <ID>`, die Freigabe kopiert die Datei nach `app-files/`. Dazu `harness app add|deny|forget`. Geprüft: Rust- und Server-Tests (198), Browser-E2E 15/15 (Bilder 46, 47), Smoke-Test 89 grün, alle Dateiarten außer AppImage echt in der VM. Nicht geprüft: Ja/Nein-Dialog des Tray-Symbols (Win32), echte AppImage.
   ✅ **4, Gateway fertig gebaut** (30.10.): Release-Workflow baut es, `harness.exe` bettet es ein, die VM packt es nur. Dazu auf Meldung des Nutzers: Setup und `install.ps1` warten auf Dienst und Tray und schieben laufende Programme beiseite (erster Lauf scheiterte sonst); bash-Exit-Codes zeigt der Chat neutral als „Exit N“ statt „Fehler“.
   **Beobachtung des Nutzers „Agent langsam“ (30.10.):** Die VM hatte 936 MB RAM, 86 MB frei, keinen Swap; Hyper-V wollte 1,6 GB geben, auf dem Rechner waren 1,2 GB frei. Dazu kostet jeder Schritt einen Modellaufruf mit ~100k Token Kontext. Vorschläge an den Nutzer: zram-Swap in der VM, Dauer je Werkzeugaufruf im Chat anzeigen.
   ✅ **Wünsche und Meldungen vom 30.10. abends** (Branch `feat/vm-memory`, Release v0.1.10):
   - „pi wurde beendet“ war der OOM-Killer der VM (Exit 137; Journal: dreimal `tsc`, einmal pi). zram-Swap nach jedem VM-Start, in der VM des Nutzers geprüft (981 MB). Der Chat nennt bei 137 den Grund.
   - Der Chat zeigt am Umgebungs-Knopf, ob die Arbeit des Agents im Ordner ist (Tunnel-Protokoll 8).
   - Kosten und Kontext nach jedem Modellaufruf statt erst am Ende (auch nach Abbruch richtig), Tokens/s je Antwort und im Schnitt.
   - Denkblöcke klappen zu, die Wahl bleibt; Autoscroll folgt weiter, solange der Nutzer nicht nach oben scrollt.
   - Jev liest große Diffs in Teilen und mehreren Anfragen.
   - Der Agent kennt die harness-Befehle (Briefing) und kann sie dem Nutzer erklären.
   - Der vitest-Fund war ein Fehlalarm unserer OSV-Regel („malicious website“), behoben.
   - Verkleinern der VM-Platte im Leerlauf scheiterte, weil Hyper-V die Datei 2 s nach dem Stopp noch hielt; der VM-Dienst versucht es jetzt eine Minute lang (braucht `harness setup`, das den Dienst aktualisiert).
   - Geprüft: Rust-Tests und clippy, Server 199/199, Browser-E2E 15/15 (Bilder 38, 39), Smoke-Test 93 grün.
   ✅ **31.10., Wünsche des Nutzers** (Branch `feat/chat-files`, Release v0.1.11):
   - Dauer je Werkzeugaufruf an jeder Karte, live hochzählend.
   - Fortschrittsliste: pi hakt in Web-Chats seinen Plan selbst ab (Werkzeug `progress`), der Chat zeigt ihn über dem Eingabefeld.
   - Dateien im Chat: Bilder an das Modell, Dokumente über markitdown als Markdown (eigener Container ohne Netz, Image per GitHub, ~440 MB), Textdateien direkt. In Umgebungs-Chats landen sie in `~/uploads` der Instanz (Protokoll 9).
   - Geprüft: Server 207/207, Rust-Tests und clippy, Browser-E2E 16/16 (Bilder 48, 49), Smoke-Test 99 grün (Fortschrittsliste und Datei mit echtem pi), markitdown-Selbsttest in CI auf amd64 und arm64 (Word, Excel, PowerPoint, PDF, HTML, CSV, ohne Netz).
   - Der Pi braucht die neue Compose-Datei (Dienst `markitdown`, Netz `documents`), siehe `docs/deploy.md`.
   ✅ **1.11., Meldungen des Nutzers** (Branch `fix/costs-memory-effort`, Release v0.1.12). Der Rechner lief noch mit 0.1.8.
   - **„VM stürzt ab“:** Hyper-V meldet keinen Absturz; am 31.10. 15:30 UTC hat der Agent puppeteer, `chrome-headless-shell` und `@sparticuz/chromium` nach `/tmp` (RAM) installiert und Chromium gestartet, der OOM-Killer schlug bei vollem Swap zu, danach stoppte der Nutzer die VM. Ursache dahinter: `hv_balloon: Memory hot add disabled on ARM64`, die VM bleibt bei ihrem Startspeicher (936 MB), das Maximum von 4 GB wirkt nie. Jetzt: Der VM-Dienst startet die VM auf ARM64 mit dem, was Windows frei hat (abzüglich 512 MB, 1–4 GB); das Briefing verbietet Browser und große Dinge in `/tmp`. Braucht den neuen VM-Dienst (`harness setup`, UAC).
   - **Kosten:** DeepSeek rechnet außerhalb von werktags 01–04 und 06–10 Uhr UTC zum halben Preis; pi kennt nur den vollen, models.dev nur den halben. Der Server rechnet DeepSeek jetzt selbst nach Uhrzeit (normale und Agent-Chats). Die „11 Cent bei OpenRouter“ waren ein Agent-Chat, den pi mit DeepSeek direkt und `deepseek-v4-pro` beantwortete: pi kannte `deepseek/deepseek-v4.1-flash-1017` nicht, `set_model` scheiterte, pi blieb bei seinem Standard. Jetzt trägt der Rechner unbekannte Modelle mit den Daten aus dem Katalog des Servers in `models.json` ein und startet pi neu; ein Modell, das pi nicht nutzen kann, beendet die Antwort mit Meldung. OpenRouter-Kosten von Agents liest der Server aus den Antworten im Durchgang (abgerechnet statt geschätzt).
   - **Günstigster OpenRouter-Anbieter** (Wunsch): Der Server lädt OpenRouters Endpunktliste je Modell (öffentlich, 10 min), rechnet je Anbieter einen typischen Aufruf (90 % Eingabe aus dem Cache, Ausgabe 5 %) mit Rabatt und Tageszeit und schickt `provider.order` mit den günstigsten acht, Ausweichen erlaubt. Admin-Schalter, dazu „4-Bit-Anbieter auslassen“ (aus).
   - **Denktiefe:** Im neuen Chat mit Umgebung fehlte der Regler, und die erste Nachricht ging ohne Stufe (pi: Mittel). Jetzt immer sichtbar und mitgeschickt.
   - **Kontext-Füllstand** stimmt (an pis Sitzungsdaten nachgerechnet: DeepSeek behält die Denk-Token im Verlauf); falsch war nur das Fenster im Chat mit dem falschen Modell.
   - Geprüft: Server 218/218, Rust-Tests und clippy, Browser-E2E 16/16 (Bild 37 mit Denktiefe), Smoke-Test 104 grün (unbekanntes Modell lernt pi nach Neustart, Kosten wie abgerechnet), echtes pi nimmt ein zusätzliches Modell beim eingebauten Anbieter `openrouter` an (Wegwerf-Container ohne Netz). Nicht geprüft: echtes OpenRouter mit `provider.order`, der neue VM-Dienst mit größerem Startspeicher beim Nutzer.
   ✅ **2.11., Rückmeldung des Nutzers** (Branch `feat/recent-models`, Release v0.1.13): Windows behält beim VM-Start 1 GB statt 512 MB; Flex-Endpunkte (z. B. `deepinfra/fp8/flex`) fallen aus der OpenRouter-Reihenfolge; die Modellauswahl zeigt oben „Zuletzt genutzt“ (fünf Modelle aus den letzten Chats). Richtigstellung: DeepSeek V4.1 Flash liest Bilder (models.dev und OpenRouter), nur V4 Pro nicht. Geprüft: Tests, Browser-E2E 16/16 (Bild 07b).
   ✅ **2.11. abends, Punkte 2–4 der Liste** (Branches `fix/data-dir`, `feat/instance-memory`, `feat/memory`, `feat/chat-progress`, `feat/long-documents`, `feat/agent-research`):
   - Server auf dem Pi startete nicht (`EACCES … mkdir '/data/files'`): `./data` gehörte root (von Docker angelegt), mit Postgres schrieb vorher nie etwas hinein. Leeren Ordner als uid 1000 neu angelegt; der Server startet seitdem auch ohne beschreibbaren Datenordner.
   - Speichergrenze je Instanz: Speicher der VM minus 384 MB (in der VM des Nutzers getestet: nur der Prozess im Container wird beendet, SSH bleibt da).
   - Gedächtnis für Chats und Agents (Einstellungen → Gedächtnis, `remember`/`forget`, pi-Erweiterung `memory.ts`, Gateway-Pfad `/_memory`, Admin-Schalter).
   - Fortschrittsliste auch in normalen Chats (Werkzeug `progress`, Admin-Schalter).
   - Lange Dokumente (über 16 000 Zeichen) als Anfang plus Gliederung, Nachlesen mit `read_document`; gespeichert bis 500 000 Zeichen. OCR für gescannte PDFs im markitdown-Container (poppler + tesseract deu/eng, erste 30 Seiten; Selbsttest in CI auf amd64 und arm64 grün).
   - Recherche für Agents: Treffer einer laufenden Recherche laden ohne Rückfrage (Gateway fragt den Server über `/_web/known`).
   - Englische Texte für CLI, `harness setup`, Einrichtungsprogramm, Tray und VM-Dienst (Branch `feat/english-cli`): Deutsch, wenn Windows Deutsch zeigt, sonst Englisch; `HARNESS_LANG=de|en` erzwingt eine Sprache (`harness-core/src/lang.rs`, `tr`/`tr!`). Was in den Web-Chat geht, bleibt Deutsch (die Web-App übersetzt selbst). Tests und Smoke-Skripte laufen auf Deutsch (Feature `test-german`).
   - Geprüft: Server 226/226, Rust-Tests und clippy, Browser-E2E 18/18 (vor den langen Dokumenten), Smoke-Test 109 grün mit echtem pi (Gedächtnis, Recherche-Treffer ohne Rückfrage, unbekanntes Modell), Selbsttest von markitdown mit OCR in CI. Die englischen Texte: Rust-Tests, deutsche Hilfe aller Befehle unverändert, `harness --help` und `doctor` in beiden Sprachen angesehen; der Smoke-Test danach lief nicht, weil die VM mangels freiem RAM (1,3 GB) nicht startete (32778).
   - Release v0.1.14.
   ✅ **2.11. spät** (Branch `feat/document-search`): Vektoren je Dokument einmal gespeichert (Suche danach nur noch mit der Frage, über alle Abschnitte statt der 200 besten), `read_document` auch für pi in Web-Chats, OCR Seite für Seite mit Zeitbudget (bis 100 Seiten, Teilergebnis statt Abbruch), Anbieter von OpenRouter unter jeder Antwort („über DeepInfra“). Geprüft: Server 228/228, Rust-Tests und clippy, Browser-E2E 18/18, OCR-Selbsttest in CI (amd64, arm64) mit zwei Seiten und Zeitlimit. Nicht geprüft: `read_document` in echtem pi (Smoke-Test ausgelassen, der Rechner hatte zu wenig freien Speicher für die VM) und die Anzeige des Anbieters im Browser (das E2E hat keinen OpenRouter-Chat). Release v0.1.15. Der Server läuft weiter auf dem Pi; den Umzug auf den Heimserver richtet der Nutzer selbst ein.
   ✅ **3.11.** (Branch `feat/document-read-info`, nur Server und Web): Unter jedem Dokument-Aufruf steht, wie gelesen wurde („1 von 25 Abschnitten · semantisch“, „wörtlich: 3 Treffer“, „ganzes Dokument“), auch bei Agents. Geprüft: Server 228/228, `document-info.test.mjs`, Browser-E2E 18/18 mit Bild 49b. Meldung des Nutzers: Anbieter von OpenRouter erscheint nicht; vermutlich lief auf dem Pi noch das alte Server-Image (offen, Nutzer prüft).
   ✅ **3.11. mittags** (Branches `feat/call-prices`, `feat/versions`, `feat/cost-limit`, Release v0.1.16): Preise je Aufruf in der Abrechnung (Listenpreis des OpenRouter-Anbieters bzw. Katalog zur Uhrzeit), Versionen von Server und harness sichtbar mit Hinweis, welche Seite zu aktualisieren ist (Tunnel-Protokoll 10, Tray), Kostendeckel pro Tag (Admin und eigenes Limit). Sicherheitsprüfung durch einen eigenen Agenten: `docs/security/review-2026-11-03.md`. Geprüft: Server 235/235, Rust-Tests und clippy, Browser-E2E 19/19 (Bild 53). Nicht geprüft: der Smoke-Test mit echtem pi (Speicher knapp), Postgres (kein Testlauf mit `TEST_POSTGRES_URL`), echtes OpenRouter mit Anbieter und Preis.
   **Als Nächstes:** ROADMAP „Weg zu 1.0“: Funde H1–H3 der Sicherheitsprüfung, dann Backup/Umzug dokumentieren, M1–M8, Neuinstallation, Release-Kandidat.
   **Davor:** Test des Nutzers mit v0.1.14 (`harness setup` aktualisiert den VM-Dienst per UAC, erst dann gilt der größere Startspeicher) und `docker compose pull` auf dem Pi (Gedächtnis, OCR im neuen markitdown-Image).
   4. Gateway fertig gebaut ausliefern.
   Danach (Empfehlung an den Nutzer): Gedächtnis und Dateien im Chat.
   Klein: beim Ersetzen eines Instanz- oder Gateway-Containers das alte Image entfernen, wenn es sonst niemand nutzt (CLAUDE.md, „Platz der VM“).
   **Für den Nutzer nach dem nächsten Release:** Das Base-Image wird einmal neu gebaut (node und uv sind wieder in der VM), danach installiert der erste Start jedes Projekts C/C++ (~60 MB, ~1 min). In der VM liegt schon eine Test-Installation von C/C++; `harness app list` markiert sie mit Stern, `harness app update c` ersetzt sie (nötig ist das nicht).
   Vorgemerkt: Gedächtnis für Agents und Chats, englische CLI (ROADMAP Phase 5).
   Der Recherche-Test auf dem Pi lief gut (Rückmeldung 23.10.); offen ist nur, ob das echte Modell die Quellen am Absatzende einhält.
2. Vorschlag, offen: Fällt SearXNG wegen gesperrter Suchmaschinen aus, automatisch auf einen Anbieter mit Key ausweichen (Tavily 1 000 Anfragen im Monat frei). Bisher gibt es nur einen Anbieter zur Zeit. Thesaurus (OpenThesaurus, WordNet, ConceptNet) vorerst nicht: Embeddings decken Synonyme besser ab (Entscheidung mit dem Nutzer, 21.10.). Stammformen und Wortteile sind eingebaut.
3. **Zweitmeinung zu Warnungen:** Jev-Teil gebaut (25.10.); offen ist ein Chat-Modell als Alternative mit Begründung (ROADMAP, eigener Abschnitt).
4. Später (ROADMAP Phase 5): Dokumente per markitdown, Bilder für Vision-Modelle, Windows-Builds auf GitHub.
5. Zum Schluss (Entscheidung des Nutzers): Secrets mit dem Passwort verschlüsseln (ROADMAP Phase 5).
6. Vorgemerkt: DeepSeeks Websuche (`api.deepseek.com/v1/tools/search`) als weiteren Suchanbieter prüfen (ROADMAP, Phase 4 „Recherche“ → Offen).

## In flight / half-finished

Nichts. Alles ist committet, nach `main` gemergt und gepusht; keine Hilfsagenten oder Worktrees offen.

## Decisions made 23.10. (nachts, Schritt 3)

- Der Agent-Branch behält seinen Namen `agent/<projekt>` und wird nach jedem Merge auf den gemergten Stand gesetzt (Nutzer).
- Jev läuft bei jeder Merge-Prüfung mit, sobald der Rechner gekoppelt ist und das Konto einen OpenRouter-Zugang hat; abschaltbar mit `[review] jev = false`.

## Decisions made 23.10. (nachts, nach Schritt 1)

- Übernehmen mergt direkt in den ausgecheckten Branch; schwere Funde gibt nur ein Befehl im Terminal mit der ID des Antrags aus dem Browser frei; „Immer erlauben“ schreibt weiter die allow-Liste.
- Autosync als Schalter: nach jeder fertigen Antwort mergen, wenn alle Prüfungen grün sind, danach neuer Branch vom gemergten Stand in die VM.
- Neu gewünscht: Kontext-Kompaktierung und Thinking-Effort-Regler im Web-Chat, abgeglichen mit pi; Einstellungen zwischen pi und Web gleich halten (ROADMAP Phase 3).

## Decisions made 23.10. (nachts, Schritt 1)

- **Leerlaufzeit Standard 30 min**, wählbar 15 min, 30 min, 1 h, 2 h oder nie; pro Gerät auf dem Server gespeichert, der Dienst bekommt sie über den Tunnel.
- **Leerlauf** = keine Antwort eines Agents, kein harness-Befehl im Terminal, keine Container in der VM, die harness nicht angelegt hat. Wartende pi-Sitzungen halten die VM nicht; sie enden mit dem Stopp, die nächste Nachricht startet VM und pi neu.
- **Stopp aus dem Browser geht immer**, auch wenn etwas läuft; die Web-App fragt dann nach. Laufende Antworten enden mit „Die VM wurde im Browser gestoppt.“
- **Protokoll-Aushandlung statt harter Versionsgleichheit**, damit der Server auf dem Pi und der Rechner nicht gleichzeitig aktualisiert werden müssen.

## Decisions made 23.10.

- **Keine Tauri-App**, stattdessen Tray-Symbol für `harnessd` und alle Einstellungen im Browser (RAM).
- **Umgebungen nur per Terminal** (`harness init`), nie aus dem Web; „Projekte direkt aus dem Web anlegen“ ist verworfen.
- **VM im Browser:** Zustand, sofort stoppen, Leerlaufzeit. „Fortschritt sichern“ im Chat bringt die Arbeit des Agents per sync und merge auf die Platte.
- **Secrets mit dem Passwort verschlüsseln:** ja, aber zum Schluss.

## Decisions made earlier (20./21.10.)

- **Abschnitte ohne Jev:** BM25 plus Embeddings (`qwen/qwen3-embedding-8b`, in den Sucheinstellungen änderbar), zusammengeführt per RRF. Jev bleibt fürs Reihen der Treffer.
- **Das Hauptmodell wählt selbst** zwischen ganzer Seite und `focus` (Wunsch des Nutzers): `focus` für Informationen zu einem Thema, ganze Seite für genaue Zahlen, Tabellen, Code und Zitate. Seiten bis 6000 Zeichen kommen immer ganz.
- **Hilfsmodelle zahlt der Nutzer selbst:** Sein OpenRouter-Zugang (der des Chats oder sein ältester). Ohne Zugang läuft alles wie vorher, nur mit Domain-Listen und BM25.
- **Eindampfen** ab 200 000 Token Kontext (oder 50 % des Fensters) oder nach 6 h Pause, nur wenn mindestens 8000 Zeichen schrumpfen; einstellbar in Administration → Server (Wunsch des Nutzers: Cache-Treffer nicht zu früh verschenken). Die Stelle wird an der Nutzernachricht gespeichert, damit Folgeanfragen denselben Anfang haben (Cache). Gekürzte Seiten laden ohne Rückfrage neu: Dieselbe URL trägt nichts Neues hinaus.
- **Hinweise statt wechselndem System-Prompt** (Idee des Nutzers): Datum und Recherche-Schalter stehen als `[LLM Harness: …]` vor der Nutzernachricht und werden gespeichert.

## Decisions made earlier (18.10.)

- **Suchanbieter:** SearXNG (Compose-Profil `search`) plus wahlweise Brave oder Tavily mit Key, alle hinter `SearchBackend`. So hat der Nutzer es gewählt.
- **Recherche erlaubt exakte URLs, nie Hosts.** Treffer und Links geladener Seiten hat ein Dritter geschrieben, ohne den Chat zu kennen. Eine selbst gebaute URL könnte Chat-Inhalte hinaustragen, auch auf einem bekannten Host. Weiterleitungen einer gefundenen URL sind frei, weil ihr Server sie wählt.
- **Suchanfragen gehen ungefiltert an den Anbieter.** Das lässt sich nicht verhindern; der System-Prompt bittet das Modell, nichts Privates hineinzuschreiben (Hinweis in `docs/deploy.md`).
- **Hauptinhalt mit Anteilsgrenze 20 %:** Auf den Startseiten von heise und SPIEGEL griff Readability einen Werbekasten (Anteil 0,12 bzw. 0,07). Dann lieber die ganze Seite als Information verlieren.
- **Fundstellen werden roh gespeichert**, unsichtbare Zeichen eingeschlossen. Sichtbar machen sie die Oberflächen; im Terminal (pi) werden auch Steuerzeichen ersetzt, damit keine Escape-Folgen durchkommen.
- **Hinweise vom Gerät bleiben Text:** Die erste Zeile ist die Meldung, weitere Zeilen sind aufklappbare Details. So bleibt das Tunnel-Protokoll unverändert (kein `TUNNEL_VERSION`-Sprung).
- Der Test vergleicht pi-Schutz und Server jetzt im Verhalten: Er lädt `guard.ts` per dynamischem `import()` und prüft beide auf denselben Texten.

## Open questions for the user

- Jev-Schwellen (ab 75 % Gefahr schwer, ab 50 % mittel) sind geschätzt; Rückmeldung nach echten Diffs.
- Vorgemerkt (Nutzer, 24.10.): ponytail als optionaler Skill in pi (ROADMAP Phase 5).
- Rückmeldung zur Recherche mit OpenRouter-Key (Reihung, Abschnitte, Kosten) und weiterhin zum Ablauf mit Hintergrunddienst und DeepSeek.
- Der OpenRouter-Key aus dem Chat vom 21.10. sollte nach den Tests widerrufen werden; er steht im Gesprächsverlauf.

## Known issues / debt

- **Mittel:** Brave und Tavily nur gegen Stand-ins geprüft (Formate aus deren Doku); SearXNG läuft beim Nutzer.
- **Mittel:** Jev (`/api/alpha/decisions`, Alpha-API) und Embeddings über OpenRouter nur gegen Stand-ins geprüft; beide Endpunkte antworten ohne Key mit 401 (also vorhanden). Schlägt ein Hilfsmodell fehl, läuft der Chat ohne weiter.
- **Niedrig:** Die Kosten des Titels zählen erst ab jetzt zur ersten Antwort; Embedding-Preise ohne `usage.cost` kommen aus OpenRouters Modellliste (24 h im Speicher).
- **Mittel:** Readability braucht für 1–2-MB-Seiten hier ~1 s, auf dem Pi vermutlich mehrere Sekunden.
- **Mittel:** Laufende Antworten liegen nur im Speicher des Servers; ein Neustart beendet sie (gespeichert ist dann nur die Nachricht des Nutzers).
- **Niedrig:** Recherche im Umgebungs-Chat fragt beim Lesen jedes neuen Treffer-Hosts nach (Gateway), anders als im Web-Chat (ROADMAP Phase 5).
- **Mittel:** „Neu erzeugen“ in einem verknüpften Chat schickt die Nachricht erneut; pis Sitzung enthält die alte Runde noch.
- **Mittel:** pis Sitzungen aller Projekte liegen gemeinsam in `harness-cfg-pi`.
- **Mittel:** VM-RAM knapp (Start 1 GB; am 18.10. 183 MiB frei); ~350–500 ms pro podman-Aufruf.
- **Mittel:** Kein Verbrauch oder Limit pro Gerät im LLM-Durchgang.
- **Mittel:** Markdown-Bilder könnten Daten hinausschicken, nur die CSP der Produktion blockt das (Vite-Dev-Server hat keine).
- **Niedrig:** Ob pis Oberfläche mehrzeilige Warnungen sauber umbricht, ist nur am Code geprüft (`showWarning` → `ThemedText`), nicht im Terminal gesehen.
- **Rechner:** Wenig freier Speicher. Der Browser-E2E scheiterte am 18.10. zweimal mit `ERR_INSUFFICIENT_RESOURCES` und lief beim zweiten Versuch grün. Windows Sandbox nie ungefragt starten.

## Verification state

Stand 28.10. (Rechner, Mathe, Kopieren):
- **Server:** 194/194 (neu: Rechner mit Zahlen, Einheiten, Brüchen, Matrizen, Ableitung; Fehler je Ausdruck; ein Aufruf verstellt den nächsten nicht; Zeitgrenze und Speichergrenze mit Neustart des Workers; Chat mit Werkzeugaufruf, gespeichertem Teil und ausgeschaltetem Rechner).
- **Web:** `node --test apps/web/e2e/math.test.mjs` grün; Browser-E2E 15/15, 0 Konsolenprobleme (ein Lauf davor scheiterte an `ERR_INSUFFICIENT_RESOURCES`): Rechner-Karte, KaTeX inline und abgesetzt, Copy-Button ohne Zwischenablage-API. Screenshot 42 angesehen.
- **Nicht geprüft:** echtes Modell (DeepSeek) mit dem Rechner, Copy-Button auf dem echten Pi über HTTP (nur simuliert), Tabellen-Kopie über den Ersatz.

Stand 28.10. (App-Katalog, Schritt 2):
- **Rust:** `cargo test` grün (harness-core 121, harness-cli 14; neu: App-Nachrichten im Tunnel, Schalten aus dem Browser nur für angebotene Projekte und bekannte Apps, Bericht ohne VM); clippy ohne Warnungen.
- **Server:** 188/188 (SQLite), neu: Bericht nur ab Protokoll 5, `apps.set`/`apps.result`, ungültiger Bericht trennt (4002), Route mit Eigentümer, nicht angebotenem Projekt und ungültigen Namen, Bericht bleibt offline sichtbar.
- **Browser-E2E:** 14/14, 0 Konsolenprobleme (ein Lauf vorher mit `ERR_INSUFFICIENT_RESOURCES` und einer mit Zeitüberschreitung am Denktiefe-Regler, beides vor dem neuen Teil): „Programme: C/C++“ im Popover, Rust einschalten → `apps.set` beim simulierten Gerät, C/C++ gesperrt „an, weil Rust es braucht“, Einstellungen → Geräte aufgeklappt hell; Screenshots 31, 36, 40, 41 angesehen.
- **Echt:** `bash scripts/smoke-connect.sh` 76 grün: Katalog des echten Dienstes im Konto, C/C++ als Standard, SQLite aus dem Browser eingeschaltet (Projektdatei geändert), unbekannte App vom Rechner abgelehnt, beim nächsten Chat installiert und `sqlite3` in der Instanz; `daemon stop` jetzt 1 s.
- **Nicht geprüft:** Postgres (nur SQLite), Dichte- und Themenvarianten außer hell/dunkel, Tastaturbedienung der Schalter nur über Radix.

Stand 27.10. (App-Katalog, Schritt 1):
- **Rust:** `cargo test` grün (harness-core 120, neu: Katalog gültig, eigene Rezepte ersetzen und ergänzen, Abhängigkeiten samt Zyklus, Validierung, Schalten pro Projekt mit Standard `c`, Quittungen, Umgebung, Image-PATH gegen Containerfile, Hinweise an den Agent); clippy ohne Warnungen.
- **VM:** alle `--ignored`-Tests des Kerns grün (9, neu `apps_vm`: Installation mit Quittung, `requires` beim Installieren, schreibgeschützt in `sh` und Login-Shell, gescheiterte Rezepte ohne Reste, alter Stand geht erst nach Neuanlage der Instanz, Sichern von Commit und offener Datei als Bundle). `base_image_vm` prüft jetzt, dass kein Compiler im Image ist.
- **Echt:** alle sieben Rezepte über `harness app install` (264 s), dann in einer Login-Shell der Instanz 17/17: gcc, g++, `-lcrypt`, statisch, make, Rust mit cc-Crate, clippy, Go, Java 25, Deno, Bun, SQLite, natives npm-Modul über node-gyp, Variablen, schreibgeschützt, kein gcc im Base-Image. `harness project remove --yes` mit Commit und offener Datei: Bundle gesichert, per `git fetch` geholt, Volumes weg. `vmd clean` als Trockenlauf mit echter und mit Testkonfiguration. `bash scripts/smoke-connect.sh` 68 grün.
- **Nicht geprüft:** Rezepte auf x64 (nur ARM64 hier), `vmd clean` mit echtem verwaisten Projekt samt Sichern (nur über `project remove` und `apps_vm`), Aufräumen alter App-Stände durch `clean` im echten Lauf (nur `harness app remove` und `apps_vm`).

Stand 24.10. (Schritt 5: Installer):
- **Rust:** Tests grün (neu: PATH-Vergleich, Größe neuer Machines), Clippy ohne Warnungen.
- **Echt:**
  - `harness setup` ohne Terminal auf dem eingerichteten Rechner: alles ✓, Autostart und PATH nur gemeldet
  - `bash scripts/smoke-connect.sh` 52/52, jetzt mit `harness setup --yes --server …` als Einstieg (meldet den älteren VM-Dienst, ändert ihn im Testlauf nicht)
  - `install.ps1` aus ZIP, aus dem Ordner und über eine bestehende Installation in einen Testordner
- **Nicht geprüft:** frische Einrichtung (Hyper-V einschalten, winget, `podman machine init` über den Dienst), dafür fehlt ein frischer Rechner. Der Download über `gh release download` ist nur am Code geprüft.

Stand 24.10. (Schritt 4: Tray-Symbol):
- **Rust:** `cargo test` grün (neu: Statusdatei, Menütexte und -aktionen in Deutsch und Englisch, Status anderer Läufe ignoriert, gezeichnetes Symbol), Clippy ohne Warnungen.
- **Symbol** als Pixel-Abzug in 16, 24 und 32 px auf hellem und dunklem Grund angesehen.
- **Echt:** `VM_STOP=1 bash scripts/smoke-connect.sh` 61 grün, dazu VM-Stopp über das Ereignis des Trays; die Tray-Prüfung selbst lief nach Korrektur des Prüfskripts im kurzen Durchlauf 51/51 grün (Fenster des Trays beim richtigen harnessd, Statusdatei verbunden, nach `daemon stop` Statusdatei weg und harnessd beendet).
- **Nicht geprüft:** Menü und Benachrichtigungen per Klick (kein Fernsteuern des Desktops), Anzeige bei 125/150 % Skalierung, Neustart des Explorers.

Stand 24.10. (Schritt 3: Fortschritt sichern, Autosync, Jev):
- **Rust:** `cargo test` grün (harness-core 107, harness-cli 9, neu: Branch-Helfer nach Merge, Freigabe-Anträge samt ID-Prüfung, Übernahme-Ereignis im Protokoll), Clippy ohne Warnungen.
- **Server:** 159/159, neu:
  - nur Geräte mit Protokoll 3 übernehmen; ältere bekommen `autosync: false`
  - Übernahme-Bericht hängt an der letzten Antwort
  - Gerät ohne Bericht → 502 mit seinem Grund
  - Autosync im Prompt, bleibt beim gleichen Projekt
  - Jev-Route mit Stand-in: Key des Besitzers, Gefahr je Datei, Grenzen, nur Geräte-Token
- **Browser-E2E:** 13/13, 0 Konsolenprobleme; Autosync-Schalter, „Fortschritt sichern“ mit angehaltenem Merge (Karte mit Funden und kopierbarem Befehl), beides nach Neuladen. Screenshot 39 angesehen.
- **Echt:** `bash scripts/smoke-connect.sh` 47/47 gegen die VM:
  - Sichern committet eine offene Datei und mergt in `main`; Agent-Branch in Host und VM danach gleich
  - Autosync nach einer Antwort
  - `.husky/pre-commit` hält den Merge an (Fund `git-hook`, ID)
  - `harness merge --approve` ohne Terminal abgelehnt, unbekannte ID abgelehnt
- **Nicht geprüft:** Jev gegen das echte OpenRouter (kein Key hier, nur Stand-in), `harness merge --approve` mit Bestätigung im echten Terminal, ein Konflikt oder unsauberer Arbeitsbaum beim Sichern im echten Lauf (nur über `HostRepo::merge`-Tests).

Stand 23.10. (nachts, Schritt 2):
- **Server:** 155/155, neu: Chat mit `link` anlegen (nicht angebotenes Projekt 409 und fremdes Gerät 404, jeweils ohne gespeicherten Chat; erste Nachricht geht ans Gerät).
- **Browser-E2E:** 13/13, 0 Konsolenprobleme; „Neuer Chat in dieser Umgebung“ öffnet die Startseite mit „Nachricht an pi in demo …“, die erste Nachricht geht als neue Sitzung ans Gerät, Antwort im neuen Chat; Auswahl über den Dialog der Startseite und „Ohne Umgebung“. Screenshots 37 und 38 angesehen.
- **Nicht geprüft:** mit echtem Gerät (nur simuliert; der Weg dahinter ist derselbe wie beim Verknüpfen).

Stand 23.10. (nachts, Schritt 1 „Umgebung im Web-Chat“):
- **Rust:** `cargo test` grün (harness-core 106, harness-cli 7, neu: Belegt-Marker samt Aufräumen nach Absturz, Leerlauf-Zustand, Status je Protokoll, Tunnel-Nachrichten), `cargo clippy --all-targets` ohne Warnungen.
- **Server:** 154/154, neu: Aushandlung (Protokoll 1 bekommt nichts Neues, 0 und 3 werden abgelehnt), Aktivität mit `stopsAt`, `settings` beim Gruß und nach Änderung, `vm.stop`/`vm.result` samt Abbruch bei Trennung, Routen mit Eigentümer-Prüfung und Grenzen.
- **Browser-E2E:** 13/13, 0 Konsolenprobleme; Popover mit „VM läuft“, „Stoppt um …“, Menü der Leerlaufzeit (→ `settings` beim Gerät), Rückfrage beim Stopp mit Terminal in Benutzung, „VM gestoppt“; Geräte-Seite hell und bei 360 px. Screenshots 31–36 angesehen.
- **Echt:** `VM_STOP=1 bash scripts/smoke-connect.sh` 41/41 gegen die VM: Aktivität mit Protokoll 2, Leerlaufzeit kommt an, `harness net watch` hält die VM, Stopp aus dem Browser (~3 s), Stopp nach 1 min Leerlauf. VM danach wieder gestoppt (wie vorher), keine Reste.
- **Nicht geprüft:** echter Server auf dem Pi mit Protokoll 2 und Rechner mit Protokoll 1 gemischt (nur in Tests), Dichten und Tastaturbedienung des neuen Menüs (nur per Radix), Stopp im Leerlauf beim echten Dienst des Nutzers über längere Zeit.

Stand 23.10. (abends):
- **Server:** 151/151. **Browser-E2E:** 13/13, 0 Konsolenprobleme; neu: Quellen-Link in Klammern verschwindet aus dem Satz, Badge „127.0.0.1“ am Absatzende, Liste beim Zeigen mit „Kaffee-Testseite“, erfundener Link bleibt echter Link (`target=_blank`, Cursor `pointer`), Tooltip „bevorzugte Quelle“. Screenshot 27b angesehen.
- **Nicht geprüft:** echtes Modell mit dem neuen Quellen-Prompt (hält DeepSeek die Klammern am Absatzende ein?), Badge auf Touch-Geräten.

Stand 23.10.:
- **Server:** 151/151, neu: `sections.ranking` je nach Weg (Stichworte, Bedeutung, Nummern).
- **Browser-E2E:** 13/13, 0 Konsolenprobleme; neue Tabellenüberschriften, „7 von 8 Abschnitten · semantisch“, „ganze Seite“, „Ungeprüfte Links“ mit einem erfundenen Link der Mock-Antwort (die gelesene Seite steht nicht darin), Laufschrift auf `/design` (verschiebt sich beim Zeigen, Tooltip nur bei langen Namen). Screenshots 27, 28, 29, 30 angesehen.
- **Nicht geprüft:** Laufschrift in der echten Modellauswahl mit langen Namen (gleiche Komponente, nur auf `/design` gesehen), Verhalten auf Touch-Geräten (dort umbrechen die Namen in der Liste).
- **Pi:** Image aktualisiert, Server gesund, SearXNG liefert Treffer von Bing, Yahoo, Google CSE und Brave (DuckDuckGo per CAPTCHA gesperrt).

Stand 22.10.:
- **Server:** 151/151, dazu abgerechnete Kosten (Platzhalter-OpenRouter durch die echte AI SDK), gesperrte Suchmaschinen, kurze grep-Wörter.
- **Echt:** Benchmark mit 24 Recherche-Läufen (SearXNG in der VM, DeepSeek V4.1 Flash über OpenRouter), Jev gegen DeepSeek auf 12 gleichen Listen; danach Kosten von OpenRouter, knappe Jev-Anfrage und grep mit Produktionscode gegen die echten Dienste. VM danach aufgeräumt und gestoppt, Testserver beendet, Test-Datenbanken gelöscht.

Stand 21.10. (abends):
- **Server:** 147/147 (SQLite), dazu Wortformen (CISTEM, Porter, Wortteile, Füllwörter), grep, rekursives Halbieren, Anhänge, Schwellen-Einstellung, Hinweis bei schwachen Treffern.
- **Browser-E2E:** 13/13, 0 Konsolenprobleme; Recherche mit Fokus und grep („Treffer: 1“), Token-Tabelle ohne Schalter, Admin-Karte „Lange Chats eindampfen“ speichert. Screenshots 26b, 27, 29, 30 angesehen.
- **Echt mit dem OpenRouter-Key des Nutzers:** Jev auf 8 Suchen (Bing/DuckDuckGo-Treffer, nur im Testskript); Embeddings auf 9 Fragen und 7 Modellen; Produktionspfad `selectSections` mit Qwen3 (1,6–3,2 s, 0,04 Cent je Wikipedia-Artikel); `grep` auf Wikipedia Dresden, Node-Releases.

Stand 21.10., nach Schritt 5 der Recherche:
- **Server:** 132/132 (SQLite), neu: Prompt bleibt über Anfragen gleich, Gewichtung nach Art, Jev-Reihung mit Kosten und Rückfall, Abschnitte (BM25, Embeddings, Gliederung, Nummern), Eindampfen (Schwelle, Pause, gleicher Prompt-Anfang danach, Neuladen ohne Rückfrage). Postgres zuletzt am 14.10.
- **Browser-E2E 21.10.:** 13/13, 0 Konsolenprobleme; Recherche mit Jev- und Embedding-Stand-in („7 von 8 Abschnitten“, Kosten je Modell im Entwicklermodus), langer Chat mit Hinweis aufs Eindampfen auch nach Neuladen. Screenshots 27, 29, 30 angesehen.
- **Echter Lauf der Abschnittsauswahl** (nur BM25, ohne Modell) gegen echte Seiten: de.wikipedia.org/wiki/Kaffee (206 000 Zeichen, 168 Abschnitte, Frage nach Schlaf → §85 „Schlaf“), nodejs.org/api/fs.html (Verzeichnis überwachen → `fs.watch`, `fsPromises.watch`), MDN Caching (Frische ohne `max-age` → „Heuristic caching“). 7–33 ms. Dabei gefunden und behoben: Readability warf alle Wikipedia-Überschriften weg.
- **pi-Schutz:** `node --test crates/harness-core/pi/guard.test.ts` 6/6.

Stand 18.10. (Block „Recherche“):
- **pi-Schutz:** `node --test crates/harness-core/pi/guard.test.ts` 6/6; `cargo test -p harness-core --lib` 103 grün (nach Schritt 1).
- **Typecheck:** alle drei Pakete grün.
- **Browser-E2E:** 12/12, 0 Konsolenprobleme:
  - Fundstellen mit Tag-Zeichen
  - Hauptinhalt
  - Websuche einrichten und testen
  - Recherche ohne Rückfrage, Quellen, Schalter nach Neuladen
  - Screenshots angesehen (dunkel Desktop, hell mobil)
- **Echte Läufe:**
  - `scripts/smoke-connect.sh` 27/27: Die Warnung des pi-Schutzes kommt mit Fundstelle aus der VM in den Chat.
  - Readability gegen echte Seiten: Wikipedia, MDN, Node-Doku, GitHub, heise und SPIEGEL (Start und Artikel), tagesschau, example.com
  - `docker compose --profile search config` gültig
- **Nicht geprüft:** echter Suchlauf; SearXNG-Container; Autostart nach echter Windows-Anmeldung; Lauf mit DeepSeek über den Web-Chat; echter Caddy.

---

## Log

### 2026-10-30 (abends, App-Katalog Schritt 4)
- Gebaut: Gateway fertig gebaut (Job `gateway` in `windows.yml`, `harness-core/build.rs`, `Containerfile.prebuilt`), Setup-Update bei laufendem harness, Exit-Code-Badge.
- Geprüft: gepacktes Gateway-Image in der VM (gebaut in 1,7 s, `gateway_vm` grün), Setup-Test mit einer laufenden Kopie von ping, `install.ps1` ebenso, Windows-Workflow auf dem Branch.
- Der Nutzer ließ dabei einen Agent arbeiten; VM-Tests liefen von 16:33 bis 16:37 parallel (eigene Projekte, sein Gateway-Image nur umbenannt, nicht ersetzt).

### 2026-10-30 (App-Katalog Schritt 3b, Release v0.1.8)
- Gebaut: Programmdateien vom Rechner (Prüfung der Datei, Antrag, Freigabe im Tray oder Terminal, Kopie in der Konfiguration, Helfer für Programm, AppImage, `.deb`, Archive), Protokoll 7, Abschnitt pro Rechner im Browser.
- Gefunden: Namen aus Dateinamen waren unhandlich (`archello-tar`); jetzt ohne Endung und Version wie in der Web-App.

### 2026-10-29/30 (Release v0.1.6, App-Katalog Schritt 3a)
- Release v0.1.6: Der Nutzer hatte v0.1.5 installiert, die Programme im Browser kamen erst danach.
- Gebaut: eigene Programme aus dem Browser (Vorlagen pro Konto, Protokoll 6, Rezepte auf dem Rechner, Abschnitt unter Einstellungen → Geräte, Herkunft in den Schaltern).
- Gefunden: `harness_apt` meldete ein unbekanntes Paket als „Schon im Base-Image“ (behoben). `prettier --write` ohne Konfiguration hat 18 Dateien umformatiert; aus `HEAD` wiederhergestellt und die Änderungen neu eingespielt (Stolperstein in CLAUDE.md).

### 2026-10-28 (Rechner, Mathe, Kopieren)
- Gefunden: Copy-Buttons scheiterten über HTTP im LAN (keine Zwischenablage-API außerhalb sicherer Kontexte); Mathe renderte nur `$$`, Modelle schreiben `\(…\)`; `config(...)` aus einem Ausdruck verstellt eine math.js-Instanz dauerhaft.
- Gebaut: Rückfall fürs Kopieren, Umwandlung der LaTeX-Begrenzer, Werkzeug `calculate` mit Worker und frischer Instanz pro Aufruf, Karte im Chat, Admin-Schalter.

### 2026-10-28 (App-Katalog Schritt 2)
- Gebaut: Tunnel-Protokoll 5 (`apps`, `apps.set`, `apps.result`), Bericht in `device.apps`, Route zum Schalten, Programme unter Einstellungen → Geräte und im Umgebungs-Popover, Smoke- und E2E-Prüfungen.
- Gefunden und behoben: Der Leerlauf-Wächter arbeitete gestaute Wakes einzeln ab (je ~2,3 s Statusabfrage beim VM-Dienst), `daemon stop` brauchte dadurch ~30 s. Der Test-Server verdrahtete seinen Hub selbst; jetzt `createDeviceHub` für beide.
- Neue Wünsche des Nutzers: Copy-Buttons reparieren, Mathe rendern, Taschenrechner (math.js) als Werkzeug.

### 2026-10-27 (abends, App-Katalog Schritt 1)
- Vorversuch in der VM: Unterordner einzeln einhängen geht (podman 5.8.8), apt ohne root mit eigenen Listen und `dpkg-deb -x` geht, gcc braucht `--sysroot` (mit `CPATH` scheitert C++). rustup brach in der 1-GB-VM beim Entpacken ab.
- Gebaut: Kern (`apps.rs`, Rezepte, `lib.sh`), `[apps]` in Projekten, CLI `harness app`, Instanz mit App-Volumes und Umgebung, Hinweise an den Agent; `build-essential` raus aus dem Base-Image, C/C++ als Standard.
- Auf Wunsch des Nutzers: `vmd clean` behält node/uv, räumt alte App-Stände weg und sichert ungesicherte Arbeit gelöschter Projekte als Bundle (`rescue.rs`), `project remove` ebenso.
- Gefunden: `vmd clean` mit `HARNESS_HOME` hätte alle echten Projekte des Nutzers für gelöscht gehalten; jetzt gesperrt. Der VM-Start scheiterte heute zweimal beim SSH-Handshake (wenig freier RAM), der nächste Versuch ging jeweils.
- Der Rechner des Nutzers stürzte während eines Builds ab (Speicher); nichts verloren.

### 2026-10-24 (Schritt 4)
- Gebaut: Tray-Symbol in `harnessd` (Win32 über windows-sys), Statusdatei des Dienstes, VM-Stopp aus der Taskleiste über ein benanntes Ereignis, Benachrichtigungen bei Rückfragen und angehaltenen Merges.
- Gefunden: PowerShell übergibt `$null` für String-Parameter als `""`; `FindWindowEx` fand das Fenster deshalb nicht (Prüfskript nutzt jetzt `EnumWindows`).

### 2026-10-24 (Schritt 3)
- Gebaut: Fortschritt sichern (Commit im Container, holen, prüfen, mergen, Agent-Branch nachziehen), Freigabe-Anträge mit `harness merge --approve`, Autosync pro Chat, Jev für Diffs über den Server, Tunnel-Protokoll 3, Übernahme-Karte im Chat.
- Gefunden: Nach einem gescheiterten VM-Start hält gvproxy die Pipe noch Minuten; Folgestarts scheitern bis dahin (CLAUDE.md, VM-Dienst).

### 2026-10-23 (nachts, Schritt 2)
- Gebaut: neuer Chat direkt mit Umgebung (Startseite, `link` beim Anlegen, „Neuer Chat in dieser Umgebung“).
- Entschieden vom Nutzer: Übernehmen in den ausgecheckten Branch, schwere Funde nur per ID im Terminal, „Immer erlauben“ bleibt, Autosync als Schalter; neu gewünscht: Kompaktierung und Effort-Regler im Web, mit pi abgeglichen.

### 2026-10-23 (nachts)
- Gebaut: Tunnel-Protokoll 2 mit Aushandlung, VM-Aktivität und Leerlaufzeit, `vm.stop` aus dem Browser, Wächter im Dienst mit Belegt-Markern und Prüfung auf fremde Container, Bedienung im Umgebungs-Popover und unter Geräte, Radio-Einträge im Dropdown-Primitiv.
- Gefunden: Puppeteers `::-p-text` sah einen geänderten Text im Popover nicht; Heredocs mit Backticks scheitern im Bash-Werkzeug (CLAUDE.md).

### 2026-10-23 (abends)
- Gefunden: Streamdown rendert Links als Knöpfe mit Bestätigungsdialog, deshalb wirkten sie nicht wie Links.
- Gebaut: echte Links, Quellen als Badge am Absatzende (Hover-Liste), Prompt verlangt Quellen gesammelt am Absatzende, Tooltip zu „bevorzugte Quelle“.
- Entschieden: Secrets mit dem Passwort verschlüsseln, aber erst am Ende.

### 2026-10-23
- Gefunden: Der Pi lief nach `docker compose pull` weiter mit dem alten Container (Stand 20.10.), deshalb fehlten Jev und Abschnitte. Neu erstellt mit `up -d --no-deps server`, SearXNG mit `--force-recreate`.
- Gebaut: getrennte Kostentabellen, Anzeige „semantisch/Stichworte/ganze Seite“, Belege je Aussage mit „Ungeprüfte Links“, `MarqueeText` für lange Modellnamen (Modellauswahl, `/design`).
- Beantwortet: Jev funktioniert auch mit DeepSeek direkt, solange der Nutzer einen OpenRouter-Zugang hat. Keys sind schon verschlüsselt (AES-256-GCM mit Server-Secret), Passwort-Variante ist vorgemerkt.

### 2026-10-22
- Benchmark: Jev reiht (10 Treffer) gegen Modell wählt aus 20; gleich gute Antworten, Kosten im Rauschen. Jev bleibt, mit 25 % knapperer Anfrage.
- Gefunden: SearXNG ab Werk nur mit drei Suchmaschinen, die nach ~80 Suchen sperren; OpenRouter rechnet anders ab als gelistet (0,14 statt 0,024 $ pro Million Eingabe-Token).
- Behoben: Bing und Yahoo in der SearXNG-Konfiguration, `engines_blocked` statt leerer Treffer, abgerechnete OpenRouter-Kosten im Chat, grep ohne Kurzwörter und Backslashes.

### 2026-10-21 (spät)
- BM25 mit Stammformen (CISTEM/Porter), Wortteilen und ohne Füllwörter: 19/20 statt 15/20 auf echten Seiten.
- Gemessen: 1024 statt 4096 Dimensionen ändern die Auswahl praktisch nicht (Rangkorrelation 0,94–0,98).

### 2026-10-21 (abends, nach Rückmeldung des Nutzers)
- `grep` für `web_fetch` und Hinweis im System-Prompt (focus fürs Thema, grep für Zahlen, sonst ganze Seite).
- Abschnitte rekursiv halbiert (bis 2000 Zeichen), Anhänge wie Einzelnachweise ausgenommen.
- Embeddings 3–5× schneller: base64, 1024 Dimensionen, parallele Teilaufrufe.
- Jev: Hinweis „neu suchen“, wenn kein Treffer taugt.
- Schwellen fürs Eindampfen höher und in der Web-App einstellbar; Entwicklermodus entfernt (alles immer sichtbar); Technik-Liste um Hersteller ergänzt.

### 2026-10-21 (Recherche verbessern, Schritte 1–5)
- Cache-fester Prompt: Datum und Recherche-Schalter als gespeicherter Hinweis vor der Nutzernachricht, `web_search` immer definiert (lehnt bei ausgeschalteter Recherche ab).
- Domain-Listen nach Art der Frage (`kind` in `web_search`), 20 Treffer holen, 10 weitergeben.
- Jev über OpenRouter reiht die Treffer (Relevanz), Kosten je Modell im Entwicklermodus, Hilfsmodelle in den Sucheinstellungen.
- `web_fetch` mit `focus`/`sections`: Abschnitte per BM25 und Embeddings, Gliederung, Anzeige „x von y Abschnitten“.
- Eindampfen langer Chats mit gespeicherter Stelle, Hinweis im Chat, Neuladen gekürzter Seiten ohne Rückfrage.
- Überschriften von Wikipedia überleben Readability jetzt (`liftHeadings`); vorher fehlten sie im Hauptinhalt ganz.

### 2026-10-20 (nach dem Test der Websuche auf dem Pi)
- Fehlerhinweise bei unerreichbarem Suchanbieter (`ENOTFOUND` → Profil fehlt, `ECONNREFUSED` → Container abgestürzt).
- Fehlalarme behoben: `html_comment` über leere Svelte-Marken hinweg, Nullbreitenzeichen als Umbruchstellen (jetzt nur noch Fund, wenn sie eine Wendung tarnen oder eine Kette bilden). Geprüft an echten Hugging-Face-Seiten.
- Gelesene Treffer im Suchblock markiert; Rückfragen im Recherche-Modus mit Grund und ähnlichster gefundener URL; aufgeräumte Varianten gefundener URLs laden ohne Frage.
- Rohe unsichtbare Zeichen aus Quelltexten entfernt (kamen über Werkzeug-Eingaben herein), `source.test.ts` verhindert neue.
- Ideen des Nutzers zu Quellen-Gewichtung und Teilagent bewertet (ROADMAP).
- Recherche-Prompt: suchen, wenn Fakten unsicher oder aktuell sind, nicht für Smalltalk, Schreiben, Code, Mathe; erst Ausschnitte, Seiten nur bei Bedarf (1–3).
- Entwicklermodus (Einstellungen → Darstellung): Tokens und Kosten je Modellaufruf unter jeder Antwort; Tooltip des Kontext-Rings mit genauen Zahlen und Summen.
- Planung mit Preisen aus OpenRouter und models.dev, Jev-API und Embeddings recherchiert (ROADMAP).

### 2026-10-18 (Block „Recherche“)
- Fundstellen von Prompt-Injection mit Ausschnitt und Erklärung (`findInjections` in `guard.ts` und `injection.ts`, `injection-findings.tsx`, aufklappbare Agent-Hinweise).
- Hauptinhalt per Readability + linkedom (`pageToMarkdown`), Tabellen als Markdown, `fullPage`.
- Websuche: `apps/server/src/websearch/`, Admin-Einstellungen mit Testsuche, Recherche-Schalter, Trefferblock, Quellen, SearXNG im Compose.

### 2026-10-18 (Phase 4 nach dem ersten Test)
- Hintergrunddienst, `harness init`, gespeicherte Umgebungen, Fortschritt, Kontext und Kosten, Web-Fetch in normalen Chats.

### 2026-10-17 (Phase 4, TLS, Server-Image)
- Tunnel, Hub, Agent-Stream, pi im RPC-Modus; TLS im Gateway; Server-Image amd64/arm64; pi-Schutz gegen Prompt-Injection.

### 2026-10-12 bis 2026-10-17 (Phase 2, 2b, 2c)
- Workspace, Provider, Base-Image, Konfiguration, Git-Sync, Gateway, Kopplung, LLM-Durchgang, `run`/`shell`/`sync`/`merge`, Terminal-Relay, VM-Dienst, Merge-Prüfung Stufe 1.

### 2026-10-10 bis 2026-10-11
- Architektur, Server und Web-App (Phase 1), WSL-Spike, E2E-Test.
