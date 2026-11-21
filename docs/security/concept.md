# Sicherheitskonzept: Sandbox und Direktmodus

Stand 21.11.2026. Ersetzt den „ungesicherten Host-Modus“ aus ARCHITECTURE.md §3 und die Idee „Host-Ausführung mit Einzelbestätigung“ aus der ROADMAP. Entscheidungen des Nutzers vom 21.11.: Freigaben am Rechner, im Browser nur per Opt-in je Projekt; der Agent im Direktmodus ist das Chat-Modell des Servers, die Werkzeuge führt ein Rust-Proxy auf dem Rechner aus; die Desktop-App wird mit egui gebaut.

## 1. Zwei Modi je Projekt

| | **Sandbox** (bisher) | **Direkt** (neu) |
|---|---|---|
| Wo der Agent arbeitet | Container in einer Linux-VM (Windows: Hyper-V, macOS: Podman Machine) bzw. rootless Podman (Linux), Arbeitskopie im Volume | im Projektordner auf dem Rechner |
| Wer die Werkzeuge ausführt | die Harness (pi) im Container | der Proxy in `hearth` auf dem Rechner (Rust) |
| Wer das Modell aufruft | pi über das Gateway und den Server | der Server, wie in normalen Chats |
| Dateien im Projektordner | nur über Sync, Merge mit Prüfung | lesen und schreiben ohne Rückfrage, außer geschützte Pfade |
| Dateien außerhalb | unerreichbar | jeder Zugriff mit Rückfrage |
| Befehle | frei im Container | **jeder** Befehl mit Rückfrage, läuft mit den Rechten des Nutzers |
| Netz | Gateway mit Regeln je Projekt | Werkzeuge haben kein Netz; freigegebene Befehle schon; Web über den Server wie im Chat |
| RAM | VM ab 1 GB | ein paar MB im Hintergrunddienst |

Der Modus gehört dem Rechner: Er steht in der Projektdatei (`[host]`), umschalten geht nur in der Desktop-App oder mit `hearth project mode`, nie aus dem Browser. Ein Projekt im Direktmodus bietet Web-Chats die Pseudo-Harness `host` an; die Projekte im Sandbox-Modus bieten wie bisher pi an.

## 2. Wem was vertraut wird

- **Das Modell** ist nie vertrauenswürdig (Prompt-Injection aus Seiten, Dateien, Werkzeugausgaben).
- **Der Server** ist halb vertrauenswürdig: Er steht im Internet und kann gekapert werden. Er liefert den Code der Web-App aus, also kann alles, was im Browser passiert, von ihm gefälscht werden.
- **Der Rechner** (Proxy, Hintergrunddienst, Desktop-App, CLI) ist vertrauenswürdig. Nur er entscheidet über Freigaben.
- **Freigegebene Befehle** laufen mit allen Rechten des Nutzers. Ein freigegebener Befehl kann alles, was der Nutzer kann, auch die Konfiguration von hearth ändern oder spätere Freigaben fälschen. Das lässt sich ohne Sandbox nicht verhindern (Claude Code hat dieselbe Grenze). Die Oberfläche sagt das deutlich, und `npm test`, `cargo build`, `make` & Co. führen Code aus, den der Agent geschrieben haben kann.

Daraus folgt die Invariante, die schon für die Sandbox galt: **Freigaben entscheidet nie etwas, das das Modell oder der Server steuern kann.** Im Direktmodus heißt das:

1. Was das Modell kann, ist genau die Liste der Werkzeuge unten. Ausführen kann es nur über `shell`, und `shell` fragt immer.
2. Der Server kann Werkzeugaufrufe schicken (so wie ein gekapertes Modell), aber keine Freigaben erteilen, keinen Modus umschalten, keine Ordner hinzufügen. Er bekommt keine generische Ausführung; der Rechner prüft jeden Aufruf gegen seine eigene Konfiguration.
3. Freigaben aus dem Browser gelten nur, wenn das Projekt sie am Rechner erlaubt (`[host] browser_approvals = true`, Desktop-App mit Warnung). Wer das einschaltet, vertraut dem Server wie dem eigenen Rechner: Ein gekaperter Server könnte dann Befehle freigeben.

## 3. Werkzeuge im Direktmodus

Alle Werkzeuge laufen im Proxy (`hearth-core/src/host/`), ohne Shell und ohne Unterprozess, außer `shell`.

| Werkzeug | Zweck | Grenzen |
|---|---|---|
| `read` | Textdatei lesen, mit Zeilenbereich | 256 KB je Aufruf, Binärdateien nur als Hinweis |
| `write` | Datei anlegen oder ersetzen | 1 MB, legt fehlende Ordner an |
| `edit` | genaue Textstellen ersetzen (mehrere je Aufruf) | jede Stelle muss genau einmal vorkommen |
| `list` | Ordnerinhalt | 1000 Einträge |
| `find` | Dateien nach Glob-Muster | beachtet `.gitignore`, 1000 Treffer |
| `grep` | Inhalt nach Regex | beachtet `.gitignore`, Regex ohne Backtracking (`regex`-Crate), 500 Treffer |
| `shell` | Befehl ausführen | immer mit Rückfrage; Timeout 2 min (höchstens 30), Ausgabe 64 KB, ganzer Prozessbaum endet bei Abbruch |

