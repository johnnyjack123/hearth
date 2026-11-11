# Wie Hearth funktioniert

Dieses Dokument erklärt die wichtigen Teile von Hearth: was sie tun, wie sie zusammenspielen und warum sie so gebaut sind. Bedienung und Einrichtung stehen in [`deploy.md`](deploy.md) und [`client.md`](client.md), das ursprüngliche Entwurfsdokument in [`../ARCHITECTURE.md`](../ARCHITECTURE.md).

## 1. Überblick

Hearth hat zwei Hälften.

**Der Server** ist ein Node.js-Prozess mit API und Web-App. Er hält Konten, Chats, Dateien und die API-Keys der Nutzer und spricht mit den Modell-Anbietern. Er läuft typischerweise auf einem kleinen Rechner im Heimnetz oder einem VPS.

**Der Windows-Client** läuft auf dem Rechner eines Nutzers und stellt dort Sandboxes für Coding-Agents bereit:

```
Windows-Rechner
├─ hearth.exe        Befehlszeile; als „daemon“ auch der Hintergrunddienst
├─ hearthd.exe       Tray-Symbol, startet und überwacht den Hintergrunddienst
├─ hearth-vmd.exe    Windows-Dienst (SYSTEM), startet und stoppt die VM
└─ Hyper-V-VM „hearth“ (Fedora CoreOS, Podman rootless)
   ├─ Gateway je Projekt     einziger Weg ins Netz
   └─ Instanz je Projekt     Container mit dem Agent (pi) und einer Kopie des Repositorys
```

Server und Client verbindet ein WebSocket-Tunnel, den der Rechner aufbaut. So erreicht der Server den Rechner auch hinter einem Router, ohne dass am Rechner ein Port offen sein muss.

Die Grundidee: **Der Agent darf in seiner Sandbox alles, aber nichts außerhalb.** Er hat keinen Zugriff auf Dateien des Rechners, keine API-Keys und keinen Netzzugang außer über das Gateway. Was er erarbeitet, kommt als Git-Commit zurück und wird geprüft, bevor es im Projektordner landet.

## 2. Der Server

### Aufbau

- **Hono** als HTTP-Framework, **Better Auth** für Konten, Sitzungen, Admin-Rollen und Zwei-Faktor-Anmeldung.
- **Kysely** für die Datenbank, wahlweise SQLite (eine Datei, keine weitere Software) oder Postgres. Migrationen laufen beim Start und sind so geschrieben, dass sie auf beiden laufen.
- **AI SDK** für die Gespräche mit den Modellen. Jeder Anbieter (Anthropic, OpenAI, Google, Mistral, OpenRouter, Ollama, OpenAI-kompatibel) wird über denselben Weg angesprochen.
- Die Web-App ist React mit Vite und wird vom Server als statische Dateien ausgeliefert.

### API-Keys

Jeder Nutzer hinterlegt eigene Keys. Sie liegen mit AES-256-GCM verschlüsselt in der Datenbank. Der Schlüssel wird per HKDF aus dem Server-Geheimnis abgeleitet, und jeder Key ist an Nutzer und Zugang gebunden (als „associated data“). Ein Key lässt sich deshalb weder ohne das Geheimnis entschlüsseln noch einem anderen Konto unterschieben. Admins sehen nur die letzten Zeichen.

### Ein Chat-Aufruf

1. Die Web-App schickt die neue Nachricht mit Modell, Zugang und Optionen (Recherche, Denktiefe, Antwortlänge, Tonfall).
2. Der Server baut den Verlauf, entfernt, was das Modell nicht braucht, und setzt Dateien als Text oder Bild ein.
3. Er ruft das Modell mit festem System-Prompt und festen Werkzeugen auf (Webseiten laden, Websuche, Rechner, Gedächtnis, Fortschrittsliste, Dokumente lesen; je nach Einstellungen des Nutzers).
4. Die Antwort fließt als Stream zum Browser und wird parallel gespeichert.

