# llm-harness

Selbst gehosteter Multi-User-LLM-Chat (Server + Web-App) mit eigenen Sandboxes auf dem Rechner jedes Nutzers. Coding-Agents wie [pi](https://www.npmjs.com/package/@earendil-works/pi-coding-agent) laufen dort in isolierten Containern einer Hyper-V-VM. Netzzugriffe gehen nur über ein Gateway mit Freigaben, und Änderungen des Agents kommen erst nach einer Prüfung ins eigene Repository.

- **Server und Web-App** (`apps/server`, `apps/web`): Chat mit eigenen API-Keys je Nutzer. Außerdem der LLM-Zugang für die Sandboxes, damit kein Key im Container liegt.
- **`harness`** (`crates/`): CLI für Windows mit:
  - VM-Dienst
  - Projekten und Instanzen
  - Netz-Gateway
  - Git-Sync
  - Merge-Prüfung

Hintergrund und Entscheidungen stehen in [`ARCHITECTURE.md`](ARCHITECTURE.md), der Stand in [`docs/PROGRESS.md`](docs/PROGRESS.md).

## Quick Start: Server mit Docker

Das Image gibt es für amd64 und arm64, es läuft also auch auf einem Raspberry Pi mit 64-Bit-System.

1. `docker-compose.yml` in einen Ordner legen und dort den Datenordner für den Container-Nutzer (uid 1000) anlegen: `mkdir -p data && sudo chown 1000:1000 data`. Daneben eine `.env` mit diesem Inhalt anlegen und anpassen (dieselbe Vorlage wie `.env.example`):

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
   ```

2. Solange das Repository privat ist, ist auch das Image privat. Einmal anmelden, mit einem GitHub-Token mit `read:packages`:

   ```sh
   docker login ghcr.io -u <github-name>
   ```

3. Starten und `PUBLIC_URL` im Browser öffnen. Der erste registrierte Nutzer wird Admin.

   ```sh
   docker compose up -d
   ```

4. In den Einstellungen API-Keys hinterlegen, etwa für DeepSeek, OpenAI oder Anthropic, und losschreiben.

Updates, Postgres und die Grenzen beim Betrieb im Netz: [`docs/deploy.md`](docs/deploy.md).

## Quick Start: Sandbox auf Windows

Voraussetzungen:
- Windows 11 Pro mit Hyper-V
- Podman mit einer Machine `harness`; die Einrichtung steht in [`docs/spikes/phase-0.md`](docs/spikes/phase-0.md) §12
- Rust
- Git

```powershell
cargo build
target\debug\harness.exe vmd install          # VM-Dienst einrichten (einmal, fragt per UAC)
target\debug\harness.exe image build          # Base- und Gateway-Image in der VM bauen
target\debug\harness.exe login http://<server>:8080   # Rechner mit dem eigenen Konto koppeln
cd <dein-git-repo>
<pfad>\harness.exe                           # Projekt anlegen und pi in der Sandbox starten
```

Danach holen `harness sync` und `harness merge` die Commits des Agents ins eigene Repository. `merge` prüft sie vorher. Alle Befehle stehen in [`CLAUDE.md`](CLAUDE.md#befehle).

## Entwicklung

```sh
pnpm install
pnpm --filter @harness/server dev    # API auf :8080
pnpm --filter @harness/web dev       # Web-App auf :5173
pnpm --filter @harness/server test
cargo test
```

Hinweise für Agents und Stolpersteine: [`CLAUDE.md`](CLAUDE.md).
