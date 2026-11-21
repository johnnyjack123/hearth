# Hearth

[English](README.md)

Hearth ist ein selbst gehosteter Chat für große Sprachmodelle. Mehrere Personen teilen sich einen Server, jede mit eigenen API-Keys. Ein Chat kann seine Arbeit außerdem an einen Coding-Agent übergeben, der in einer isolierten Sandbox auf dem eigenen Windows-Rechner läuft. Der Agent kann dort Code bauen, testen und committen, ohne an den Rest des Rechners zu kommen.

## Funktionen

**Chat (Server und Web-App)**
- Eigene Keys: Anthropic, OpenAI, Google Gemini, Mistral, OpenRouter, Ollama und jeder OpenAI-kompatible Endpunkt wie DeepSeek. Die Keys liegen verschlüsselt auf dem Server und dienen nur den Anfragen ihres Besitzers.
- Recherche-Modus mit Websuche (SearXNG, Brave oder Tavily). Von langen Seiten liest das Modell nur die passenden Abschnitte und nennt seine Quellen je Absatz.
- Seiten von Hosts, die der Nutzer nicht selbst genannt hat, lädt der Server erst nach Freigabe, außer der Nutzer schaltet freien Webzugriff ein. Text, der nach Prompt-Injection aussieht, wird mit genauer Fundstelle markiert; ein Hilfsmodell kann die Warnung bewerten.
- Bilder, PDF, Word, Excel und PowerPoint im Chat, mit Texterkennung für gescannte PDFs. Lange Dokumente werden durchsucht statt komplett mitgeschickt.
- Optionen je Chat: Denktiefe, Antwortlänge und Tonfall. Lange Chats werden automatisch eingedampft und zusammengefasst, damit der Kontext bezahlbar bleibt.
- Ein Gedächtnis unter Kontrolle des Nutzers, Tags als Ordner und Kosten je Antwort und je Modellaufruf.
- OpenRouter-Routing zum günstigsten Anbieter oder zu einem fest gewählten Anbieter je Modell, mit dessen Firmensitz.
- Konten mit Admin-Rolle, ab Werk geschlossene Registrierung, Zwei-Faktor-Anmeldung und ein Kostendeckel pro Tag.
- Als App auf Handy und Desktop installierbar (PWA), wenn der Server per HTTPS erreichbar ist. Die Seite hält Abstand zu Notch und Gestenleiste, und die Bildschirmtastatur verdeckt das Eingabefeld nicht. Auf dem Handy bietet die Büroklammer Fotos, Kamera oder Dateien an.
- Ein Chat, der auf mehreren Geräten offen ist, bleibt gleich: Was auf dem Handy geschrieben wird, erscheint auf dem Laptop, samt Antwort, während sie entsteht. Nach einem Update des Servers bieten offene Seiten an, neu zu laden.
- Suche in Titeln, Tags und im Text der Nachrichten.