**Warum der System-Prompt fest bleibt:** Anbieter rechnen wiederholte Anfangsstücke eines Prompts aus ihrem Cache ab, oft zu einem Bruchteil des Preises. Alles, was sich ändert (Datum, Recherche-Schalter, Antwortlänge, Tonfall, neue Erinnerungen), steht deshalb nicht im System-Prompt, sondern als kurzer Hinweis vor der jeweiligen Nutzernachricht, und auch nur, wenn sich etwas geändert hat. Der Hinweis wird mit der Nachricht gespeichert, damit spätere Aufrufe Zeichen für Zeichen denselben Anfang schicken.

**Antworten laufen ohne Browser weiter.** Der Server liest den Stream selbst bis zum Ende und speichert die Antwort. Lädt jemand die Seite neu oder schließt sie, holt die Web-App die laufende Antwort von Anfang an wieder ab. Beenden lässt sie sich nur mit „Stopp“.

### Lange Chats

Jeder Aufruf schickt den ganzen bisherigen Verlauf. Damit das bezahlbar bleibt:

- **Eindampfen:** Ab einer Schwelle (Standard 200 000 Token oder die Hälfte des Kontextfensters, oder nach 6 Stunden Pause) bekommt das Modell ältere Webseiten und Suchergebnisse nur noch als Titel und Adresse. Die Grenze wird einmal festgelegt und gespeichert, damit der Anfang des Prompts danach wieder stabil bleibt.
- **Zusammenfassen:** Wird das Kontextfenster knapp, schreibt das Modell eine Zusammenfassung des älteren Teils, und künftige Aufrufe lesen statt der alten Runden diese Zusammenfassung. Im Chat bleibt alles sichtbar.

### Kosten

Jeder Modellaufruf wird mit Token-Zahlen und Kosten gespeichert. Die Preise kommen aus dem Katalog von models.dev; bei OpenRouter zählt, was OpenRouter tatsächlich abrechnet. Ein Kostendeckel pro Tag (vom Admin für alle, vom Nutzer für sich) stoppt neue Anfragen, auch die der Agents.

## 3. Webseiten und Recherche

Das Modell kann Seiten laden (`web_fetch`) und im Recherche-Modus suchen (`web_search`). Beides läuft auf dem Server, nicht beim Anbieter.

**Nur öffentliche Adressen.** Der Server löst den Namen selbst auf, lehnt private und besondere Bereiche ab (Heimnetz, localhost, Link-local, die Sonderformen von IPv6) und verbindet sich genau mit der geprüften Adresse. Ein DNS-Eintrag, der zwischen Prüfung und Verbindung wechselt, hilft also nicht. Jede Weiterleitung wird erneut geprüft.

**Freigaben.** Hosts, die der Nutzer selbst genannt hat, lädt der Server ohne Frage. Alles andere fragt im Chat nach. Der Grund: Eine Adresse kann Daten aus dem Chat enthalten. Ein Modell, das durch eine präparierte Seite umgelenkt wurde, könnte sonst Inhalte des Gesprächs in einer URL an einen fremden Server schicken. Im Recherche-Modus laden Seiten aus Suchtreffern ohne Frage, aber nur genau so, wie die Suche sie gefunden hat; eine selbst zusammengesetzte Adresse fragt weiter nach.

**Nur das Wesentliche.** Von einer Seite bleibt der Hauptinhalt (Mozilla Readability), umgewandelt in Markdown. Lange Seiten werden in Abschnitte geteilt; das Modell bekommt die Gliederung und die Abschnitte, die zu seiner Frage passen. Die Auswahl verbindet eine Stichwortsuche (BM25 mit Wortstämmen für Deutsch und Englisch) mit semantischer Ähnlichkeit über Embeddings. Für genaue Zahlen kann das Modell gezielt nach Wörtern suchen lassen.

