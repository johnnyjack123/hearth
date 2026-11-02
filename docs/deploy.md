# Server dauerhaft betreiben

Der Server (API und Web-App in einem Prozess) gibt es als Container-Image für **amd64** und **arm64**, also auch für einen Raspberry Pi mit 64-Bit-System. GitHub baut es bei jedem Push auf `main`, der den Server, die Web-App oder `packages/` ändert (`.github/workflows/server-image.yml`):

- `ghcr.io/johnnyjack123/llm-harness-server:latest`: Stand von `main`
- `…:sha-<commit>`: ein bestimmter Commit
- `…:<version>`: bei einem Tag `v1.2.3`

Jede Architektur baut nativ auf ihrem Runner. Vor dem Hochladen startet der Workflow das Image einmal und prüft:
- Health-Endpunkt
- Web-App
- Datenbank im Volume
- Prozess läuft nicht als root

## Raspberry Pi (oder ein anderer Rechner mit Docker)

1. Docker mit Compose-Plugin installieren (Compose ≥ 2.24 wegen `env_file … required: false`).
2. `docker-compose.yml` aus dem Repository in einen Ordner legen, daneben eine `.env` (Vorlage unten). Den Datenordner vorher selbst anlegen und dem Nutzer des Containers geben (uid 1000). Legt Docker ihn an, gehört er root, und der Server kann nicht hineinschreiben:
   ```sh
   mkdir -p data && sudo chown 1000:1000 data
   ```
3. Solange das Repository privat ist, ist auch das Image privat. Einmal anmelden, mit einem GitHub-Token mit `read:packages`:
   ```sh
   docker login ghcr.io -u johnnyjack123
   ```
4. Starten:
   ```sh
   docker compose up -d
   docker compose logs -f server
   ```
5. Im Browser `PUBLIC_URL` öffnen, Konto anlegen, API-Keys hinterlegen. Auf dem Windows-Rechner dann `harness login http://<pi>:8080`.

Aktualisieren: `docker compose pull && docker compose up -d`. Die Daten liegen neben `docker-compose.yml`:
- `data`: SQLite-Datenbank und, falls `HARNESS_SECRET` leer ist, das erzeugte Geheimnis
- `postgres`: die Postgres-Datenbank, falls genutzt

**Beide Ordner sichern.** Ohne Geheimnis sind die gespeicherten API-Keys nicht mehr lesbar.

Postgres statt SQLite: `POSTGRES_PASSWORD`, `DATABASE_URL` und `COMPOSE_PROFILES=postgres` in der `.env` setzen. Ohne `COMPOSE_PROFILES` braucht jeder Aufruf `docker compose --profile postgres …`. Der Server startet beim ersten Mal eventuell ein- bis zweimal neu, bis Postgres bereit ist.

- **`Invalid URL` beim Start:** Das Passwort in `DATABASE_URL` enthält `/`, `+`, `=`, `#` oder `?`. Diese Zeichen kodieren (`/` → `%2F`, `+` → `%2B`, `=` → `%3D`). Die Alternative ist ein Passwort nur aus Buchstaben und Ziffern, dann aber den Ordner `postgres` leeren, weil Postgres das alte Passwort behält.
- **`EACCES` auf `/data`** (etwa `mkdir '/data/files'`): Der Ordner `data` gehört root, weil Docker ihn beim ersten Start selbst angelegt hat. Mit Postgres fiel das bis v0.1.11 nicht auf, erst die Dateiablage schreibt dort. Abhilfe: `sudo chown 1000:1000 data`. Ist der Ordner leer, geht es auch ohne sudo: `docker compose stop server && rmdir data && mkdir data && docker compose up -d server`. Seit v0.1.14 startet der Server trotzdem, nur Uploads melden den Fehler.
- **`getaddrinfo ENOTFOUND postgres`:** Der Postgres-Container läuft nicht, meist fehlt `COMPOSE_PROFILES=postgres` in der `.env`. Ohne das Profil legt Compose auch den Ordner `postgres` nicht an.
- **Anmelden klappt nicht, obwohl die Seite lädt:** `PUBLIC_URL` muss genau die Adresse sein, die im Browser steht, mit dem Port aus `HARNESS_PORT`.