Die Shell ist unter Linux und macOS die Login-Shell des Nutzers bzw. `bash`, unter Windows Git Bash, wenn installiert, sonst PowerShell. Der System-Prompt nennt sie, damit das Modell passende Befehle schreibt.

## 4. Pfadregeln

Jeder Pfad wird vor der Entscheidung aufgelöst: relativ zum Projektordner, `..` lexikalisch, dann Symlinks und Junctions über den existierenden Teil (bei neuen Dateien über den Elternordner). Ein Link im Projekt, der nach draußen zeigt, zählt als draußen.

1. **Gesperrt, ohne Rückfrage abgelehnt:** Konfiguration und Daten von hearth (`~/.hearth`, `%LOCALAPPDATA%\hearth`, `C:\ProgramData\hearth`, der Programmordner), Gerätepfade (`\\.\`, `\\?\`, `CON`, `NUL` …), UNC- und Netzpfade (Windows meldet sich dort sonst mit dem NTLM-Hash an), alternative Datenströme (`datei:strom`).
2. **Im Projektordner, frei:** lesen, auflisten, suchen, schreiben, editieren.
3. **Im Projektordner, Schreiben nur mit Rückfrage** (alles, was später ohne Zutun Code ausführt oder Rechte vergibt; dieselben Regeln wie die Merge-Prüfung in `review.rs`): `.git/`, Hook-Ordner (`.husky/`, `.githooks/` …), `.gitattributes`, `.gitmodules`, `.lfsconfig`, `.vscode/`, `.idea/`, `.claude/`, `.cursor/`, `.pi/`, `.mcp.json`, `.envrc`, `desktop.ini`, `autorun.inf`, `*.lnk`, `*.url`, `*.scf`, `*.library-ms`.
4. **Im Projektordner, Lesen nur mit Rückfrage** (Geheimnisse, die sonst an Server und Anbieter gingen): `.env` und `.env.*` außer `.env.example`/`.env.sample`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc`, `.netrc`.
5. **Außerhalb des Projektordners:** jeder Zugriff mit Rückfrage. Lesen lässt sich für eine Datei oder einen Ordner „für diesen Chat“ freigeben, Schreiben nur einzeln.

Optional je Projekt: `[host] writes = "ask"` lässt auch jedes Schreiben im Projektordner fragen (Standard `"free"`, Wunsch des Nutzers).

Grenze: Zwischen Prüfung und Zugriff könnte ein Link umgebogen werden. Das kann nur ein Prozess, der parallel läuft, also nur ein freigegebener Befehl; für den gilt ohnehin Abschnitt 2.

## 5. Rückfragen

- Eine Rückfrage zeigt Projekt, Chat, Werkzeug, den aufgelösten Pfad bzw. den Befehl mit Arbeitsordner und Shell. Antworten: **Einmal**, **In diesem Chat** (bei `shell` nur genau dieser Befehlstext, bei Lesen draußen dieser Ordner), **Ablehnen**. Kein „immer“ über den Chat hinaus.
- Oberflächen: Desktop-App (Fenster oder Benachrichtigung, die das Fenster öffnet), `hearth host watch` im Terminal, und nur mit Opt-in der Web-Chat. Die erste Antwort gilt.
- Ohne Antwort nach 10 Minuten gilt Ablehnen. Stopp im Chat bricht offene Fragen und laufende Befehle ab.
- Jeder Aufruf und jede Entscheidung landet im Protokoll `~/.hearth/logs/host-<projekt>.jsonl` (Werkzeug, Pfad oder Befehl, Entscheidung, wer entschied, Exit-Code). Inhalte von Dateien stehen dort nicht.

## 6. Was gleich bleibt

- Sandbox-Modus mit allen bisherigen Regeln (Gateway, Merge-Prüfung, App-Katalog, Freigaben am Rechner).
- Geräte-Token nur für `/api/device` und `/api/llm`, Kopplung, Umzug, Grenzen gegen einen feindlichen Server (Client-Prüfung vom 10.11.).
- Werkzeugausgaben laufen im Server durch die Prompt-Injection-Muster (`agentWarning`) und auf Wunsch durch Jev, wie bei pi.

## 7. Plattformen

- **Direktmodus:** Windows, Linux, macOS aus einer Codebasis (`hearth-core/src/host/`), ohne VM und ohne Node.
- **Sandbox:** Windows mit Hyper-V-VM (wie bisher), macOS mit Podman Machine, Linux mit rootless Podman ohne VM (Container teilen sich den Kernel des Rechners; gVisor, falls installiert). Unter Linux ist die Sandbox also schwächer als unter Windows und macOS; die Desktop-App sagt das.
- **Desktop-App** (`hearth-app`, egui) für alle drei, dazu das Tray-Symbol (Windows wie bisher, macOS Menüleiste, Linux StatusNotifierItem) und der Hintergrunddienst.
