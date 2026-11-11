# Windows-Client einrichten und nutzen

Der Windows-Client bringt Coding-Agents in eine isolierte Umgebung auf dem eigenen Rechner. Er besteht aus vier Programmen, die das Einrichtungsprogramm gemeinsam installiert:

| Programm | Aufgabe |
|---|---|
| `hearth.exe` | Befehlszeile: Projekte, Agent im Terminal, Sync, Prüfung, Einrichtung |
| `hearthd.exe` | Tray-Symbol; startet bei der Anmeldung den Hintergrunddienst für Web-Chats |
| `hearth-vmd.exe` | Windows-Dienst, der die VM startet und stoppt (der einzige Teil mit Administratorrechten) |
| Gateway | läuft in der VM, regelt jeden Netzzugriff der Sandbox |

Ohne Client funktioniert der Web-Chat ganz normal, nur ohne Agents auf dem Rechner.

## Voraussetzungen

- Windows 11 Pro, Enterprise oder Education, x64 oder ARM64. Windows Home hat kein Hyper-V und wird nicht unterstützt.
- Virtualisierung in der Firmware eingeschaltet (Intel VT-x bzw. AMD-V; auf ARM-Geräten meist ab Werk an).
- 16 GB RAM empfohlen. Die VM nimmt sich beim Start, was Windows frei hat, abzüglich 1 GB.
- Rund 10 GB freier Platz für die VM-Festplatte.
- Ein laufender Hearth-Server und ein Konto darauf ([`deploy.md`](deploy.md)).

## Installation

