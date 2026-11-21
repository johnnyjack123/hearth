# hearth unter Linux und macOS

Stand 21.11.2026. Unter Linux und macOS gibt es den **Direktmodus**: Der Agent arbeitet ohne VM in deinem Projektordner, jeder Befehl und jeder Zugriff außerhalb des Ordners braucht deine Freigabe (Konzept: [`security/concept.md`](security/concept.md)). Die Sandbox mit Podman folgt mit einem späteren Update; unter Windows gibt es beide Modi ([`client.md`](client.md)).

## Installation

```sh
curl -fsSL https://github.com/johnnyjack123/hearth/releases/latest/download/install.sh | sh
```

Das Skript braucht keine Root-Rechte:

- **Linux:** `hearth`, `hearthd` und `hearth-app` landen in `~/.local/bin` (anderer Ort: `HEARTH_PREFIX=…`).
- **macOS:** `Hearth.app` landet in `~/Applications`, Links auf ihre Programme in `~/.local/bin`. Die Programme sind nicht von Apple beglaubigt; das Skript entfernt die Quarantäne-Markierung, damit macOS sie startet.

Danach läuft `hearth setup`:

1. git prüfen (fehlt es: über den Paketmanager bzw. `brew install git`)
2. mit deinem hearth-Server koppeln (der Browser öffnet sich zum Bestätigen)
3. den Hintergrunddienst bei jeder Anmeldung starten (Linux: `~/.config/autostart/hearth.desktop`, macOS: `~/Library/LaunchAgents/io.hearth.daemon.plist`)
4. unter Linux die Hearth-App ins Startmenü eintragen

Offline: ein heruntergeladenes Archiv `hearth-linux-<x64|arm64>.tar.gz` bzw. `hearth-macos.tar.gz` mit `sh install.sh <archiv>` installieren. Aus den Quellen: `cargo build --release -p hearth-cli -p hearth-app` (Rust ab 1.95; unter Linux zusätzlich `libxkbcommon` und für OpenSSL `perl` und einen C-Compiler).

## Projekt im Direktmodus

```sh
cd ~/projekte/website
hearth init                 # Projekt anlegen (mit Git, falls nötig) und Web-Chats anbieten
hearth project host on      # Direktmodus
```

Im Web-Chat wählst du unter Zahnrad → Umgebung „website, direkt auf dem Rechner“. Der Agent ist dann das Chat-Modell selbst, mit Werkzeugen auf deinem Rechner:

- Lesen, Schreiben, Suchen im Projektordner geht ohne Rückfrage, außer bei Geheimnissen (`.env`, Schlüssel) und bei Dateien, die später von selbst Code ausführen (`.git`, Hooks, Editor- und Agent-Einstellungen).
- Jeder Befehl und jeder Zugriff außerhalb des Ordners wartet auf deine Freigabe. Freigegebene Befehle laufen mit deinen Rechten.

Weitere Schalter:

```sh
hearth project host on --browser-approvals   # Freigaben auch im Web-Chat (wer den Server kontrolliert, kann dann Befehle freigeben)
hearth project host on --writes ask          # auch jedes Schreiben im Ordner fragen
hearth project host off                      # zurück zur Sandbox (unter Linux/macOS noch nicht verfügbar)
```

## Freigaben

- **Hearth-App** (`hearth-app`, unter Linux im Startmenü, unter macOS in `~/Applications`): Seite „Freigaben“ mit Befehl, Ordner und Grund; dazu Projekte mit ihrem Modus, Kopplung, Hintergrunddienst und Protokolle.
- **Benachrichtigung:** Der Hintergrunddienst meldet jede neue Freigabe auf dem Desktop (Linux über `notify-send`, ein Klick auf „Öffnen“ startet die App; macOS über die Mitteilungszentrale).
- **Terminal:** `hearth host` listet offene Freigaben, `hearth host approve <ID>` (mit `--chat` für den ganzen Chat), `hearth host deny <ID>`, `hearth host watch` fragt sie nacheinander ab.
- **Protokoll:** `hearth host log <projekt>` bzw. die Seite „Protokoll“ der App zeigt jeden Werkzeugaufruf mit Entscheidung, ohne Dateiinhalte.

## Wo was liegt

| | Linux | macOS |
|---|---|---|
| Konfiguration | `~/.hearth` | `~/.hearth` |
| Geräte-Token | `~/.hearth/tokens.json` (nur für dich lesbar) | Schlüsselbund |
| Protokoll des Dienstes | `~/.hearth/daemon.log` | `~/.hearth/daemon.log` |

## Deinstallieren

```sh
hearth daemon uninstall     # Autostart weg, Dienst stoppt
hearth logout               # Gerät beim Server entfernen
rm ~/.local/bin/hearth ~/.local/bin/hearthd ~/.local/bin/hearth-app
rm -rf ~/.hearth            # Konfiguration und Protokolle (optional)
# Linux: rm ~/.local/share/applications/io.hearth.app.desktop
# macOS: rm -rf ~/Applications/Hearth.app
```
