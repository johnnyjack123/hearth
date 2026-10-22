# Progress

_Last updated: 2026-10-22 · Benchmark Jev gegen Modell mit echtem SearXNG und DeepSeek (Jev bleibt); abgerechnete OpenRouter-Kosten, gesperrte Suchmaschinen erkannt, Bing/Yahoo in SearXNG, knappere Jev-Anfrage, grep ohne Kurzwörter; nächstes: Test des Nutzers_

## Where we are

Repo auf GitHub (`origin` = `https://github.com/johnnyjack123/llm-harness.git`). Gearbeitet wird auf Feature-Branches (zuletzt `feat/research`), die nach jedem geprüften Stück per `--no-ff` in `main` gemergt und gepusht werden (Freigabe des Nutzers). `feat/phase-4-chat-env` ist seit dem 18.10. nicht mehr der Arbeitsbranch.

**Terminal:** Im Projektordner startet `harness` (bzw. `harness run pi`) Gateway, Instanz und pi in der VM. Rückfragen des Gateways kommen im selben Terminal. `harness sync`/`merge` holen die Commits des Agents; `merge` prüft sie vorher (Regeln, OSV).

**Web-Chat:**
1. `harness login <Server>` koppelt den Rechner und richtet den Hintergrunddienst ein; `harness init` meldet ein Projekt.
2. Im Chat „Umgebung“ → Gerät → Projekt: pi arbeitet in der VM. Schritte, Werkzeugaufrufe, Rückfragen, Kontext und Kosten erscheinen im Chat. Warnungen des pi-Schutzes zeigen aufklappbar Fundstelle und Grund.
3. **Ohne Umgebung** lädt das Modell Seiten über `web_fetch` im Server, als Hauptinhalt (Readability). Fremde Hosts brauchen eine Freigabe. Bei Verdacht auf Prompt-Injection zeigt die Warnung aufklappbar jede Fundstelle mit Ausschnitt und einem Satz, warum das gefährlich ist.
4. **Recherche-Schalter** im Composer (nur wenn ein Admin unter Administration → Server → Websuche einen Anbieter gespeichert hat): Das Modell sucht mit `web_search` und liest Treffer und Links geladener Seiten ohne Rückfrage. Unter der Antwort stehen die gelesenen Seiten als Quellen.
   - Treffer werden nach Art der Frage gewichtet (Domain-Listen) und mit OpenRouter-Key von Jev gereiht (Relevanz in Prozent).
   - `web_fetch` kann mit `focus` nur passende Abschnitte holen (BM25, mit OpenRouter-Key plus Embeddings), für genaue Zahlen holt das Modell die ganze Seite.
   - System-Prompt und Werkzeuge bleiben für den Cache fest, Datum und Schalter kommen als Hinweis vor der Nachricht.
   - `grep` holt für genaue Zahlen nur die Zeilen mit bestimmten Wörtern (mit Überschriften und Tabellenköpfen).
   - Lange Chats (ab 200 000 Token, 50 % des Kontextfensters oder nach 6 h Pause; in der Web-App einstellbar) bekommen ältere Seiten und Suchen nur noch gekürzt; der Chat sagt das an.
   - Tokens und Kosten je Modellaufruf und je Modell (Hauptmodell, Jev, Embeddings, Titel) stehen immer hinter dem Beleg-Knopf, Summen im Tooltip des Kontext-Rings. Den Entwicklermodus gibt es nicht mehr.

**Server des Nutzers:** Docker-Compose mit Postgres auf seinem Raspberry Pi, `http://192.168.1.20:8085`. Das Image baut GitHub bei jedem Push auf `main`. Der Windows-Rechner war am 18.10. mit diesem Pi-Server gekoppelt (`harness doctor`).

Schnelltest vor Beginn: `cargo build && target/debug/harness doctor`, dann `bash scripts/smoke-connect.sh` (~1 min). Browser-E2E nur bei freiem Speicher (siehe Known issues).

## Next step

