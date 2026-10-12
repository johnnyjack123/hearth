# Phase 0b – Windows Sandbox (abgebrochen)

> 12.10.2026, Windows 11 Pro ARM64 (Build 26200). Ergebnis: **als Provider verworfen**, siehe ARCHITECTURE.md §3.

## Befunde

- **CLI `wsb`:** Sie ist im Feature `Containers-DisposableClientVM` nicht enthalten. Windows liefert nur `WindowsSandbox.exe` (Build 26100.8875). Die CLI kommt mit der Store-Version, die die Sandbox beim **ersten Start** nachlädt.
- **`wsb exec` liefert keine Ausgabe**, nur den Exit-Code (laut Microsoft-Doku). Mit `-r ExistingLogin` braucht es eine aktive Desktop-Sitzung (`wsb connect`), mit `-r System` nicht. Für jede Interaktion wäre ein eigener Agent-Prozess in der Sandbox nötig, der sich übers Netz meldet (`wsb ip`).
- **RAM:** Die Sandbox bootet ein vollständiges Windows mit Desktop, Explorer, Diensten und Defender. Einen Modus ohne Oberfläche gibt es nicht. `MemoryInMB` (Standard 4096) lässt sich nur bis zu einer von Windows erzwungenen Untergrenze senken. Geschätzt 1,5–2,5 GB pro Instanz, **nicht gemessen** (siehe nächster Punkt).
- **Absturz beim ersten Start:** Bluescreen `0xA0` (INTERNAL_POWER_ERROR, Parameter `0x618`), Abbild `C:\Windows\Minidump\101226-11343-01.dmp`. Kein Speichermangel: Commit 14/31 GB, Auslagerungsdatei kaum genutzt. Vermutlich ein Fehler der Energieverwaltung (Treiber/Firmware) beim Start der Virtualisierung. Möglicherweise hängt das mit dem Abbruch der WSL-VM beim Standby in Phase 0 zusammen. Nicht weiter analysiert.
- **Nicht getestet:** parallele Instanzen, Geschwindigkeit von Ordnerfreigaben, Netzwerk.

## Warum verworfen

Auf einem Rechner mit typischerweise 2,5–3 GB freiem RAM ist mehr als eine Instanz unrealistisch. Die Linux-VM mit Containern braucht für beliebig viele Projekte 0,4–1 GB (Phase 0, §12). Die Vorteile der Sandbox wiegen das nicht auf: kaum zusätzlicher Plattenplatz, Windows-Programme, NTFS-native Freigaben.

Sie könnte später wieder interessant werden, etwa als Provider für Windows-spezifische Builds auf Rechnern mit mehr RAM.