### Websuche (Recherche-Modus)

Mit einem Suchanbieter zeigt der Composer den Schalter **Recherche**. Ist er an, darf das Modell im Web suchen und gefundene Seiten ohne Rückfrage laden. Adressen, die es selbst zusammensetzt, fragen weiter nach. Den Anbieter wählt ein Admin unter **Administration → Server → Websuche**; „Suche testen“ zeigt gleich, ob er antwortet.

- **SearXNG** (selbst gehostet, kein Key): `docker compose --profile search up -d` startet den Container `llm-harness-searxng`. Ohne veröffentlichten Port erreicht ihn nur der Server, als `http://searxng:8080`; diese Adresse in der Web-App eintragen. Die Einstellungen (JSON-Ausgabe an, Limiter aus) stehen in `docker-compose.yml` unter `configs`, eine eigene `settings.yml` ist nicht nötig. Der Container braucht etwa 150–250 MB RAM, das Image ist rund 90 MB groß.
- **Brave Search API** oder **Tavily**: Key in der Web-App eintragen. Er wird wie die Anbieter-Keys verschlüsselt gespeichert; Admins sehen danach nur die letzten vier Zeichen.

Fehler beim Test:
- **HTTP 403 von SearXNG:** Die JSON-Ausgabe ist aus. Bei einer eigenen Instanz `search.formats` um `json` ergänzen.
- **„Die Suchmaschinen hinter SearXNG blocken gerade …“:** Google, Brave und DuckDuckGo sperren einen Server, der viele Suchen in kurzer Zeit schickt, meist für etwa eine Stunde (am 22.10. nach rund 80 Suchen in 30 Minuten). Die Compose-Datei schaltet deshalb zusätzlich Bing und Yahoo zu. Nach einer Änderung der Compose-Datei mit `docker compose --profile search up -d` neu starten. Wer häufiger an die Grenze kommt, nimmt einen Suchanbieter mit Key (Tavily: 1 000 Anfragen im Monat frei, Brave: 5 $ Guthaben im Monat).
- **„nicht erreichbar“, Name unbekannt (`ENOTFOUND`):** Den Container gibt es nicht. Compose legt Dienste mit Profil nur an, wenn das Profil aktiv ist: `COMPOSE_PROFILES=postgres,search` in der `.env` (ohne Leerzeichen) oder `docker compose --profile search up -d`. `--force-recreate` ist nicht nötig, `up -d` legt fehlende Container von selbst an. Prüfen mit `docker compose ps -a`. `configs` mit `content` braucht Compose ≥ 2.23 (`docker compose version`).
- **„nicht erreichbar“, Verbindung abgelehnt (`ECONNREFUSED`):** Der Container läuft nicht oder startet noch. `docker compose logs --tail 50 searxng` zeigt den Grund.
- Direkt aus dem Server-Container prüfen: `docker compose exec server node -e "fetch('http://searxng:8080/search?q=test&format=json').then(r => console.log(r.status), e => console.log(e.cause))"`. Antwort `200` heißt: Die Verbindung steht.
- **HTTP 429:** Das Kontingent des Anbieters ist aufgebraucht. Bei SearXNG blockieren die abgefragten Suchmaschinen gelegentlich; dann liefert es weniger Treffer.

Was nach außen geht: die Suchanfragen selbst (an SearXNG und von dort an die Suchmaschinen, bzw. an Brave oder Tavily) und Abrufe der gefundenen Seiten. Das Modell soll keine privaten Details in Suchanfragen schreiben, verhindern lässt sich das aber nicht.

### Dateien im Chat (markitdown)

Bilder und Textdateien gehen ohne weiteren Dienst. PDF, Word, Excel, PowerPoint, EPUB und HTML wandelt der Container `llm-harness-markitdown` in Markdown um, das das Modell liest. Er steht seit dem 31.10. ohne Profil in `docker-compose.yml`, `docker compose up -d` startet ihn also mit. Er hängt nur im internen Netz `documents`: kein Weg ins Internet, nur der Server erreicht ihn (`MARKITDOWN_URL`, Standard `http://markitdown:8080`). Dazu schreibgeschütztes Dateisystem, keine Rechte, höchstens 1 GB RAM. Das Image ist etwa 440 MB groß (ausgepackt), einmal herunterzuladen.