1. **Test des Nutzers auf dem Pi** nach dem Update des Images und `docker compose --profile search up -d` (neue SearXNG-Konfiguration mit Bing und Yahoo): OpenRouter-Zugang anlegen (Einstellungen → Anbieter), dann Recherche. Prüfen: Relevanz-Angaben im Suchblock, „x von y Abschnitten“ und „Treffer: n“ bei langen Seiten, unter der Antwort die Zeilen „Treffer reihen“ (Jev) und „Abschnitte wählen“ (Embeddings) mit Kosten. Jev und Embeddings sind am 21.10. mit seinem Key echt geprüft, aber nur in Skripten, nicht im Chat mit echtem Modell.
2. Vorschlag, offen: Fällt SearXNG wegen gesperrter Suchmaschinen aus, automatisch auf einen Anbieter mit Key ausweichen (Tavily 1 000 Anfragen im Monat frei). Bisher gibt es nur einen Anbieter zur Zeit. Thesaurus (OpenThesaurus, WordNet, ConceptNet) vorerst nicht: Embeddings decken Synonyme besser ab (Entscheidung mit dem Nutzer, 21.10.). Stammformen und Wortteile sind eingebaut.
3. Danach (Reihenfolge laut Nutzer):
   - **Zweitmeinung zu Warnungen** (ROADMAP, eigener Abschnitt): Jev oder ein Chat-Modell bewertet Warnungen mit frischem Kontext. Die Fundstellen (`InjectionFinding`) sind jetzt die natürliche Eingabe dafür.
   - **Stopp der VM im Leerlauf**
4. Phase-4-Reste: Übernehmen per Knopf (sync/merge samt Merge-Prüfung im Web), neuer Chat direkt mit Umgebung, fortsetzbare Streams, Suche für Agents in der VM (über den Server).
5. Später (ROADMAP Phase 5): Dokumente per markitdown, Bilder für Vision-Modelle, Windows-Builds auf GitHub.
6. Vorgemerkt: DeepSeeks Websuche (`api.deepseek.com/v1/tools/search`) als weiteren Suchanbieter prüfen (ROADMAP, Phase 4 „Recherche“ → Offen).

## In flight / half-finished

Nichts. Alles ist committet, nach `main` gemergt und gepusht; keine Hilfsagenten oder Worktrees offen.

## Decisions made this session (20./21.10.)

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

- Rückmeldung zur Recherche mit OpenRouter-Key (Reihung, Abschnitte, Kosten) und weiterhin zum Ablauf mit Hintergrunddienst und DeepSeek.
- Der OpenRouter-Key aus dem Chat vom 21.10. sollte nach den Tests widerrufen werden; er steht im Gesprächsverlauf.

## Known issues / debt

- **Mittel:** Brave und Tavily nur gegen Stand-ins geprüft (Formate aus deren Doku); SearXNG läuft beim Nutzer.
- **Mittel:** Jev (`/api/alpha/decisions`, Alpha-API) und Embeddings über OpenRouter nur gegen Stand-ins geprüft; beide Endpunkte antworten ohne Key mit 401 (also vorhanden). Schlägt ein Hilfsmodell fehl, läuft der Chat ohne weiter.
- **Niedrig:** Die Kosten des Titels zählen erst ab jetzt zur ersten Antwort; Embedding-Preise ohne `usage.cost` kommen aus OpenRouters Modellliste (24 h im Speicher).
- **Mittel:** Readability braucht für 1–2-MB-Seiten hier ~1 s, auf dem Pi vermutlich mehrere Sekunden.
- **Mittel:** Neuladen oder Schließen des Tabs während einer Agent-Antwort bricht sie ab (fortsetzbare Streams fehlen).
- **Mittel:** „Neu erzeugen“ in einem verknüpften Chat schickt die Nachricht erneut; pis Sitzung enthält die alte Runde noch.
- **Mittel:** pis Sitzungen aller Projekte liegen gemeinsam in `harness-cfg-pi`.
- **Mittel:** VM-RAM knapp (Start 1 GB; am 18.10. 183 MiB frei); ~350–500 ms pro podman-Aufruf.
- **Mittel:** Kein Verbrauch oder Limit pro Gerät im LLM-Durchgang.
- **Mittel:** Markdown-Bilder könnten Daten hinausschicken, nur die CSP der Produktion blockt das (Vite-Dev-Server hat keine).
- **Niedrig:** Ob pis Oberfläche mehrzeilige Warnungen sauber umbricht, ist nur am Code geprüft (`showWarning` → `ThemedText`), nicht im Terminal gesehen.
- **Rechner:** Wenig freier Speicher. Der Browser-E2E scheiterte am 18.10. zweimal mit `ERR_INSUFFICIENT_RESOURCES` und lief beim zweiten Versuch grün. Windows Sandbox nie ungefragt starten.

## Verification state

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