**Prompt-Injection.** Seiten, Suchtreffer, Dokumente und Werkzeugausgaben werden nach typischen Mustern untersucht: Anweisungen an das Modell, versteckter Text, unsichtbare Zeichen, die Wörter tarnen. Ein Fund wird im Chat mit genauer Stelle gezeigt, und das Modell bekommt einen Warnhinweis. Ein kleines Bewertungsmodell (Jev) kann einschätzen, wie wahrscheinlich ein echter Angriff ist. Die Warnung wird nie entfernt, nur ergänzt. Erkennen ist grundsätzlich unvollständig; der eigentliche Schutz sind die Freigaben und bei Agents die Sandbox.

## 4. Die Sandbox auf dem Rechner

### Warum eine VM

Container allein teilen sich den Kernel des Hosts, und unter Windows laufen Linux-Container ohnehin in einer VM. Hearth nutzt dafür eine eigene Hyper-V-VM mit Podman:

- **Keine Dateifreigaben.** Die VM sieht keinen Ordner des Rechners. Daten wandern nur ausdrücklich hinein und heraus (`podman cp`, Git-Bundles). WSL hätte dagegen den ganzen Rechner mit Schreibrechten eingehängt.
- **Rootless Podman.** Auch in der VM laufen Container ohne Root-Rechte.
- **Eigene Maschine.** Hearth verwaltet seine VM getrennt von anderen Podman- oder Docker-Installationen.

### Der VM-Dienst

Hyper-V verlangt für jeden Start und Stopp Administratorrechte. Damit `hearth` trotzdem ohne Rückfrage arbeiten kann, gibt es einen kleinen Windows-Dienst (`hearth-vmd`), der als SYSTEM läuft und genau das kann: die VM starten, stoppen und ihre Festplatte verkleinern. Er nimmt Aufträge über eine benannte Pipe an, die nur der installierende Nutzer und Administratoren öffnen dürfen. Weil er als SYSTEM läuft, liest und startet er nichts, was ein normaler Prozess verändern könnte: podman bekommt eine von Grund auf leere Umgebung, sein Datenordner gehört Administratoren, und der Dienst prüft beim Start Besitzer und Verknüpfungen dieses Ordners.

### Instanzen

Für jedes Projekt gibt es einen Container („Instanz“) mit festen Sicherheitsregeln, die im Code verdrahtet sind und nicht per Konfiguration abgeschaltet werden können: schreibgeschütztes Root-Dateisystem, keine zusätzlichen Rechte, ein internes Netz ohne Weg nach draußen außer über das Gateway, keine Einbindung von Host-Ordnern. Daten liegen in Volumes:

- Arbeitskopie des Repositorys (`/workspace`)
- Home-Verzeichnis des Agents
- Konfiguration des Agents, je Projekt getrennt
- ein schreibgeschütztes Werkzeug-Volume mit dem Agent und den Erweiterungen von Hearth

**Programme aus dem Katalog** (Compiler, Laufzeitumgebungen) werden je Projekt in eigene Volumes installiert und schreibgeschützt eingehängt. Eine Installation gilt erst als fertig, wenn ein Quittungs-Volume mit Version und Rezept-Prüfsumme existiert; abgebrochene Installationen werden so nie benutzt. Das Basis-Image bleibt dadurch klein, und ein Projekt bekommt nur, was es braucht.

### Speicher

Auf ARM64 kann Hyper-V einer laufenden Linux-VM keinen Speicher nachreichen. Der VM-Dienst gibt der VM deshalb beim Start, was Windows gerade frei hat, abzüglich einer Reserve, und richtet in der VM komprimierten Swap ein. Jede Instanz hat eine Speichergrenze unter dem Speicher der VM, damit ein ausufernder Build nur seine Instanz beendet und nicht die ganze VM.

## 5. Das Gateway

Jedes Projekt hat ein eigenes Gateway, einen kleinen Proxy in Rust in einem eigenen Container. Er ist der einzige Weg der Instanz ins Netz.