**Agents auf dem eigenen Rechner (Windows-Client)**
- Der Coding-Agent [pi](https://www.npmjs.com/package/@earendil-works/pi-coding-agent) läuft in einem gehärteten Container in einer Linux-VM (Hyper-V und Podman). Die VM sieht keine Dateien des Rechners.
- Jeder Netzzugriff der Sandbox geht über ein Gateway mit Regeln je Projekt. Unbekannte Hosts brauchen eine Freigabe im Chat oder im Terminal.
- API-Keys kommen nie in die Sandbox. Das Gateway reicht Modellanfragen an den Server weiter, der den Key einsetzt.
- Der Agent arbeitet auf einer Kopie des ausgecheckten Branches. Seine Commits kommen erst nach einer Prüfung zurück: feste Regeln, ein Abgleich neuer Abhängigkeiten mit der OSV-Datenbank und auf Wunsch eine KI-Bewertung. Übernahmen, die ein Web-Chat anstößt, brauchen eine Freigabe am Rechner, außer das Projekt erlaubt sie dort.
- Werkzeuge wie C/C++, Rust, Go, Java, Deno und Bun lassen sich je Projekt einschalten, ebenso eigene Programme.
- Ein Tray-Symbol zeigt den Zustand der VM und weist auf alles hin, was auf eine Freigabe wartet. Im Leerlauf stoppt die VM von selbst.

**Direktmodus (Windows, Linux, macOS)**
- Ohne VM: Das Chat-Modell arbeitet als Agent direkt im Projektordner, hearth führt seine Werkzeuge auf dem Rechner aus. Lesen und Schreiben im Ordner gehen ohne Rückfrage; jeder Befehl und jeder Zugriff außerhalb des Ordners, auf Geheimnisse oder auf Dateien, die später Code ausführen (Git-Hooks, Editor-Einstellungen), wartet auf deine Freigabe am Rechner. Siehe [`docs/security/concept.md`](docs/security/concept.md).
- Eine Desktop-App (nativ, ohne Browser-Engine) zeigt Freigaben, Projekte mit ihrem Modus, Kopplung und Protokolle.

## Aufbau

```
Browser ──HTTPS──► Server (API, Web-App, SQLite oder Postgres)
                     ▲
                     │ WebSocket-Tunnel, Geräte-Token
                     │
Windows-Rechner: hearth-Hintergrunddienst ──► Hyper-V-VM (Podman)
                                                ├─ Gateway (Netzregeln, Freigaben)
                                                └─ Sandbox mit dem Agent und einer Kopie des Projekts
```

Wie die Teile zusammenspielen und warum sie so gebaut sind, steht in [`docs/how-it-works.md`](docs/how-it-works.md).

## Voraussetzungen

- **Server:** Docker mit Compose 2.24 oder neuer, auf amd64 oder arm64. Ein kleiner Rechner wie ein Raspberry Pi mit 4 GB RAM reicht.
- **Client (optional, für Agents):** Windows 11 Pro, Enterprise oder Education (Hyper-V ist nötig, Windows Home hat es nicht), x64 oder ARM64, 16 GB RAM empfohlen. Git und Podman installiert das Einrichtungsprogramm.

## Schnellstart

### Server

```sh
mkdir hearth && cd hearth
curl -LO https://raw.githubusercontent.com/johnnyjack123/hearth/main/docker-compose.yml
mkdir -p data && sudo chown 1000:1000 data
echo "PUBLIC_URL=http://<adresse-dieses-rechners>:8080" > .env
docker compose up -d
docker compose logs server | grep "setup code"
```

`PUBLIC_URL` im Browser öffnen, das erste Konto mit dem Einrichtungscode aus dem Protokoll anlegen (es wird Admin) und unter Einstellungen → API-Keys einen Key hinterlegen.

Postgres, Websuche, HTTPS und den Betrieb im Internet beschreibt [`docs/deploy.md`](docs/deploy.md). Eine kommentierte Vorlage für die `.env` liegt in [`.env.example`](.env.example) (englisch).

### Windows-Client

1. `hearth-setup-x64.exe` oder `hearth-setup-arm64.exe` aus dem [neuesten Release](https://github.com/johnnyjack123/hearth/releases/latest) herunterladen und starten. Es installiert die Programme für den eigenen Nutzer, prüft Hyper-V, installiert bei Bedarf Git und Podman, legt die VM an und baut ihre Images (zusammen bis zu etwa 1,4 GB Download). Im Release liegt auch ein ZIP mit Installationsskript, siehe [`docs/client.md`](docs/client.md).
2. **Rechner mit dem Server verbinden.** Gegen Ende fragt das Setup nach der Adresse des Servers, also `PUBLIC_URL` (etwa `https://hearth.example.org`). Es zeigt einen Code und öffnet eine Bestätigungsseite im Browser: dort mit dem eigenen Konto anmelden und den Code bestätigen. Ab dann nutzen die Agents des Rechners die Modelle dieses Kontos; die API-Keys verlassen den Server nie. Die Adresse lässt sich auch gleich mitgeben (`hearth setup --server <adresse>`). Später koppelt `hearth login <adresse>` neu, auch nach einem Umzug des Servers, und `hearth logout` hebt die Kopplung auf.
3. In einem Projektordner `hearth init` ausführen. Das Projekt erscheint dann im Web-Chat unter Zahnrad → Umgebung.
4. Alternativ startet `hearth` im Projektordner den Agent im Terminal.

Einzelheiten und Hilfe bei Problemen: [`docs/client.md`](docs/client.md).

### Linux und macOS

```sh
curl -fsSL https://github.com/johnnyjack123/hearth/releases/latest/download/install.sh | sh
```

Installiert `hearth`, den Hintergrunddienst und die Desktop-App für deinen Nutzer und startet `hearth setup`. Projekte laufen in einer Podman-Sandbox (Linux: das rootless Podman des Rechners ohne VM, macOS: eine Podman-Machine) oder im Direktmodus (`hearth project host on`). Details: [`docs/client-unix.md`](docs/client-unix.md).

## Dokumentation

| Dokument | Inhalt |
|---|---|
| [`docs/deploy.md`](docs/deploy.md) | Server: Installation, Einstellungen, Updates, Sicherung, HTTPS, Betrieb im Internet |
| [`docs/client.md`](docs/client.md) | Windows-Client: Einrichtung, Projekte, Web-Chat und Terminal, Updates, Probleme |
| [`docs/client-unix.md`](docs/client-unix.md) | Linux und macOS: Installation, Sandbox, Direktmodus, Freigaben |
| [`docs/security/concept.md`](docs/security/concept.md) | Sicherheitskonzept: Sandbox und Direktmodus |
| [`docs/how-it-works.md`](docs/how-it-works.md) | Wie die Teile funktionieren und warum sie so gebaut sind |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Das ursprüngliche Entwurfsdokument |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Phasen und nächste Schritte |

## Entwicklung

```sh
pnpm install
pnpm --filter @hearth/server dev     # API auf :8080
pnpm --filter @hearth/web dev        # Web-App auf :5173
pnpm --filter @hearth/server test
pnpm --filter @hearth/web e2e        # Browser-Test gegen einen isolierten Stack
cargo build
cargo test
```

Server und Web-App sind in TypeScript geschrieben (Hono, React, AI SDK), Client, VM-Dienst und Netz-Gateway in Rust.

## Stand

Hearth wird aktiv entwickelt und hat Version 1.0 noch nicht erreicht. Die Windows-Programme sind noch nicht signiert; SmartScreen und Microsoft Defender können deshalb warnen.

## Lizenz

Hearth ist freie Software unter der [GNU Affero General Public License v3.0 oder später](LICENSE.md). Wer eine veränderte Version als Dienst für andere betreibt, muss ihnen deren Quellcode anbieten.