1. Aus dem [neuesten Release](https://github.com/johnnyjack123/hearth/releases/latest) `hearth-setup-x64.exe` bzw. `hearth-setup-arm64.exe` herunterladen und starten.
2. SmartScreen warnt beim ersten Start, weil die Programme nicht signiert sind: „Weitere Informationen“ → „Trotzdem ausführen“.
3. Das Einrichtungsprogramm legt die Programme unter `%LOCALAPPDATA%\Programs\hearth` ab und startet `hearth setup`. Das prüft der Reihe nach und fragt vor jedem größeren Schritt:
   - **Hyper-V:** schaltet es bei Bedarf ein (Administratorrechte, danach ist ein Neustart nötig; das Einrichtungsprogramm danach noch einmal starten).
   - **Git und Podman:** installiert sie über `winget`.
   - **VM-Dienst und VM:** richtet den Dienst `hearth-vmd` ein und legt die VM `hearth` an (2–4 CPUs, bis 4 GB RAM, 40 GB Festplatte, die nur so weit wächst wie nötig). Der Download der VM dauert einige Minuten.
   - **Images:** baut das Basis-Image der Sandbox und das Gateway in der VM.
   - **Server:** fragt nach der Adresse des Servers und öffnet die Bestätigung im Browser (siehe Kopplung).
   - **Autostart und PATH:** der Hintergrunddienst startet ab jetzt bei der Anmeldung, `hearth` geht in jedem neuen Terminal.
4. Am Ende `hearth doctor` ausführen; es zeigt, ob alles bereit ist.

Bricht ein Schritt ab, das Einrichtungsprogramm einfach erneut starten. Es macht nur, was noch fehlt.

**Microsoft Defender** hält die unsignierte `hearth.exe` gelegentlich für Schadsoftware und löscht sie kurz nach dem Schreiben. Das Einrichtungsprogramm erkennt das und erklärt den Weg zurück: Windows-Sicherheit → Viren- & Bedrohungsschutz → Schutzverlauf → Eintrag öffnen → „Aktionen“ → „Wiederherstellen“. Dauerhaft hilft eine Meldung als Fehlalarm bei Microsoft.

## Kopplung mit dem Server

```powershell
hearth login https://hearth.example.org
```

Der Befehl zeigt einen Code und öffnet die Bestätigungsseite der Web-App. Dort steht, von welcher Adresse die Anfrage kommt; stimmt sie nicht mit der des Browsers überein, warnt die Seite. Nur bestätigen, wenn man den Befehl gerade selbst ausgeführt hat. Danach kann der Rechner über den Server Modelle mit den eigenen Keys nutzen. Die Keys selbst sieht er nie.

- `hearth logout` widerruft die Kopplung sofort.
- Bekommt der Server eine neue Adresse (etwa hinter einem Reverse-Proxy), genügt `hearth login <neue Adresse>`. Der Client erkennt denselben Server, fragt einmal nach und behält das Gerät samt verknüpfter Chats. Die neue Adresse muss `PUBLIC_URL` des Servers sein oder in `TRUSTED_ORIGINS` stehen; sonst lehnt der Client den Umzug ab, weil eine fremde Adresse, die nur an den echten Server weiterleitet, so das Geräte-Token abgreifen könnte.
- Einstellungen → Geräte in der Web-App zeigt alle gekoppelten Rechner, ihre Version und den Zustand der VM.

## Projekte

Ein Projekt ist ein Ordner mit Git-Repository, in dem ein Agent arbeiten darf.

```powershell
cd C:\Projekte\mein-projekt
hearth init                  # Projekt anlegen (bietet git init an), für Web-Chats freigeben, beim Server melden
hearth init . anderer-name   # mit eigenem Namen statt des Ordnernamens
hearth project list          # alle Projekte
hearth project remote off    # nicht mehr für Web-Chats anbieten (on: wieder anbieten)
hearth project remove        # Projekt vergessen und seine Daten in der VM löschen
```

Der Agent arbeitet nie direkt im Ordner, sondern auf einer Kopie in der VM. Dorthin kommt nur der ausgecheckte Branch (mit den Tags in seinem Verlauf), andere Branches bleiben auf dem Rechner. Was der Agent committet, kommt erst nach einer Prüfung zurück (siehe unten).

Web-Chats sehen nur Projekte, die mit `hearth init` angelegt oder mit `hearth project remote on` freigegeben wurden. Projekte, die anders entstanden sind (`hearth project add`, `hearth` in einem neuen Ordner, ältere Versionen ohne `[remote]`-Eintrag), bleiben für den Browser unsichtbar.

## Im Web-Chat

1. Im Chat auf das Zahnrad → **Umgebung** → Rechner und Projekt wählen.
2. Die erste Nachricht startet VM, Gateway und Sandbox, falls nötig. Jeder Schritt erscheint im Chat.
3. Will der Agent einen Host erreichen, den die Regeln des Projekts nicht erlauben, fragt der Chat nach: einmal, für 15 Minuten, bis zum Stopp der Sandbox oder immer.
4. **Fortschritt sichern** im Umgebungs-Menü holt die Arbeit des Agents und prüft sie. Übernommen wird sie erst nach Freigabe am Rechner: `hearth merge --approve <ID>` im Terminal (die ID steht im Chat, das Tray-Symbol zeigt wartende Freigaben).
5. Wer das nicht jedes Mal will, erlaubt es pro Projekt am Rechner: `hearth project remote on --merge` (oder Ja bei der Frage in `hearth init`). Dann übernimmt „Fortschritt sichern“ direkt in den ausgecheckten Branch, und **Autosync** tut es nach jeder Antwort. Ohne diese Erlaubnis bleibt Autosync aus.
6. Findet die Prüfung etwas Mittleres oder Schweres (etwa Build-Skripte, CI-Dateien, Editor-Einstellungen, Anweisungen für KI-Werkzeuge, Git-Hooks), hält sie in jedem Fall an; freigeben geht dann ebenfalls nur mit `hearth merge --approve <ID>`.

Die VM stoppt nach 30 Minuten ohne Agent und ohne `hearth`-Befehl im Terminal (einstellbar unter Einstellungen → Geräte, auch „nie“). Die nächste Nachricht startet sie wieder.

## Im Terminal

```powershell
hearth                 # Standard-Agent (pi) im Projekt des aktuellen Ordners
hearth run pi -- …     # bestimmter Agent mit eigenen Argumenten
hearth shell           # Shell in der Sandbox
hearth sync            # Commits des Agents holen (Branch agent/<projekt>)
hearth review          # Prüfung ansehen, ohne zu übernehmen
hearth merge           # prüfen und in den ausgecheckten Branch übernehmen
hearth status          # was läuft
hearth stop            # Sandbox des Projekts stoppen
```

Rückfragen des Gateways erscheinen im selben Fenster. In der ersten Sekunde zählt keine Taste, damit nichts aus Versehen beantwortet wird; „immer“ braucht einen zweiten Tastendruck.

## Netz der Sandbox

Jedes Projekt hat sein eigenes Gateway. Es lässt nur öffentliche Adressen zu, nie das Heimnetz oder den Rechner selbst.

```powershell
hearth net                    # Regeln und Zustand
hearth net allow pypi.org     # Host erlauben (--always: dauerhaft)
hearth net deny example.com
hearth net preset open        # Voreinstellung wechseln
hearth net watch              # Live-Protokoll, beantwortet Rückfragen
```

## Programme in der Sandbox

C/C++ ist ab Werk an. Weitere Werkzeuge kommen aus dem Katalog und werden je Projekt eingeschaltet:

```powershell
hearth app list
hearth app on rust go
hearth app off java
```

Dasselbe geht im Browser unter Einstellungen → Geräte. Dort lassen sich auch eigene Programme anlegen (apt-, npm- oder pip-Pakete oder ein Skript). Programmdateien vom Rechner (ein Linux-Programm, AppImage, `.deb` oder Archiv mit Endung `.zip`, `.tar.gz`, `.tgz`, `.tar.xz` oder `.txz`) fragt der Browser nur an; freigeben geht am Rechner über das Tray-Symbol oder mit `hearth app approve <ID>`. Der Pfad muss auf einem Laufwerk des Rechners liegen, Netzwerkpfade und Netzlaufwerke nimmt der Rechner nicht an; Office- und OpenDocument-Dateien gelten nie als Archiv.

## VM und Speicherplatz

```powershell
hearth vmd status
hearth vmd stop
hearth vmd clean        # alte Image-Stände und Reste gelöschter Projekte entfernen, Festplatte verkleinern
```

Bevor `vmd clean` oder `project remove` Projektdaten löscht, sichert es nicht übernommene Arbeit des Agents als Git-Bundle unter `%USERPROFILE%\.hearth\gesichert\`.

## Updates

Das neue Einrichtungsprogramm herunterladen und starten. Es hält den Hintergrunddienst an, ersetzt die Programme und richtet nach, was sich geändert hat. Laufende Agents vorher fertig werden lassen. Server und Client verhandeln ihre Protokollversion, deshalb ist die Reihenfolge der Updates meist egal; die Web-App und das Tray-Menü sagen, wenn eine Seite zu alt ist.

## Umstieg von LLM Harness (bis 0.1.x)

Mit 0.2.0 heißt das Projekt Hearth; Programme, Ordner, VM und Dienst haben neue Namen. Zuerst den Server auf 0.2.0 bringen ([`deploy.md`](deploy.md#umstieg-von-llm-harness-bis-01x)), dann:

1. Nicht übernommene Arbeit der Agents mit `harness merge` sichern (die alte VM wird nicht übernommen).
2. Das neue Einrichtungsprogramm ausführen. Beim ersten Start übernimmt `hearth`
   - Projekte, eigene Programme und Einstellungen aus `%USERPROFILE%\.harness` nach `%USERPROFILE%\.hearth` (der alte Ordner bleibt unverändert),
   - das Geräte-Token, die Kopplung bleibt also bestehen,
   - und es beendet den alten Hintergrunddienst, entfernt seinen Autostart und den alten Eintrag im PATH.
3. Das Einrichtungsprogramm legt eine neue VM `hearth` und den Dienst `hearth-vmd` an. Danach die alte Installation entfernen:
   ```powershell
   & "$env:LOCALAPPDATA\Programs\llm-harness\harness.exe" vmd uninstall   # alter VM-Dienst (fragt per UAC)
   # in einer Administrator-PowerShell:
   $env:CONTAINERS_MACHINE_PROVIDER = 'hyperv'
   & "C:\Program Files\RedHat\Podman\podman.exe" machine rm -f harness
   ```
   Anschließend die Ordner `%LOCALAPPDATA%\Programs\llm-harness`, `%LOCALAPPDATA%\llm-harness` und `C:\ProgramData\llm-harness` löschen, und wenn alles läuft auch `%USERPROFILE%\.harness`.

## Probleme

| Meldung oder Zustand | Ursache und Abhilfe |
|---|---|
| Setup startet nicht, `hearth.exe` fehlt | Defender hat sie entfernt, siehe Installation |
| Hyper-V-Fehler 32788 oder 32778 beim Start der VM | zu wenig freier Arbeitsspeicher; Programme schließen und erneut versuchen |
| „expected pipe is not available“ nach dem Standby | ein alter `gvproxy` hält die Verbindung; in einer Administrator-PowerShell `Stop-Process -Name gvproxy -Force`, dann erneut starten |
| „pi wurde beendet“ bzw. Exit-Code 137 | der VM ging der Speicher aus; weniger parallel laufen lassen oder mehr RAM freigeben |
| Web-Chat findet den Rechner nicht | `hearth daemon status`; sonst `hearth daemon start` |
| Agent erreicht einen Host nicht | `hearth net watch` zeigt, was blockiert wurde und warum |

Protokolle: `%USERPROFILE%\.hearth\daemon.log` (Hintergrunddienst) und `C:\ProgramData\hearth\vmd\vmd.log` (VM-Dienst). `hearth doctor` prüft Konfiguration, VM, Verbindung und Images in einem Durchgang.

## Deinstallieren

```powershell
hearth daemon uninstall     # Hintergrunddienst und Autostart
hearth logout               # Kopplung widerrufen
hearth vmd uninstall        # VM-Dienst (fragt per UAC)
# in einer Administrator-PowerShell:
$env:CONTAINERS_MACHINE_PROVIDER = 'hyperv'
& "C:\Program Files\RedHat\Podman\podman.exe" machine rm -f hearth
```

Danach `%LOCALAPPDATA%\Programs\hearth`, `%LOCALAPPDATA%\hearth`, `C:\ProgramData\hearth` und `%USERPROFILE%\.hearth` löschen und den Eintrag aus dem PATH (Benutzervariablen) entfernen.