- **Adressfilter.** Das Gateway verbindet sich nur mit öffentlichen Adressen, für jeden Modus und auch für IP-Literale. Das Heimnetz, der Rechner selbst und die VM-internen Adressen sind nie erreichbar.
- **Regeln je Projekt.** Voreinstellungen (z. B. nur Paket-Registries) plus eigene Erlaubnis- und Sperrlisten. Was nicht passt, wird zur Rückfrage.
- **Rückfragen entscheidet nie etwas im Container.** Sie werden im Terminal (`hearth run`, `hearth net watch`), über das Tray-Symbol oder im Web-Chat beantwortet, also von Programmen außerhalb der Sandbox. Sonst könnte sich ein gekaperter Agent selbst freigeben.
- **Modelle über `llm.hearth`.** Der Agent spricht seine Modelle unter der Adresse `http://llm.hearth/<zugang>/…` an. Das Gateway reicht diese Anfragen mit dem Geräte-Token an den Server weiter, und erst der Server setzt den API-Key ein. In der Sandbox liegt damit kein Key, den ein Angreifer stehlen könnte; das Geräte-Token selbst kennt nur das Gateway, nicht die Instanz.
- **Werkzeuge über den Server.** Webseiten laden, Websuche, Gedächtnis und Dokumente laufen für Agents ebenfalls über `llm.hearth`. Beim Laden einer Seite prüft das Gateway vorher den Host nach den Regeln des Projekts, und der Server lädt nur genau diesen Host.
- **TLS.** Ist der Server per `https://` erreichbar, verschlüsselt das Gateway selbst und prüft Zertifikat und Namen; eine eigene CA lässt sich hinterlegen.

## 6. Git-Sync und Prüfung

Der Agent arbeitet nie im Projektordner, sondern auf einem Klon in der VM. Der Austausch läuft über Git-Bundles:

1. Beim Start einer Sitzung schickt `hearth` den ausgecheckten Branch als Bundle in die Instanz, dazu `agent/<projekt>` und die Tags im Verlauf. Andere Branches und Tags bleiben auf dem Rechner: Wer den Agent steuert, auch über einen Web-Chat, soll nicht lesen können, was der Nutzer getrennt hält.
2. Der Agent committet auf den Branch `agent/<projekt>`.
3. `hearth sync` holt diese Commits als Bundle zurück, `hearth merge` prüft sie und übernimmt sie in den ausgecheckten Branch.

**Git auf dem Rechner ist gegen den Inhalt abgesichert.** Was aus der Sandbox kommt, gilt als nicht vertrauenswürdig. Hearth ruft Git deshalb mit abgeschalteten Hooks, ohne fsmonitor, ohne Submodule, ohne Filter-Treiber (auch nicht Git LFS) und mit Prüfung aller Objekte auf. Ein Hook-Ordner im Repository würde sonst nach dem Merge Code des Agents auf dem Rechner ausführen.

**Die Prüfung vor dem Merge** hat drei Stufen:
- **Feste Regeln**, die nicht mit dem Agent reden und sich nicht überreden lassen: Git-Hooks, Install-Skripte in `package.json`, Änderungen an CI-Konfigurationen, Einstellungen von Editoren und KI-Werkzeugen (`.claude/settings`, MCP-Konfigurationen), `.gitattributes` mit Treibern, Explorer-Dateien wie `desktop.ini`, Paketnamen, die beliebten Paketen zum Verwechseln ähnlich sind, und Binärdateien mit Textendung. Damit eine `.gitattributes` die Prüfung nicht blind macht, liest der Diff die Attribute aus einem leeren Baum.
- **OSV:** neue Abhängigkeiten werden gegen die Open Source Vulnerability Database geprüft, bekannte Schadpakete sind ein schwerer Fund.
- **Jev** (optional): ein Bewertungsmodell liest die hinzugefügten Zeilen und schätzt die Gefahr ein. Es kann nur Funde hinzufügen, nie welche entfernen.

