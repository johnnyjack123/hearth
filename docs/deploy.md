# Server einrichten und betreiben

Der Server enthält API und Web-App in einem Prozess. Er läuft als Container-Image für **amd64** und **arm64**, also auch auf einem Raspberry Pi mit 64-Bit-System. Dazu kommt immer der Container `markitdown`, der Dokumente für das Modell in Text umwandelt; optional laufen Postgres und SearXNG.

Images (gebaut von GitHub Actions, sobald sich auf `main` etwas am jeweiligen Image ändert, und für jedes Release):

| Image | Inhalt |
|---|---|
| `ghcr.io/johnnyjack123/hearth-server` | API und Web-App |
| `ghcr.io/johnnyjack123/hearth-markitdown` | Umwandlung von PDF, Word, Excel, PowerPoint, EPUB und HTML |

Tags: `latest` (Stand von `main`), `sha-<commit>` und bei einem Release die Versionsnummer, z. B. `0.3.0`. Eine Versionsnummer gibt es für beide Images, ein `sha-`-Tag nur für Commits, die das jeweilige Image geändert haben. Vor dem Veröffentlichen startet der Workflow das Server-Image einmal und prüft Health-Endpunkt, Web-App, Datenbank und dass der Prozess nicht als root läuft; `markitdown` führt einen Selbsttest ohne Netz aus.

## Installation mit Docker Compose

Voraussetzung: Docker mit Compose-Plugin ab 2.24.

1. Einen Ordner anlegen und `docker-compose.yml` aus dem Repository hineinlegen:
   ```sh
   mkdir hearth && cd hearth
   curl -LO https://raw.githubusercontent.com/johnnyjack123/hearth/main/docker-compose.yml
   ```
2. Den Datenordner selbst anlegen und dem Nutzer des Containers geben (uid 1000). Legt Docker ihn an, gehört er root, und der Server kann dort nichts speichern:
   ```sh
   mkdir -p data && sudo chown 1000:1000 data
   ```
