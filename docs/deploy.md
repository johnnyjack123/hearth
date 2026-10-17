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
2. `docker-compose.yml` aus dem Repository in einen Ordner legen, daneben eine `.env` (Vorlage unten).
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

Aktualisieren: `docker compose pull && docker compose up -d`. Die Daten liegen im Volume `harness-data`: SQLite-Datenbank und, falls `HARNESS_SECRET` leer ist, das erzeugte Geheimnis. **Das Volume sichern**: Ohne Geheimnis sind die gespeicherten API-Keys nicht mehr lesbar.

Postgres statt SQLite: `POSTGRES_PASSWORD` und `DATABASE_URL` in der `.env` setzen, dann `docker compose --profile postgres up -d`.

### Grenzen heute

- **Nur `http://` zwischen Gateway und Server.** Das Gateway in der VM spricht noch kein TLS (ROADMAP, Known issues). Im Heimnetz geht das. Das Geräte-Token läuft dann aber unverschlüsselt durchs LAN. Für einen Server im Internet braucht es zuerst TLS im Gateway.
- `.local`-Namen (mDNS) löst das Gateway nicht auf. In `harness login` die IP-Adresse oder einen DNS-Namen verwenden.

## `.env`-Vorlage

Compose liest die `.env` neben `docker-compose.yml` und reicht sie an den Server weiter. Inhalt der Vorlage `.env.example`:

```sh
# Vorlage für .env neben docker-compose.yml: kopieren, anpassen, nie committen.

# Adresse, unter der Browser und harness-CLI den Server erreichen. Sie steht
# im Kopplungs-Link (harness login) und bestimmt, für welche Adresse die
# Login-Cookies gelten. Beispiel Raspberry Pi im Heimnetz:
PUBLIC_URL=http://192.168.1.50:8080

# Port auf dem Host.
HARNESS_PORT=8080

# Image-Version: latest (Stand von main), sha-<commit> oder eine Versionsnummer.
HARNESS_IMAGE_TAG=latest

# Geheimnis für Sitzungen und die Verschlüsselung der API-Keys, mindestens
# 32 Zeichen (z. B. `openssl rand -base64 48`). Leer lassen: Der Server
# erzeugt eins und legt es im Volume harness-data ab. Geht es verloren, sind
# die gespeicherten Keys nicht mehr lesbar; das Volume also sichern.
HARNESS_SECRET=

# Weitere Adressen der Web-App, kommagetrennt, z. B. wenn der Server auch
# unter einem Namen erreichbar ist: http://raspberrypi:8080
TRUSTED_ORIGINS=

# Postgres statt SQLite: Passwort setzen, DATABASE_URL einkommentieren und
# mit `docker compose --profile postgres up -d` starten.
POSTGRES_PASSWORD=
# DATABASE_URL=postgres://harness:<POSTGRES_PASSWORD>@postgres:5432/harness
```

## Lokal bauen

```sh
docker build -f apps/server/Dockerfile -t llm-harness-server .
```

`.dockerignore` hält den Datenordner `apps/server/data` ausdrücklich aus dem Build-Kontext heraus. Darin liegen die Entwicklungsdatenbank mit verschlüsselten Keys und das Geheimnis dazu. `pnpm deploy` würde ihn sonst ins Image kopieren.