`hearth merge` im Terminal hält bei schweren Funden an. Übernahmen, die ein Web-Chat anstößt („Fortschritt sichern“, Autosync), halten schon bei mittleren Funden an, denn Build-Skripte, CI-Dateien oder Editor-Einstellungen laufen ebenso auf dem Rechner. Freigeben geht nur im Terminal mit `hearth merge --approve <ID>`; der Browser allein kann nie freigeben, und die Freigabe gilt nur für genau den geprüften Commit.

**Übernehmen aus dem Browser braucht eine Erlaubnis am Rechner** (`[remote] merge`, `hearth project remote on --merge`). Ohne sie legt auch eine Übernahme ohne Funde nur einen Antrag an, und Autosync bleibt aus. So kann ein kompromittierter Server nichts in den Projektordner bringen, ohne dass jemand am Rechner zustimmt.

**Autosync** übernimmt bei erlaubten Projekten nach jeder Antwort des Agents selbst, mit derselben Prüfung. Ist der Projektordner nicht sauber (eigene, nicht committete Änderungen), lässt Hearth alles, wie es ist, und sagt es.

## 7. Web-Chat und Rechner

### Kopplung

`hearth login` folgt dem Ablauf, den auch Fernseher und Konsolen für Anmeldungen nutzen (OAuth Device Flow): Der Rechner holt sich einen Code, der Nutzer bestätigt ihn in der Web-App, der Rechner erhält ein Geräte-Token. Das Token liegt in der Windows-Anmeldeinformationsverwaltung. Auf dem Server steht nur seine Prüfsumme. Es gilt ausschließlich für die Geräte-Schnittstellen (Tunnel, Modell-Durchgang), nie für Chats oder Keys; ein gestohlenes Token gibt also Zugang zu Modellen, aber nicht zum Konto.

Zieht der Server um, beweist er beim nächsten `hearth login`, dass er das Gerät kennt, ohne dass das Token übertragen wird (eine HMAC über eine Zufallszahl mit der Prüfsumme des Tokens als Schlüssel). Nur dann wandert die Kopplung zur neuen Adresse.

### Tunnel

Der Hintergrunddienst hält einen WebSocket zum Server. Darüber laufen in beide Richtungen kleine JSON-Nachrichten: Projekte und Zustand des Rechners, Nachrichten an den Agent, seine Ereignisse, Rückfragen und Antworten. Beide Seiten nennen ihre Protokollversion und nutzen die kleinere; neue Nachrichten schickt jede Seite nur, wenn die andere sie kennt. So lassen sich Server und Rechner unabhängig voneinander aktualisieren.

Der Rechner entscheidet selbst, was er tut. Er nimmt vom Server nur harmlose Befehle an (Nachricht an einen Agent in einem angebotenen Projekt, VM stoppen, Programme schalten, Arbeit sichern) und prüft jeden gegen seine eigene Konfiguration. Umgebungen entstehen nur durch `hearth init` am Rechner, nie aus dem Browser.

### Eine Nachricht an den Agent

1. Der Server schickt die Nachricht über den Tunnel.
2. Der Hintergrunddienst startet bei Bedarf VM, Gateway und Instanz, aktualisiert die Arbeitskopie und startet pi im RPC-Modus. Jeder Schritt erscheint im Chat.
3. Die Ereignisse von pi (Text, Werkzeugaufrufe, Kosten, Rückfragen des Gateways) gehen als neutrale Agent-Ereignisse zurück und werden im Server zu denselben Nachrichtenteilen wie bei einem normalen Chat. Speichern, Anzeigen und Wiederaufnehmen funktionieren dadurch gleich.
4. Am Ende meldet der Rechner, ob Arbeit des Agents noch nicht im Projektordner ist, und übernimmt sie mit Autosync, wenn das Projekt es erlaubt.