3. Daneben eine `.env` anlegen (Vorlage [unten](#env-vorlage), englisch kommentiert auch als `.env.example` im Repository). Nötig ist nur `PUBLIC_URL`.
4. Starten:
   ```sh
   docker compose up -d
   docker compose logs -f server
   ```
5. `PUBLIC_URL` im Browser öffnen und das erste Konto anlegen. Es wird Admin und braucht den **Einrichtungscode**, den der Server ins Protokoll schreibt, solange es kein Konto gibt (`docker compose logs server | grep "setup code"`). So kann niemand einen frisch gestarteten Server übernehmen.
6. Unter Einstellungen → API-Keys mindestens einen Zugang hinterlegen.

Die Registrierung ist ab Werk geschlossen. Weitere Konten legt ein Admin unter Administration → Nutzer an, oder er öffnet die Registrierung unter Administration → Server.

## Daten und Sicherung

| Ordner | Inhalt |
|---|---|
| `data` | SQLite-Datenbank (ohne Postgres), hochgeladene Dateien, das erzeugte Server-Geheimnis `server-secret` |
| `postgres` | die Postgres-Datenbank, falls genutzt |

**Beide Ordner gehören in die Sicherung, zusammen mit der `.env`.** Die API-Keys der Nutzer sind mit einem Schlüssel verschlüsselt, der aus dem Server-Geheimnis abgeleitet wird (`HEARTH_SECRET` oder `data/server-secret`). Eine Datenbank ohne dieses Geheimnis enthält keine lesbaren Keys mehr; umgekehrt verrät eine gestohlene Sicherung ohne das Geheimnis keine Keys.

Sicherung mit kurzer Pause. Alle Container stoppen, damit auch Postgres nichts mehr schreibt. Der Ordner `postgres` gehört dem Postgres-Nutzer des Containers, deshalb `sudo`. Ohne Postgres `postgres` im `tar`-Befehl weglassen.

```sh
docker compose stop
sudo tar czf hearth-backup-$(date +%F).tar.gz data postgres .env docker-compose.yml
docker compose start
```

**Umzug auf einen anderen Rechner:** den gesicherten Ordner dort auspacken (Besitzer von `data` wieder uid 1000), `PUBLIC_URL` in der `.env` anpassen und `docker compose up -d`. Die Windows-Rechner ziehen mit `hearth login <neue Adresse>` nach; sie erkennen denselben Server und behalten ihre Kopplung.

## Updates

```sh
docker compose pull && docker compose up -d
```

`pull` allein tauscht den laufenden Container nicht aus. Vor einem Update zeigt Administration → Nutzer, ob gerade jemand den Server nutzt oder eine Antwort läuft; ein Neustart beendet laufende Antworten. Die Datenbank wird beim Start automatisch migriert. Offene Seiten verbinden sich danach von selbst neu und bieten „Neu laden“ an, sobald der Server einen neuen Stand meldet. Eine bestimmte Version statt `latest`: `HEARTH_IMAGE_TAG=0.3.0` in der `.env`.

## Postgres statt SQLite

SQLite reicht für einige Nutzer. Für Postgres in der `.env` setzen:

```sh
POSTGRES_PASSWORD=<nur Buchstaben und Ziffern, z. B. openssl rand -hex 32>
DATABASE_URL=postgres://hearth:<dasselbe Passwort>@postgres:5432/hearth
COMPOSE_PROFILES=postgres
```

`POSTGRES_PASSWORD` gilt nur beim ersten Start mit leerem Ordner `postgres`. Enthält das Passwort `/`, `+`, `=`, `#` oder `?`, muss es in `DATABASE_URL` kodiert sein (`%2F`, `%2B`, `%3D` …), sonst meldet der Server „Invalid URL“. Beim ersten Start kann der Server ein- bis zweimal neu starten, bis Postgres bereit ist.

## Websuche (Recherche-Modus)

Mit einem Suchanbieter zeigt das Zahnrad im Chat den Schalter **Recherche**. Ist er an, darf das Modell im Web suchen und gefundene Seiten ohne Rückfrage laden; Adressen, die es selbst bildet, fragen weiter nach, außer der Nutzer hat unter Einstellungen → Chat „Freier Webzugriff“ eingeschaltet. Einrichten unter Administration → Server → Websuche, „Suche testen“ prüft die Verbindung.

- **SearXNG** (selbst gehostet, kein Key): Profil `search` dazunehmen (`COMPOSE_PROFILES=postgres,search` bzw. `search`) und `SEARXNG_SECRET` setzen. Der Container hat keinen veröffentlichten Port, der Server erreicht ihn als `http://searxng:8080`; diese Adresse in der Web-App eintragen. Er braucht etwa 150–250 MB RAM.
- **Brave Search API** oder **Tavily**: Key in der Web-App eintragen. Er wird verschlüsselt gespeichert.

Suchmaschinen sperren einen Server, der viele Suchen in kurzer Zeit schickt, meist für etwa eine Stunde. SearXNG fragt deshalb zusätzlich Bing und Yahoo. Wer öfter an diese Grenze stößt, nimmt einen Anbieter mit Key.

Was nach außen geht: die Suchanfragen (an SearXNG und von dort an die Suchmaschinen, bzw. an Brave oder Tavily) und Abrufe der Seiten, beides vom Server aus.

## Dateien im Chat

Bilder und Textdateien brauchen keinen weiteren Dienst. Dokumente wandelt `markitdown` um; er startet mit `docker compose up -d` und hängt nur im internen Netz `documents`, ohne Weg ins Internet, mit schreibgeschütztem Dateisystem und höchstens 1 GB RAM. Gescannte PDFs liest er per Texterkennung (Deutsch und Englisch). Hochgeladene Dateien liegen in `data/files`; Löschen eines Chats löscht seine Dateien.

## HTTPS und Betrieb im Internet

Der Server spricht selbst nur HTTP. TLS übernimmt ein Reverse-Proxy davor. Für einen Server im Internet:

1. **Nur der Proxy ist von außen erreichbar.** Am Router 443 (und 80 für Let's Encrypt) an den Proxy weiterleiten, nie den Port des Servers. `HEARTH_PORT=127.0.0.1:8080` bindet ihn nur noch lokal; teilt sich der Proxy ein Docker-Netz mit dem Server, braucht der Server gar keinen veröffentlichten Port.
2. **`PUBLIC_URL=https://<name>`.** Die Anmelde-Cookies gelten dann nur noch über HTTPS. Auch im Heimnetz den Namen verwenden, nicht `http://<ip>`.
3. **`TRUSTED_PROXIES`** auf die Adresse des Proxys setzen, so wie der Server ihn sieht (kommagetrennt, auch als Bereich). Nur von dort glaubt der Server `X-Forwarded-For`; sonst zählen die Anmelde-Sperren alle Besucher zusammen. Für einen Proxy in Docker das gemeinsame Netz: `docker network inspect <netz> --format '{{(index .IPAM.Config 0).Subnet}}'`. Fehlt der Eintrag, warnt das Protokoll beim Start.
4. **Kostendeckel** pro Tag setzen (Administration → Server), falls ein Passwort doch einmal in falsche Hände gerät.
5. **Zwei-Faktor-Anmeldung** einschalten (Einstellungen → Konto).
6. Auf jedem Windows-Rechner `hearth login https://<name>`.

Der Proxy muss Streams (Server-Sent Events) und WebSockets durchreichen: Antworten kommen Wort für Wort, offene Seiten hören über `GET /api/events` auf Änderungen in ihren Chats, und die Windows-Rechner halten einen WebSocket zum Server (`/api/device/tunnel`). Der Server sorgt selbst dafür, dass das ohne Einstellungen am Proxy klappt:

- Jede Antwort mit `text/event-stream` trägt `X-Accel-Buffering: no`. nginx, und damit auch Nginx Proxy Manager, puffert sie dann nicht.
- Ist ein Stream 25 Sekunden still, schickt der Server eine Kommentarzeile (`: keepalive`). `/api/events` schickt alle 20 Sekunden ein `ping`, den WebSocket pingt der Server alle 30 Sekunden. So bleibt jede Verbindung unter dem Zeitlimit, nach dem ein Proxy stille Verbindungen schließt (nginx ab Werk 60 Sekunden).
- Ausnahme ist der Durchgang zu den Modellen für Agents (`/api/llm/`): Er reicht die Antwort des Anbieters Byte für Byte durch, ohne Kommentarzeilen.

Caddy und traefik brauchen deshalb nichts weiter, Nginx Proxy Manager nur den Schalter „Websockets Support“. Ein selbst eingerichtetes nginx braucht das WebSocket-Upgrade (`proxy_http_version 1.1` und die Header `Upgrade` und `Connection`) und `client_max_body_size 25m`, weil es ab Werk nur 1 MB große Uploads annimmt (Hearth erlaubt Dateien bis 20 MB).

### Nginx Proxy Manager

Den Server in das Docker-Netz von NPM hängen, in `docker-compose.yml` (`npm-proxy` steht für den Namen dieses Netzes):

```yaml
services:
  server:
    networks: [default, documents, npm-proxy]

networks:
  documents:
    internal: true
  npm-proxy:
    external: true
```

Im Proxy-Host von NPM:
- **Details:** Schema `http`, Forward Host `hearth-server`, Port `8080` (der Port im Container, nicht der veröffentlichte), **Websockets Support an**. Ohne diesen Schalter klappt die Kopplung, aber der Web-Chat findet den Rechner nicht.
- **SSL:** Let's-Encrypt-Zertifikat, Force SSL, HSTS.
- **Advanced:** bleibt leer. Pufferung und Zeitlimits regelt der Server (siehe oben), und NPM nimmt ab Werk große Uploads an.

`TRUSTED_PROXIES` ist dann der Adressbereich des Netzes `npm-proxy` (Befehl in Schritt 3 oben). Wer unter „Advanced“ schon Zeilen wie `proxy_buffering off` eingetragen hat, kann sie stehen lassen; sie schaden nicht.

### Caddy

Als `docker-compose.override.yml` neben `docker-compose.yml`:

```yaml
services:
  caddy:
    image: docker.io/library/caddy:2
    restart: unless-stopped
    ports: ["80:80", "443:443"]
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy-data:/data

volumes:
  caddy-data:
```

`Caddyfile` für einen öffentlichen Namen (Caddy holt das Zertifikat selbst):

```
hearth.example.org {
	reverse_proxy server:8080
}
```

### traefik

```yaml
services:
  server:
    networks: [default, documents, proxy]
    labels:
      - traefik.enable=true
      - traefik.docker.network=proxy
      - traefik.http.routers.hearth.rule=Host(`hearth.example.org`)
      - traefik.http.routers.hearth.entrypoints=websecure
      - traefik.http.routers.hearth.tls.certresolver=letsencrypt
      - traefik.http.services.hearth.loadbalancer.server.port=8080

networks:
  proxy:
    external: true
```

Namen von Netz, Entrypoint und Zertifikats-Resolver hängen an der eigenen traefik-Konfiguration.

### Heimnetz mit eigener Zertifizierungsstelle

Ohne öffentlichen Namen stellt Caddy mit `tls internal` Zertifikate seiner eigenen CA aus:

```
homeserver.lan, 192.168.1.50 {
	tls internal
	reverse_proxy server:8080
}
```

Das Wurzelzertifikat holen (`docker compose cp caddy:/data/caddy/pki/authorities/local/root.crt ./hearth-root.crt`) und auf dem Windows-Rechner **vor** `hearth login` in `%USERPROFILE%\.hearth\config.toml` eintragen:

```toml
[server]
ca = 'C:\Users\<name>\hearth-root.crt'
```

Der Client und das Gateway in der VM vertrauen für diesen Server dann nur dieser CA. PEM und DER (`.cer`) gehen beide. Das Gateway löst keine `.local`-Namen per mDNS auf; der Name muss über DNS auflösbar sein oder es wird die IP-Adresse genommen.

### Was der Server selbst schützt

- Anmelden höchstens 5-mal pro Minute und Adresse (IPv6 je /64), Registrieren 5-mal in 10 Minuten, Kopplungen 5 Starts in 10 Minuten je Adresse.
- Bei geschlossener Registrierung lehnt der Server ab, bevor er nachsieht, ob es eine Adresse schon gibt.
- Die Kopplungsseite zeigt, von welcher Adresse `hearth login` kam, und warnt, wenn das nicht die des Browsers ist.
- Gesperrte Konten verlieren auch den Zugang ihrer Rechner.
- Hochgeladene Dateien werden nur als Download ausgeliefert, mit einer Sicherheitsrichtlinie, die nichts darin ausführen lässt.

**Zwei-Faktor-Anmeldung zurücksetzen**, wenn jemand Handy und Backup-Codes verloren hat (Postgres; mit SQLite dieselben Befehle per `sqlite3 data/hearth.db`, bei Servern aus der Zeit vor 0.2.0 `data/harness.db`):

```sh
docker compose exec postgres psql -U hearth -d hearth -c "DELETE FROM \"twoFactor\" WHERE \"userId\" = (SELECT id FROM \"user\" WHERE email = 'name@example.org'); UPDATE \"user\" SET \"twoFactorEnabled\" = false WHERE email = 'name@example.org';"
```

### Als App installieren

Über `https://` lässt sich die Web-App wie eine App installieren (PWA): eigenes Symbol, eigenes Fenster ohne Browserleiste. Nur dort (und unter `localhost`) gibt es auch den Offline-Modus: Ein Service Worker hält App und zuletzt geladene Chats vor, ohne Verbindung zum Server lassen sie sich lesen. Über `http://` im Heimnetz geht die App nur mit Verbindung.

- **Android (Chrome):** Menü → „App installieren“ bzw. „Zum Startbildschirm hinzufügen“.
- **iPhone/iPad (Safari):** Teilen → „Zum Home-Bildschirm“.
- **Desktop (Chrome, Edge):** Symbol „App installieren“ in der Adressleiste.

Über `http://` im Heimnetz bieten Browser das nicht an (nur eine Verknüpfung). Die App lädt immer den aktuellen Stand vom Server; sie speichert nichts offline.

## Umstieg von LLM Harness (bis 0.1.x)

Mit 0.2.0 haben Images und Container neue Namen. Daten und `.env` bleiben gültig:

1. Sicherung anlegen (siehe oben).
2. In der eigenen `docker-compose.yml` die Image- und Containernamen ersetzen, oder die neue Datei aus dem Repository nehmen und eigene Ergänzungen (Netze, Labels) übertragen:
   ```sh
   cp docker-compose.yml docker-compose.yml.bak-$(date +%F)
   sed -i 's/llm-harness-/hearth-/g' docker-compose.yml
   docker compose pull && docker compose up -d && docker compose ps
   ```
3. Die alten Variablennamen (`HARNESS_SECRET`, `HARNESS_SETUP_CODE`, `HARNESS_PORT`, `HARNESS_IMAGE_TAG`) gelten weiter, neue Namen (`HEARTH_…`) haben Vorrang. Eine vorhandene `data/harness.db` nutzt der Server weiter. Eine bestehende Postgres-Datenbank behält Nutzer und Namen aus ihrer `DATABASE_URL`.
4. Alle müssen sich einmal neu anmelden, weil die Anmelde-Cookies neu heißen. Zeigt ein Reverse-Proxy auf den Containernamen, diesen auf `hearth-server` ändern.
5. Die alten Images entfernen, wenn alles läuft: `docker image rm ghcr.io/johnnyjack123/llm-harness-server ghcr.io/johnnyjack123/llm-harness-markitdown`.

Danach die Windows-Rechner umstellen ([`client.md`](client.md#umstieg-von-llm-harness-bis-01x)).

## Probleme

| Meldung | Ursache und Abhilfe |
|---|---|
| `SQLITE_CANTOPEN` oder `EACCES` auf `/data` | `data` gehört root, weil Docker den Ordner angelegt hat: `sudo chown 1000:1000 data` |
| `Invalid URL` beim Start | Sonderzeichen im Passwort in `DATABASE_URL`, siehe Postgres |
| `getaddrinfo ENOTFOUND postgres` | Postgres läuft nicht, meist fehlt `COMPOSE_PROFILES=postgres` |
| Anmelden klappt nicht, obwohl die Seite lädt | `PUBLIC_URL` stimmt nicht genau mit der Adresse im Browser überein, oder `https://` in `PUBLIC_URL` und Zugriff über `http://` |
| 502 Bad Gateway im Proxy | der Proxy zeigt auf den falschen Port; im gemeinsamen Docker-Netz ist es `8080`, nicht der veröffentlichte Port |
| Antworten erscheinen erst am Ende | der Proxy puffert Streams; nginx und NPM tun das nicht, weil der Server `X-Accel-Buffering: no` setzt. Bei anderen Proxys die Pufferung für den Server abschalten |
| Kopplung klappt, aber der Web-Chat findet den Rechner nicht | der Proxy reicht keine WebSockets durch; in NPM „Websockets Support“ einschalten |
| Android: `ERR_TOO_MANY_RETRIES` in Chrome oder der installierten App, nur im WLAN | Der Name zeigt im Heimnetz auf eine lokale Adresse (eigener DNS-Eintrag), und Chrome fehlt die Android-Berechtigung für das lokale Netzwerk (seit Android 17). Android blockt die Verbindung (`ERR_LOCAL_NETWORK_PERMISSION_MISSING` im Mitschnitt von `chrome://net-export`), Chrome versucht es 32-mal und gibt auf. Mit mobilen Daten geht es, weil der Name dort auf die öffentliche Adresse zeigt. Abhilfe: in den App-Einstellungen von Chrome die Berechtigung für Geräte im lokalen Netzwerk erlauben, oder den lokalen DNS-Eintrag weglassen |
| Hochladen scheitert mit `413` | der Proxy nimmt zu große Anfragen nicht an; bei nginx `client_max_body_size 25m` |
| „Dokumente umwandeln geht gerade nicht“ | `markitdown` läuft nicht: `docker compose ps -a`, `docker compose logs --tail 50 markitdown` |
| Suche „nicht erreichbar“ (`ENOTFOUND`) | Profil `search` fehlt in `COMPOSE_PROFILES` |

Der Server-Container prüft eine Verbindung auch selbst: `docker compose exec server node -e "fetch('http://searxng:8080/search?q=test&format=json').then(r => console.log(r.status), e => console.log(e.cause))"`.

## .env-Vorlage

```sh
# Adresse, unter der Browser und Windows-Rechner den Server erreichen. Sie
# steht im Kopplungs-Link und bestimmt, wofür die Anmelde-Cookies gelten.
PUBLIC_URL=http://192.168.1.50:8080

# Port auf dem Host; mit Reverse-Proxy z. B. 127.0.0.1:8080.
HEARTH_PORT=8080

# Image-Version: latest (Stand von main) oder eine Versionsnummer wie 0.3.0.
# Gilt für beide Images (server und markitdown).
HEARTH_IMAGE_TAG=latest

# Geheimnis für Sitzungen und die Verschlüsselung der API-Keys, mindestens
# 32 Zeichen (openssl rand -base64 48). Leer: Der Server erzeugt eins in
# data/server-secret. Geht es verloren, sind die gespeicherten Keys weg.
HEARTH_SECRET=

# Weitere Adressen der Web-App, kommagetrennt.
TRUSTED_ORIGINS=

# Hinter einem Reverse-Proxy: dessen Adresse oder Netz (auch CIDR). Nur von
# dort glaubt der Server X-Forwarded-For.
TRUSTED_PROXIES=

# Einrichtungscode für das erste Konto. Leer: Der Server erzeugt einen und
# schreibt ihn ins Protokoll, solange es kein Konto gibt.
HEARTH_SETUP_CODE=

# Postgres statt SQLite (siehe oben).
POSTGRES_PASSWORD=
# DATABASE_URL=postgres://hearth:<POSTGRES_PASSWORD>@postgres:5432/hearth
# COMPOSE_PROFILES=postgres

# SearXNG für die Websuche: Profil search dazunehmen und ein eigenes
# Geheimnis setzen (openssl rand -hex 32).
# COMPOSE_PROFILES=postgres,search
SEARXNG_SECRET=
```

## Für Entwickler

Das Image lokal bauen (Kontext ist das Repository):

```sh
docker build -f apps/server/Dockerfile -t hearth-server .
```

`.dockerignore` hält `apps/server/data` ausdrücklich heraus. Dort liegen die Entwicklungsdatenbank mit verschlüsselten Keys und das Geheimnis dazu; `pnpm deploy` würde sie sonst ins Image kopieren.

Das Gateway der VM kommt fertig gebaut mit dem Windows-Client (der Release-Workflow baut es unter Linux). Entwicklungs-Builds kompilieren es in der VM; die Versionen der TLS-Bibliotheken stehen fest in `images/gateway/Cargo.lock` (`bash scripts/gateway-lock.sh` nach Änderungen an den Abhängigkeiten).