- **Ältere Compose-Datei:** Den Dienst `markitdown`, beim Server `environment` und `networks` sowie das Netz `documents` aus der aktuellen `docker-compose.yml` übernehmen, dann `docker compose pull && docker compose up -d`. Vorher eine Sicherung der Datei daneben legen.
- **„Dokumente umwandeln geht gerade nicht“:** Der Container läuft nicht. `docker compose ps -a` und `docker compose logs --tail 50 markitdown` zeigen den Grund.
- **„In dem Dokument steht kein lesbarer Text“:** Ein gescanntes PDF ohne Texterkennung. Als Bild (Screenshot der Seite) geht es mit einem Modell, das Bilder versteht.
- Hochgeladene Dateien liegen in `data/files`. Löschen eines Chats löscht seine Dateien; Uploads, die nie in einer Nachricht landeten, gehen nach einem Tag.

### Grenzen heute

- Mit `http://` läuft das Geräte-Token unverschlüsselt durchs Netz. Im Heimnetz mag das gehen, für einen Server im Internet immer HTTPS nehmen (nächster Abschnitt).
- `.local`-Namen (mDNS) löst das Gateway nicht auf. In `harness login` die IP-Adresse oder einen DNS-Namen verwenden.
- Ein Server auf dem Windows-Rechner selbst ist für das Gateway nur per `http://localhost:<port>` erreichbar, nicht per `https://`: Die VM erreicht ihn über eine andere Adresse, auf die das Zertifikat nicht ausgestellt ist.

## HTTPS mit einem Reverse-Proxy (Caddy)

Der Server selbst spricht nur HTTP. TLS übernimmt ein Reverse-Proxy davor, hier Caddy. Das Gateway in der VM verschlüsselt seinerseits selbst (rustls, Mozillas Wurzelzertifikate sind eingebaut) und prüft Zertifikat und Namen; abschalten lässt sich die Prüfung nicht. `harness login https://…` reicht, das Gateway übernimmt die Adresse beim nächsten `harness run` bzw. `harness net start`.

Caddy neben dem Server im selben Compose-Projekt, z. B. als `docker-compose.override.yml` neben `docker-compose.yml` (Compose liest sie automatisch):

```yaml
services:
  caddy:
    image: docker.io/library/caddy:2
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy-data:/data

volumes:
  caddy-data:
```

In der `.env` dann `PUBLIC_URL=https://<name>` setzen (der Kopplungs-Link und die Login-Cookies hängen daran). Den Port 8080 braucht nur noch Caddy; wer ihn nicht im Netz offen lassen will, setzt `HARNESS_PORT=127.0.0.1:8080`.

**Server im Internet (VPS) mit öffentlichem Namen:** Caddy holt das Zertifikat selbst bei Let's Encrypt, Ports 80 und 443 müssen von außen erreichbar sein. `Caddyfile`:

```
harness.example.com {
	reverse_proxy server:8080
}
```

Auf dem Windows-Rechner: `harness login https://harness.example.com`. Sonst ist nichts einzurichten.

**Heimnetz ohne öffentlichen Namen (Raspberry Pi):** Caddy stellt mit `tls internal` Zertifikate seiner eigenen CA aus. Der Name muss im LAN auflösbar sein (Router-DNS, z. B. `raspberrypi.fritz.box`, kein `.local`), oder man nimmt die IP-Adresse:

```
raspberrypi.fritz.box, 192.168.1.50 {
	tls internal
	reverse_proxy server:8080
}
```

Dann das Wurzelzertifikat von Caddy auf den Windows-Rechner holen:

```sh
docker compose cp caddy:/data/caddy/pki/authorities/local/root.crt ./harness-root.crt
```

und in `%USERPROFILE%\.harness\config.toml` eintragen, **vor** `harness login`:

```toml
[server]
ca = 'C:\Users\<name>\harness-root.crt'
```

Für den Server vertraut harness dann nur dieser CA, auf dem Windows-Rechner (Kopplung, `harness doctor`) und im Gateway, das die Zertifikate mit dem Geräte-Token über seinen Steuerkanal bekommt. Die Datei darf PEM (wie Caddys `root.crt`) oder DER (Windows-Export `.cer`) sein. `harness logout` lässt den Eintrag stehen. Die CA in Windows zu importieren ist nicht nötig. Danach `harness login https://raspberrypi.fritz.box`.