Dateien aus dem Chat landen in der Instanz unter `~/uploads`, nie im Repository, damit sie nicht versehentlich in einem Commit landen.

### Leerlauf

Der Hintergrunddienst zählt, ob ein Agent antwortet oder ein `hearth`-Befehl im Terminal die VM nutzt (über Markierungsdateien, die ein laufender Prozess offen hält). Ist beides eine Weile nicht der Fall, stoppt er die VM. Container, die jemand von Hand gestartet hat, halten sie an.

## 8. OpenRouter

OpenRouter verteilt ein Modell auf viele Anbieter mit unterschiedlichen Preisen und eigenem Cache. Hearth wählt deshalb selbst:

- **Günstigster Anbieter:** Hearth lädt die Endpunktliste eines Modells und vergleicht einen typischen Aufruf (viel Eingabe aus dem Cache, wenig neu, wenig Ausgabe). Rabatte und Tageszeit-Preise zählen mit.
- **Beim Anbieter bleiben:** Ein Chat bleibt eine Weile bei dem Anbieter, der ihn zuletzt bedient hat, weil dort sein Cache liegt. Jede Anfrage trägt dazu einen Schlüssel je Unterhaltung (`prompt_cache_key`).
- **Fester Anbieter:** Je Modell lässt sich ein Anbieter festlegen, mit Firmensitz und Rechenzentren aus der Anbieterliste von OpenRouter. Dann gibt es kein Ausweichen, auch nicht für Agents.
- **Was das Konto erlaubt:** Eine normale API kann nicht abfragen, welche Anbieter ein Konto zulässt. Hearth fragt deshalb für die Modelle der letzten Chats einmal pro Woche je Anbieter mit einer Ein-Token-Anfrage nach und blendet ausgeschlossene aus. Das lässt sich abschalten.

## 9. Gedächtnis

Nutzer können Vorlieben und Fakten speichern, die in jedem Chat gelten. Modelle dürfen nur Vorschläge machen; ein Eintrag gilt erst, wenn der Nutzer ihn übernimmt. Der Grund: Jede Erinnerung landet in jedem künftigen Chat. Eine präparierte Webseite könnte sonst dauerhaft Anweisungen verankern. Vor dem Speichern laufen dieselben Muster wie gegen Prompt-Injection.

## 10. Betrieb im Internet

- **Client-Adresse:** Hinter einem Reverse-Proxy zählt `X-Forwarded-For` nur, wenn die Verbindung von einer Adresse aus `TRUSTED_PROXIES` kommt. Wer den Server direkt erreicht, kann sich keine fremde Adresse geben.
- **Bremsen:** Anmelden und Registrieren sind je Adresse begrenzt, ebenso das Starten und Abfragen von Kopplungen.
- **Erstes Konto:** braucht einen Einrichtungscode aus dem Protokoll, damit niemand einen frisch gestarteten Server übernimmt.
- **Registrierung:** ab Werk geschlossen; bei geschlossener Registrierung verrät der Server nicht, ob eine Adresse schon ein Konto hat.
- **Zwei-Faktor-Anmeldung** mit Authenticator-App und Backup-Codes.

## 11. Grenzen

- Der Client läuft nur unter Windows mit Hyper-V. Weitere Sandbox-Anbieter sind über eine Provider-Schnittstelle vorgesehen, aber noch nicht gebaut.
- Die Windows-Programme sind nicht signiert.
- Erkennung von Prompt-Injection und die KI-Bewertung sind Hilfen, kein Schutz. Was zählt, sind Sandbox, Gateway und die Prüfung vor dem Merge.
- Ein kompromittierter Server kann einem gekoppelten Rechner Nachrichten an den Agent schicken, in freigegebenen Projekten also den ausgecheckten Branch lesen lassen und dort, wo `[remote] merge` an ist, Übernahmen ohne Funde auslösen. Die Ergebnisse der Prüfungen aus [`security/`](security/) und ihr Stand sind dort dokumentiert.