Antworten der Modelle kommen als Stream (Server-Sent Events); Caddy reicht sie ohne Puffern durch. Schlägt der Handshake fehl, zeigen `harness net status` und `harness net watch` bei `llm.harness` „TLS zum harness-Server fehlgeschlagen“. Die genaue Ursache steht in der Fehlerseite an den Agent (Status 502, z. B. „certificate not valid for name …“ oder `UnknownIssuer`) und in `podman logs` des Gateway-Containers.

### Gateway-Image

Das Gateway-Image kompiliert in der VM die TLS-Bibliotheken (rustls, ring, webpki-roots) und lädt sie dafür beim ersten `harness image build gateway` von crates.io, einige MB. Versionen und Prüfsummen stehen fest in `images/gateway/Cargo.lock`; die Datei ist der Workspace-Lock, beschränkt aufs Gateway (`bash scripts/gateway-lock.sh`, ein Test meldet Abweichungen). Auf dem Windows-Entwicklungsrechner baut das Gateway ohne TLS (Feature `tls` aus), weil `ring` dort clang bräuchte. Gelintet und getestet wird der TLS-Code im Workflow `.github/workflows/gateway.yml` und beim Bau des Images.

## `.env`-Vorlage

Compose liest die `.env` neben `docker-compose.yml` und reicht sie an den Server weiter. Inhalt der Vorlage `.env.example`:

```sh
# Vorlage für .env neben docker-compose.yml: kopieren, anpassen, nie committen.

# Adresse, unter der Browser und harness-CLI den Server erreichen. Sie steht
# im Kopplungs-Link (harness login) und bestimmt, für welche Adresse die
# Login-Cookies gelten; der Port ist der aus HARNESS_PORT. Beispiel Raspberry
# Pi im Heimnetz:
PUBLIC_URL=http://192.168.1.50:8080

# Port auf dem Host.
HARNESS_PORT=8080

# Image-Version: latest (Stand von main), sha-<commit> oder eine Versionsnummer.
HARNESS_IMAGE_TAG=latest

# Geheimnis für Sitzungen und die Verschlüsselung der API-Keys, mindestens
# 32 Zeichen (z. B. `openssl rand -base64 48`). Leer lassen: Der Server
# erzeugt eins und legt es im Ordner data ab. Geht es verloren, sind die
# gespeicherten Keys nicht mehr lesbar; den Ordner also sichern.
HARNESS_SECRET=

# Weitere Adressen der Web-App, kommagetrennt, z. B. wenn der Server auch
# unter einem Namen erreichbar ist: http://raspberrypi:8080
TRUSTED_ORIGINS=

# Postgres statt SQLite: Passwort setzen und die beiden Zeilen darunter
# einkommentieren. COMPOSE_PROFILES startet den Postgres-Container mit.
# Das Passwort steht auch in DATABASE_URL, deshalb nur Buchstaben und
# Ziffern (z. B. `openssl rand -hex 32`). Sonst müssen / + = usw. dort
# URL-kodiert sein (%2F, %2B, %3D). POSTGRES_PASSWORD gilt nur beim ersten
# Start mit leerem Ordner postgres; danach ändert es nichts mehr.
POSTGRES_PASSWORD=
# DATABASE_URL=postgres://harness:<POSTGRES_PASSWORD>@postgres:5432/harness
# COMPOSE_PROFILES=postgres

# Websuche mit SearXNG: Profil search dazunehmen (mit Postgres:
# COMPOSE_PROFILES=postgres,search) und ein eigenes Geheimnis setzen
# (openssl rand -hex 32).
# COMPOSE_PROFILES=search
SEARXNG_SECRET=
```

## Lokal bauen

```sh
docker build -f apps/server/Dockerfile -t llm-harness-server .
```

`.dockerignore` hält den Datenordner `apps/server/data` ausdrücklich aus dem Build-Kontext heraus. Darin liegen die Entwicklungsdatenbank mit verschlüsselten Keys und das Geheimnis dazu. `pnpm deploy` würde ihn sonst ins Image kopieren.
