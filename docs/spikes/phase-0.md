# Phase 0 – Spike: Sandbox-Grundlagen unter Windows

> Historischer Bericht: Pfade, Programm- und Paketnamen beziehen sich auf den Stand vor der Umbenennung von „LLM Harness“ in **Hearth** (0.2.0), z. B. `crates/harness-cli` statt `crates/hearth-cli`.

> Durchgeführt am 10./11.10.2026 auf einem Rechner (Windows 11 Pro ARM64, Podman 5.8 mit WSL2-Provider).
> Bezug: ARCHITECTURE.md, Abschnitte 3.1–3.5 und 9. Skripte liegen in `spikes/phase-0/`, Rohdaten in `spikes/phase-0/work/` (gitignored).

## Kurzfassung

- **Bind-Mount über WSL (drvfs/9p) ist für Quelltext-lastige Arbeit 30- bis 300-mal langsamer** als ein Container-Volume. `git status` im vite-Repo (2.866 Dateien) braucht 5,5 s statt 0,02 s (Volume) bzw. 0,17 s (Windows nativ). Ein `stat` kostet über 9p etwa 2 ms.
- **Der Hybrid (Projekt per Bind-Mount, `node_modules` auf einem Volume) reicht für Install/Build/Test.** `npm ci` dauert 4,6 s statt 70–80 s, Build und Tests liegen nur 15–20 % über dem reinen Volume. Schneller als Windows nativ ist das auch.
- **Alles, was den Quellbaum abläuft (`git status`, rg, Glob-Suchen), bleibt teuer:** etwa 1,3–1,9 ms pro Datei. Bei 30.000 Dateien sind das hochgerechnet 40–60 s pro `git status`. Für große Repos braucht es einen Fallback.
- **Windows-git und Linux-git am selben Index:** Ohne `core.checkStat=minimal` und `core.trustctime=false` hasht Linux-git nach jeder Windows-Nutzung alle Dateien neu (24 s statt 3,7 s). Mit read-only `.git` passiert das sogar bei **jedem** Aufruf. Diese Einstellungen müssen ins Base-Image.
- **inotify:** Änderungen von Windows kommen im Container nicht an, Polling funktioniert. In der Gegenrichtung sieht Windows (FileSystemWatcher) Container-Schreibzugriffe sofort.
- **Netz:** Ein `--internal`-Netz hat weder Route noch externes DNS. Das Proxy-Gateway mit Domain-Allowlist funktioniert (npm, git über `HTTPS_PROXY`). **Aber:** Aus jedem normalen Netz, also auch aus dem Gateway, sind der Windows-Host über seine LAN-IP, der Router, das Tailnet (100.x) und der WSL-DNS erreichbar. Das Gateway muss deshalb Ziel-IPs nach der eigenen DNS-Auflösung prüfen.
- **WSL-Provider und Sicherheit:** Die VM sieht `C:\` komplett mit Schreibrechten, WSL-Interop ist aktiv (die VM kann `whoami.exe` auf Windows starten), und `sudo` geht ohne Passwort. Ein Ausbruch aus dem Container in die VM bedeutet damit Codeausführung als Windows-Benutzer.
- **Postgres-Tests des Servers:** 46/46 bestanden, davon 16 gegen Postgres. Hinweis: `-p 55432:5432` war unter Windows nur über `[::1]` erreichbar, erst `-p 127.0.0.1:55432:5432` funktionierte.
- **Hyper-V:** Das Feature ist deaktiviert und wurde nicht aktiviert. Ein `hyperv`-Disk-Image für aarch64 existiert in `quay.io/podman/machine-os:5.8`. Ob es zur Laufzeit auf ARM64 funktioniert, ist ungetestet. Freigaben lassen sich nach `machine init` nicht ändern (`podman machine set` hat kein `--volume`). **Nachtest mit aktiviertem Hyper-V: siehe Abschnitt 12.**
- **Tauri-RAM:** übersprungen. Visuelle Tests übernimmt der Nutzer. Der Release-Build ist zudem an rustc-Abstürzen gescheitert, vermutlich weil die Commit-Grenze des Hosts erreicht war (siehe 9).

## 1. Setup

| | |
|---|---|
| Hardware | ARM64-Rechner, Netzbetrieb |
| Windows | 11 Pro 10.0.26200, ARM64. Defender-Echtzeitschutz aktiv. Hyper-V-Feature deaktiviert |
| WSL | 2.7.14.0, Kernel 6.18.33.2-2, NAT-Netzwerkmodus, keine `.wslconfig` |
| Podman (Client) | 5.8.3 windows/arm64 |
| Podman (Server) | 5.8.8 linux/arm64, rootless, cgroup v2, overlay, netavark 1.17.2 (`firewall_driver="iptables"`) |
| Machine | `harness-spike`, WSL-Provider, Fedora 44 (Container Image), Benutzer `user` (UID 1000) |
| Testimage | `docker.io/library/node:24-bookworm` (Node 24.21, npm 11.19, git 2.39.5, arm64) |

Konfiguriert sind in `podman machine` 4 CPUs und 2 GiB. **Der WSL-Provider ignoriert das:** In der VM stehen 12 vCPUs und 7,7 GB (50 % des Host-RAMs) zur Verfügung. Die Grenzen setzt WSL global über `.wslconfig`, und sie gelten für alle WSL-Distributionen gemeinsam.

### Stolperstein: `~/.ssh/known_hosts`

`podman.exe` brach mit `knownhosts: …\known_hosts:4: missing host pattern` ab. Die Datei des Nutzers enthält Zeilen ohne Host-Muster. Podmans Go-SSH-Client lehnt die ganze Datei ab, bevor er überhaupt eine Verbindung aufbaut. Die Datei wurde nicht angefasst. Als Workaround dient `spikes/phase-0/pm.sh`: Es setzt `USERPROFILE`/`HOME` auf ein isoliertes Verzeichnis, dessen `known_hosts` nur den Host-Key der Machine enthält (per `ssh-keyscan`), und übergibt `CONTAINER_HOST`/`CONTAINER_SSHKEY` explizit.

**Konsequenz für `harnessd`:** Nicht auf die SSH-Konfiguration des Nutzers verlassen. Besser ist die libpod-REST-API über die Named Pipe (`\\.\pipe\podman-<machine>`), die `podman.exe` selbst nicht unterstützt (`npipe` ist kein unterstütztes Schema), die ein eigener Client aber ansprechen kann. Alternativ ein eigener SSH-Client mit gepinntem Host-Key.

## 2. Umgebung: Was die VM vom Host sieht

| Prüfung | Ergebnis |
|---|---|
| Automount | `C:\ on /mnt/c type 9p (rw,…,aname=drvfs;path=C:\;uid=1000;gid=1000…)`: **das ganze Laufwerk C:, beschreibbar** |
| Schreibzugriff aus der VM | `touch /mnt/c/Users/<nutzer>/…` funktioniert |
| WSL-Interop | `/proc/sys/fs/binfmt_misc/WSLInterop` aktiv (Interpreter `/init`, Flags `PF`). `/mnt/c/Windows/System32/whoami.exe` liefert aus der VM `<rechner>\<nutzer>` |
| Windows-`PATH` in der VM | wird angehängt (`appendWindowsPath` ist Standard) |
| `sudo` in der VM | ohne Passwort (`user` ist in `wheel`) |
| Gemeinsame VM | Alle WSL2-Distributionen (hier auch `Ubuntu`, `docker-desktop`) laufen in derselben Utility-VM mit demselben Kernel |
| `/etc/wsl.conf` | nur `[user] default=user`, kein Interop- oder Automount-Abschnitt |

**Interop aus dem Container:** Eine `whoami.exe` im Bind-Mount, im Container ausgeführt, startet tatsächlich den WSL-Handler `/init`. Durch das Flag `F` ist er schon bei der Registrierung geöffnet worden und läuft daher auch in fremden Mount-Namespaces. Er scheitert dann an `UtilBindVsockAnyPort: socket failed 1`, weil das Standard-Seccomp-Profil `AF_VSOCK` sperrt. Mit `--security-opt seccomp=unconfined` scheitert er etwas später an `UtilGetPpid … /proc/1/stat`. Heute kommt der Container also nicht bis Windows durch, der Schutz ist aber eher zufällig.

**Sicherheitsfolge:** Die Aussage in ARCHITECTURE.md 3.4 („Die VM sieht nur die freigegebenen Wurzelordner“) gilt für den WSL-Provider **nicht**. Wer aus dem Container in die VM ausbricht, hat Lese- und Schreibzugriff auf `C:\` und kann über Interop Windows-Programme als der angemeldete Benutzer starten. Gegenmaßnahmen für die eigene Machine (in Phase 2 testen): In `/etc/wsl.conf` `[interop] enabled=false`, `appendWindowsPath=false` und `[automount] enabled=false` setzen und nur die freigegebenen Wurzelordner per `mount -t drvfs 'C:\…' /mnt/…` einhängen. Weil root in der VM das rückgängig machen kann (passwortloses `sudo`), ist das Härtung, aber **keine Grenze**. Die eigentliche Grenze bleibt die Container-Isolation: rootless, Seccomp, `--cap-drop=ALL`, `no-new-privileges`. **Nie** `--privileged` oder `seccomp=unconfined` verwenden.

## 3. Ressourcenverbrauch im Leerlauf

Gemessen wurde die Summe von Working Set und Private Bytes aller `vmmem*`-Prozesse unter Windows (`measure-vmmem.ps1`, 10 Messungen im Abstand von 3 s). „used“ ist der Wert aus `free -m` in der VM.

| Zustand | vmmemWSL Working Set | Private Bytes | VM „used“ |
|---|---:|---:|---:|
| Machine läuft, 0 Container, keine Images | 606 MB | 981 MB | 554 MB |
| nach Pull von node (1,17 GB) + postgres + alpine, Page-Cache warm | **1.857 MB** | 2.839 MB | 559 MB |
| 0 Container, `drop_caches` + 45 s | 446 MB | 1.094 MB | 564 MB |
| 1 Container im Leerlauf (`alpine sleep infinity`) | 520 MB | 1.058 MB | 570 MB |
| 3 Container im Leerlauf | 533 MB | 1.046 MB | 592 MB |
| nach allen Benchmarks, `harness-pg` läuft (Cache warm) | 999 MB | 1.531 MB | 622 MB |
| dasselbe nach `drop_caches` + 45 s | 656 MB | 1.160 MB | 625 MB |

- Pro Container im Leerlauf kommen in der VM etwa **13 MB** hinzu: `conmon` ca. 2 MB und `pasta` ca. 12 MB (Standard-Rootless-Netz). Der Podman-API-Dienst selbst belegt etwa 70 MB RSS.
- CPU im Leerlauf (20 s Stichprobe aus `/proc/stat`, mit `harness-pg`): 0,0 %, Load 0,00.
- **Der Page-Cache bläht vmmemWSL stark auf** (+1,4 GB nach den Image-Pulls). WSL gibt den Speicher erst nach `drop_caches` zurück, dann aber innerhalb von Sekunden (Free Page Reporting). `harnessd` könnte nach großen Operationen (Pull, Build) in der eigenen Machine `drop_caches` auslösen. `autoMemoryReclaim` in `.wslconfig` wäre die Alternative, gilt aber global für alle Distributionen.
- Während des ersten `npm install`/Build-Laufs meldete Windows `Resource-Exhaustion-Detector 2004` („virtueller Speicher unzureichend“, vmmemWSL 4,2 GB virtuell). Der Host hatte insgesamt wenig freien Commit (siehe 9).

## 4. Datei-I/O

### 4.1 Methodik

Jeder Ort bekommt einen eigenen Container (`node:24-bookworm`, `--userns=keep-id --user 1000:1000`). Alle Schritte laufen per `podman exec`, die Zeit wird **im Container** gemessen, ohne SSH-Overhead (`bench-run.sh` → `bench-step.sh`).

| Ort | Mount |
|---|---|
| **bind** | `-v C:\…\spikes\phase-0\work\bind:/bench` (Podman übersetzt nach `/mnt/c/…`, also drvfs/9p) |
| **volume** | `-v bench-vol:/bench:U` (benanntes Volume, ext4 in der VM) |
| **containerfs** | `/home/node/bench` im Container-Dateisystem (overlay) |
| **hybrid** | wie bind, zusätzlich `-v bench-nm:/bench/app/node_modules:U` |
| Windows nativ (Referenz) | dieselben Schritte mit Windows-Node/-Git auf NTFS (`work/native`), Defender aktiv |

Workloads:

- **Kleine Dateien** (`smallfiles.mjs`): 5.000 Dateien zu je 1 KiB in 50 Ordnern anlegen, lesen, `stat`, löschen. Synchrone fs-Aufrufe, Median aus 3 Läufen.
- **git**: `git clone --depth 1` von vitejs/vite (2.866 Dateien), einmal über das Netz und einmal lokal aus einem Bare-Mirror auf einem Volume (reine Checkout-I/O). Danach `git status --porcelain` kalt (nach `drop_caches` in der VM; der Windows-Cache lässt sich ohne Admin nicht leeren) und warm (Median aus 5).
- **npm**: create-vite `template-react-ts`, erweitert um eine typische Toolchain (eslint, typescript-eslint, vitest, jsdom, testing-library, tailwind, prettier, react-router, react-query, zod; siehe `spikes/phase-0/app-overlay/package.json`). Ergibt 277 Pakete, 10.200 Dateien, 201 MB in `node_modules`. `npm ci --prefer-offline` mit warmem Cache auf einem Volume, also ohne Netz-Einfluss. Danach `npm run build` (`tsc -b && vite build`), `vitest run` (ein Komponententest mit jsdom) und `rm -rf node_modules`.

### 4.2 Ergebnisse (Wall-Clock in Sekunden)

| Workload | containerfs | volume | **bind** | **hybrid** | Windows nativ |
|---|---:|---:|---:|---:|---:|
| 5.000 Dateien anlegen | 0,15 | 0,10 | **17,4** | – | 1,17 |
| 5.000 Dateien lesen | 0,06 | 0,05 | **13,1** | – | 1,79 |
| 5.000 × `stat` | 0,03 | 0,02 | **10,0** | – | 0,14 |
| 5.000 Dateien löschen | 0,06 | 0,05 | **4,8** | – | 0,36 |
| `git clone --depth 1` vite (Netz) | 2,8 | 2,7 | **39,0** | – | 4,1 |
| `git clone --depth 1` vite (lokaler Mirror) | 0,72 | 0,70 | **31,9 / 40,6** | – | 1,58 |
| `rm -rf vite` | 0,08 | 0,06 | **8,0 / 8,9** | – | 0,69 |
| `git status` kalt | 0,26 | 0,18 | **6,5** | (wie bind) | 0,19 |
| `git status` warm (Median) | 0,025 | 0,018 | **5,5** | (wie bind) | 0,17 |
| `npm ci` (Cache warm), Lauf 1 / 2 | 4,7 / 4,1 | 4,8 / 4,4 | **80,1 / 70,7** | **4,9 / 4,6** | 5,8 / 5,8 |
| `npm run build`, Lauf 1 / 2 | 1,8 / 1,7 | 1,8 / 1,8 | **7,7 / 7,9** | **2,1 / 2,2** | 4,9 / 3,1 |
| `vitest run`, Lauf 1 / 2 | 1,35 / 1,31 | 1,33 / 1,32 | **23,9 / 24,0** | **1,55 / 1,49** | 28,8* / 3,3 |
| `rm -rf node_modules` | 0,33 | 0,29 | **16,3** | **0,25** | 1,9 |

\* Der erste Testlauf unter Windows ist vermutlich vom Defender-Scan neuer Binaries dominiert.

Zum Vergleich: `npm install` mit leerem Cache über das Netz dauerte im Container 26 s, unter Windows (`npm ci`) 13 s.

Gegenprobe ohne Container: `git status` **direkt in der VM** auf `/mnt/c` dauert ebenfalls 5,5 s, `find` + `stat` über alle Dateien 10,7 s. Die Kosten entstehen also in drvfs/9p, nicht in Podman, User-Namespace oder overlay.

### 4.3 `git status` auf dem Bind-Mount: Windows-git und Linux-git am selben Repo

`git-pingpong.sh` lässt Windows-git und Linux-git (in der VM, auf `/mnt/c`) abwechselnd `git status --porcelain -uno` ausführen. `--no-optional-locks` simuliert das geplante read-only `.git`, bei dem git den aufgefrischten Index nicht zurückschreiben kann.

| Schritt | Zeit |
|---|---:|
| Windows-git (Baseline) | 0,15 s |
| Linux-git, Standardkonfiguration, Index read-only, nachdem Windows den Index geschrieben hat, 1. / 2. Aufruf | **23,1 s / 25,0 s** |
| Linux-git mit `core.checkStat=minimal` + `core.trustctime=false`, Index read-only, 1. / 2. Aufruf | **3,7 s / 3,7 s** |
| Linux-git, Standardkonfiguration, darf Index schreiben, 1. / 2. Aufruf | 24,9 s / 3,7 s |
| Windows-git, nachdem Linux den Index geschrieben hat, 1. / 2. Aufruf | 0,44 s / 0,15 s |
| Linux-git minimal-stat, nachdem Windows erneut geschrieben hat | 3,6 s |

Ursache: Git für Windows speichert andere Stat-Daten (Inode, Device, UID, ctime) als Linux-git über 9p sieht. Linux-git hält deshalb jede Datei für möglicherweise geändert und vergleicht den Inhalt. Mit read-only `.git` passiert das bei jedem Aufruf. **`core.checkStat=minimal` und `core.trustctime=false` gehören in die `/etc/gitconfig` des Base-Images.** Sie ändern nichts auf dem Host.

Weitere Stellschrauben (Linux-git auf 9p, vite):

| Variante | Zeit |
|---|---:|
| `git status` (mit untracked) | 5,5 s |
| `git status -uno` (nur getrackte Dateien) | 3,7 s |
| `core.untrackedCache=true` | 6,1 s (kein Gewinn) |
| `core.preloadIndex=false` (seriell) | 10,8 s |
| `index.threads=1` vs `8` | 3,6 s vs 3,6 s |

Das parallele `lstat` (`preloadIndex`, Standard) hilft also schon um den Faktor 3. `fsmonitor` scheidet aus, weil inotify nicht funktioniert (Abschnitt 5).

### 4.4 Beobachtungen

- Pro Datei kosten Metadaten-Zugriffe über drvfs/9p hier **etwa 2 ms** (Windows nativ: 0,03 ms). Anlegen kostet etwa 3,5 ms pro Datei.
- **Hochrechnung** für `git status` auf dem Bind-Mount: etwa 1,3 ms (`-uno`) bis 1,9 ms pro getrackter Datei, also bei 10.000 Dateien 13–19 s und bei 50.000 Dateien 65–95 s. Nicht gemessen, aber die Werte für `smallfiles stat`, `find` und `git` liegen konsistent in dieser Größenordnung.
- Agents rufen `git status`, `rg` und Glob-Suchen ständig auf. Ab einigen tausend Dateien bremst der Bind-Mount spürbar, und zwar auch dann, wenn die Abhängigkeiten auf Volumes liegen.
- Der Hybrid funktioniert ohne Sonderbehandlung: Podman legt den Mountpunkt `node_modules` auf Windows-Seite als leeren Ordner an. Windows-Werkzeuge sehen dort also ein leeres `node_modules`, und die Linux-Binaries kollidieren nicht mit dem Host.
- Container-Volume und Container-Dateisystem sind gleichauf und bei diesem Workload **schneller als Windows nativ** (Defender).

## 5. Dateiänderungs-Benachrichtigungen

`watch.mjs` läuft im Container auf dem Bind-Mount (`fs.watch`, also inotify, und parallel ein Polling alle 500 ms).

| Änderung | `fs.watch` (inotify) im Container | Polling im Container |
|---|---|---|
| Datei unter Windows angelegt (Git Bash) | – (kein Ereignis) | erkannt |
| Datei unter Windows geändert | – | erkannt |
| Datei per PowerShell `Set-Content` angelegt | – | erkannt |
| Datei im Container angelegt | `rename` + `change` | erkannt |

In der Gegenrichtung (`watch-host.ps1`, `FileSystemWatcher` unter Windows) liefern Schreibzugriffe im Container `Created`, `Changed` und `Renamed` sofort. IDEs auf dem Host sehen Änderungen des Agents also live.

**Bestätigt:** Dev-Server mit Hot Reload brauchen im Container Polling (`CHOKIDAR_USEPOLLING=1`, `WATCHPACK_POLLING=true`, Vite `server.watch.usePolling`). Polling auf 9p ist teuer (ein `stat` pro Datei und Intervall, siehe 4.4). Bei großen Projekten sollten nur die Quellordner und nicht `node_modules` gepollt werden.

## 6. Netzwerk-Isolation

### 6.1 Befehle

```sh
# Netze
podman network create --internal harness-int-test      # 10.89.0.0/24, keine Route nach außen
podman network create harness-ext-test                  # 10.89.1.0/24, normales Bridge-Netz

# Gateway: tinyproxy auf Alpine, Domain-Allowlist (spikes/phase-0/proxy/)
podman build -t localhost/harness-spike-proxy:latest spikes/phase-0/proxy
podman run -d --name egress-proxy --network harness-int-test --network harness-ext-test \
  localhost/harness-spike-proxy:latest
#   Allowlist (ERE, gegen Host bzw. CONNECT-Host):  ^registry\.npmjs\.org$   ^github\.com$
#   tinyproxy.conf: FilterDefaultDeny Yes, FilterURLs Off, FilterType ere, ConnectPort 443

# Client im internen Netz
podman run --rm --network harness-int-test -v <spike>:/scripts:ro node:24-bookworm \
  bash /scripts/net-tests.sh internal http://egress-proxy:8888
# Client im normalen Netz bzw. im Rootless-Standardnetz (pasta)
podman run --rm --network harness-ext-test … net-tests.sh bridge
podman run --rm … net-tests.sh pasta-default
```

Das Gateway bekommt eine Default-Route nur über das externe Netz (`default via 10.89.1.1 dev eth0`) und hat `ip_forward=0`. Es routet also nicht zwischen den Netzen.

### 6.2 Erreichbarkeit

TCP-Connect mit 4 s Timeout. „offen“ heißt, die Verbindung kam zustande.

| Ziel | internes Netz | Bridge-Netz | Rootless-Standard (pasta) |
|---|---|---|---|
| DNS `github.com` | **nicht auflösbar** | ok | ok |
| DNS Containername (`egress-proxy`) | ok (`10.89.0.2 egress-proxy.dns.podman`) | ok | – |
| `https://github.com` direkt | scheitert (DNS) | 200 | 200 |
| `1.1.1.1:443` direkt | blockiert (keine Route) | offen | offen |
| `host.containers.internal` (→ `169.254.1.2`) :22 | blockiert | abgelehnt | abgelehnt |
| Windows-Host über WSL-vEthernet `172.17.0.1` (:135, :139, :445, :1234) | blockiert | Timeout (Hyper-V-Firewall) | Timeout |
| **Windows-Host über LAN-IP `192.168.1.43:445`** | blockiert | **offen** | **offen** |
| VM `eth0` `172.17.10.117:22` | blockiert | abgelehnt | abgelehnt |
| **WSL-DNS `10.255.255.254:53`** | blockiert | **offen** | **offen** |
| **LAN-Router `192.168.1.1` :80 / :53** | blockiert | **offen** | **offen** |
| **Tailscale `100.100.100.100:53`** | blockiert | **offen** | **offen** |
| Cloud-Metadaten `169.254.169.254:80` | blockiert | Timeout | Timeout |

Dienste, die unter Windows nur auf `127.0.0.1` lauschen (z. B. Vite :5173, Syncthing-GUI :8384), sind im NAT-Modus von WSL aus der VM nicht erreichbar. Im Mirrored-Modus wäre das anders, dort muss neu geprüft werden.

### 6.3 Gateway mit Allowlist (Client im internen Netz)

| Anfrage über `http://egress-proxy:8888` | Ergebnis |
|---|---|
| `https://registry.npmjs.org/react` | 200 |
| `https://github.com` | 200 |
| `https://example.com` | 403 (CONNECT abgelehnt) |
| `https://api.github.com` (Subdomain nicht freigegeben) | 403 |
| `http://example.com` (plain HTTP) | 403 |
| `https://1.1.1.1` (IP-Literal) | 403 |
| `http://172.17.0.1:1234`, `http://192.168.1.1` | 403 |
| `npm view react version` mit `HTTPS_PROXY` | `19.3.0` |
| `git ls-remote https://github.com/vitejs/vite` mit `HTTPS_PROXY` | ok |
| `git ls-remote https://gitlab.com/…` | `CONNECT tunnel failed` (gesperrt) |

### 6.4 Gegenprobe: Gateway ohne IP-Prüfung („Offen“-Modus)

Derselbe tinyproxy mit `FilterDefaultDeny No`, also so wie die geplante Voreinstellung „Offen“, wenn nur nach Domains gefiltert wird:

| Anfrage aus dem internen Netz über das Gateway | Ergebnis |
|---|---|
| `http://192.168.1.1/` (Router-Oberfläche) | **200** |
| `http://fritz.box/` (LAN-DNS-Name) | **301** |
| `http://192.168.1.43:1234/` (Node-Dienst auf dem Windows-Host) | **404**, Dienst erreicht |
| `http://100.100.100.100/` (Tailscale) | **200** |
| `http://169.254.169.254/` | Timeout |

Das Gateway ist also selbst der Weg ins LAN, zum Host und ins Tailnet. „Immer gesperrt“ (ARCHITECTURE.md 3.5) lässt sich nur durchsetzen, wenn das Gateway die **aufgelösten** Ziel-IPs prüft.

### 6.5 Isolation zwischen Instanzen

| Test | Ergebnis |
|---|---|
| Client → anderer Container im **selben** internen Netz (`peer:9000`) | **erreichbar** |
| Client → Container in einem **anderen** internen Netz (`10.89.2.2:9000`) | nicht erreichbar, Name nicht auflösbar |
| Client → derselbe Container über das Gateway (das in beiden Netzen hängt) | 403 (Allowlist). Bei „Offen“ ohne IP-Prüfung wäre er erreichbar |

Das bestätigt den Entwurf: **ein internes Netz pro Instanz**. Zusätzlich muss das Gateway die Container-Subnetze (`10.88.0.0/16`, `10.89.0.0/16` …) sperren.

## 7. Hyper-V-Provider (nicht aktiviert)

**Zustand auf diesem Rechner** (ohne Adminrechte über `Win32_OptionalFeature` abgefragt):

| Feature | Zustand |
|---|---|
| `Microsoft-Hyper-V-All` / `-Hypervisor` / `-Services` / `-Management-*` | Disabled (also auf dieser SKU verfügbar) |
| `VirtualMachinePlatform`, `Microsoft-Windows-Subsystem-Linux` | Enabled |
| `HypervisorPlatform` | Disabled |
| Dienst `vmms` | nicht vorhanden |
| Gruppe „Hyper-V-Administratoren“ (`S-1-5-32-578`) | existiert, keine Mitglieder |

`Get-WindowsOptionalFeature` selbst verlangt erhöhte Rechte.

**Schritte für den Nutzer** (einmalig, als Administrator):

```powershell
Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All   # danach Neustart
Add-LocalGroupMember -SID S-1-5-32-578 -Member $env:USERNAME               # danach ab- und wieder anmelden
```

Danach ohne Adminrechte:

```powershell
$env:CONTAINERS_MACHINE_PROVIDER = "hyperv"   # oder provider="hyperv" in containers.conf [machine]
podman machine init harness --cpus 4 -m 4096 -v "C:\Users\<user>\Projekte:/mnt/projekte"
```

**Was sich herausfinden ließ:**

- `quay.io/podman/machine-os:5.8` enthält ein Disk-Image mit `disktype: hyperv` für **aarch64** (neben applehv, qemu, wsl). Das Image für Hyper-V auf ARM64 wird also gebaut.
- Eine Dokumentation oder Erfahrungsberichte zum Hyper-V-Provider auf Windows ARM64 gibt es nicht. Die offizielle Doku nennt nur amd64. Ein Red-Hat-Epic für Podman auf Windows AArch64 existiert (RHEL-88520). Ein bekanntes ARM64-Problem betrifft den WSL-Provider (containers/podman#28341, SSH-Race beim Start).
- `podman machine set` kennt nur `--cpus`, `--disk-size`, `--memory`, `--rootful`, `--usb` und `--user-mode-networking`, **kein `--volume`**. Freigaben werden bei `init` festgelegt. Um sie zu ändern, muss die Machine neu angelegt werden (`rm` + `init`; Images und Volumes gehen dabei verloren). Ob sich die Mounts in der Machine-JSON unter `~/.config/containers/podman/machine/hyperv/` gefahrlos von Hand ändern lassen, ist **nicht geprüft** und auch nicht dokumentiert.
- Der Standard für `--volume` ist laut `podman machine init --help` auf diesem Rechner `C:\Users\<nutzer>:/Users/<nutzer>` und `C:\:/mnt/c`. `harnessd` muss die Freigaben also immer explizit setzen, sonst sieht auch eine Hyper-V-Machine ganz `C:\`. Beim WSL-Provider hat `--volume` laut Doku keine Wirkung.
- Unter Hyper-V laufen die Freigaben über Podmans eigenen 9p-Server per vsock. Die Performance kann anders (auch schlechter) ausfallen als drvfs und muss mit `bench-run.sh` neu gemessen werden.
- Podman 6.0.x ist erschienen (lokal 5.8.3) und wurde nicht getestet.

## 8. Postgres-Testlauf des Servers

```sh
podman run -d --name harness-pg -e POSTGRES_USER=harness -e POSTGRES_PASSWORD=harness \
  -e POSTGRES_DB=harness_test -p 127.0.0.1:55432:5432 docker.io/library/postgres:17-alpine
cd apps/server && TEST_POSTGRES_URL=postgres://harness:harness@127.0.0.1:55432/harness_test pnpm exec vitest run
```

Ergebnis (wörtlich):

```
 RUN  v5.0.3 C:/Users/<nutzer>/Documents/Programmieren/llm_harness/apps/server

 Test Files  2 passed (2)
      Tests  46 passed (46)
   Start at  23:52:05
   Duration  22.07s (import 56%, tests 42%, transform 2%)
```

- Mit `--reporter=verbose` sind alle 16 Tests unter „API on postgres“ grün (Accounts, Provider-Credentials, Conversations, Chat), dazu die 16 entsprechenden SQLite-Tests und 14 Unit-Tests. Ohne `TEST_POSTGRES_URL` laufen 30 Tests.
- Die Migrationen haben in `harness_test` 10 Tabellen angelegt (`account`, `conversation`, `kysely_migration`, `kysely_migration_lock`, `message`, `providerCredential`, `session`, `setting`, `user`, `verification`).
- **Abweichung vom vorgegebenen Befehl:** Mit `-p 55432:5432` lauschte `pasta` in der VM auf `*:55432` (IPv6-Dual-Stack). Das WSL-Port-Relay (`wslrelay`) leitete den Port dann **nur auf `[::1]:55432`** weiter, und `127.0.0.1:55432` wurde abgelehnt. Mit `-p 127.0.0.1:55432:5432` neu gestartet funktionierte `127.0.0.1`. Für Doku und `compose.yaml` unter Windows/Podman heißt das: Host-IP explizit angeben oder `localhost` verwenden.
- `harness-pg` läuft weiter.

## 9. Tauri-RAM (übersprungen)

Die visuellen Tests (Fenster offen/minimiert) übernimmt der Nutzer. Vorbereitet ist Folgendes:

- `spikes/phase-0/work/tauri-ram`: eine mit `create-tauri-app` 4.7.5 erzeugte Vanilla-Vorlage, mit `pnpm install --ignore-workspace` außerhalb des Workspaces installiert. Die Lockfile im Repo-Root bleibt unverändert.
- `spikes/phase-0/measure-app-ram.ps1 -Name tauri-ram -Label "Fenster offen"`: summiert Working Set und Private Bytes des App-Prozesses und aller Nachfahren (msedgewebview2.exe).

Der Release-Build (`pnpm tauri build --no-bundle`, danach `cargo build --release` mit `-j 12` und `-j 2`) ist **bei jedem Versuch fehlgeschlagen**, mit wechselnden rustc-Abstürzen: Internal Compiler Errors wie `no resolution for an import` bei `use std::env`, `only metadata stub found for rlib dependency core` und Exit-Code `0xc0000409`. Ein Hello-World-Build mit derselben Toolchain funktioniert. Zum selben Zeitpunkt waren von 31,6 GB Commit-Grenze nur noch 2,2 GB frei, und Windows hatte vorher zweimal „virtueller Speicher unzureichend“ gemeldet. **Vermutete Ursache:** Der Host hatte zu wenig Commit-Speicher. rustc bricht bei Allokationsfehlern unter Windows mit `0xc0000409` ab. Vorschlag: mit mehr freiem Speicher erneut bauen (`cargo build --release -j 2` in `src-tauri`), notfalls die Toolchain neu installieren.

## 10. Robustheit und weitere Stolpersteine

- **Standby beendet die VM:** Während eines Laufs war der Rechner etwa 2 h im Modern Standby (Deckel zu). Danach war die WSL-VM weg (`podman machine list`: „Never“ bzw. gestoppt), der laufende Container wurde abgebrochen und die Podman-API lieferte `EOF`. `podman machine start` funktionierte, meldete aber `could not start api proxy since expected pipe is not available`. Ein verwaister `win-sshproxy` aus der vorherigen Sitzung hielt `\\.\pipe\podman-harness-spike` weiter belegt (vgl. podman-container-tools/podman#29474). **`harnessd` braucht einen Watchdog:** Er muss den Machine-Zustand erkennen, die Machine neu starten, verwaiste `win-sshproxy`/`gvproxy` beenden und die Instanzen wieder hochfahren.
- **`known_hosts`:** siehe Abschnitt 1.
- **Ressourcen-Grenzen:** Der WSL-Provider ignoriert `--cpus`/`--memory` (siehe 1). Speicherlimits pro Container (`--memory`) sind deshalb wichtiger, damit ein Agent nicht die ganze WSL-VM und damit auch andere Distributionen ausbremst.
- **Port-Weiterleitung:** Nur `[::1]` bei veröffentlichten Ports ohne Host-IP (siehe 8).

## 11. Empfehlung / Entscheidung

### Ist der Direkt-Modus (Bind-Mount) unter Windows mit WSL-Provider tragfähig?

**Nur eingeschränkt.** Ein reiner Bind-Mount ist für Entwicklungsarbeit nicht brauchbar: `npm ci` 70–80 s statt 5 s, Tests 24 s statt 1,3 s. Mit dem Hybrid ist er für **kleine bis mittlere Repos** (Richtwert ≤ 5.000 getrackte Dateien, dort liegt `git status` bei 3–5 s) als Standard vertretbar. Für größere Repos ist er zu langsam, weil jede Operation über den Quellbaum linear mit etwa 1,3–2 ms pro Datei wächst.

Vorschlag für die Entscheidung:

1. **Direkt-Modus bleibt Standard**, aber nur zusammen mit:
   - Volumes für alle Abhängigkeits- und Build-Ordner (`node_modules`, `.venv`, `target`, `.pnpm-store`, `.next`, `dist` …). Die Erkennung erfolgt über Lockfiles und Manifeste.
   - `/etc/gitconfig` im Base-Image mit `core.checkStat=minimal` und `core.trustctime=false` (Pflicht, besonders bei read-only `.git`).
   - Polling-Variablen für Watcher (`CHOKIDAR_USEPOLLING`, `WATCHPACK_POLLING`).
   - einer Warnung in UI und CLI, wenn das Repo viele Dateien hat (z. B. > 5.000 getrackte Dateien laut `git ls-files | wc -l` auf dem Host), mit dem Angebot, in den Fallback-Modus zu wechseln.
2. **Fallback für große Repos: den Isoliert-Modus vorziehen** (Phase 2/3 statt 5), mit einem Clone oder Worktree auf einem Container-Volume. Er bringt Volume-Geschwindigkeit (`git status` 0,02 s) ohne Sync-Engine und beseitigt nebenbei das Problem, dass Windows- und Linux-git denselben Index nutzen. Ein bidirektionaler Sync-Modus (à la mutagen) ist deutlich aufwendiger und lohnt sich nur, wenn Nutzer bei großen Repos auf Live-Bearbeitung im eigenen Ordner bestehen.
3. **Erneut messen** mit dem Hyper-V-Provider (Podmans 9p-Server). Falls verfügbar, auch alternative Freigabewege (virtiofs) messen. Die Skripte sind dafür vorbereitet: `bench-run.sh bind volume hybrid`.

### Reicht der Hybrid (Abhängigkeitsordner auf Volumes)?

**Für Install, Build und Test ja:** `npm ci` 4,6–4,9 s (Volume 4,4–4,8, Windows nativ 5,8), Build 2,1 s (1,8), Tests 1,5 s (1,3). **Für git und Suchen über den Quellbaum nein:** Diese bleiben bei Bind-Mount-Geschwindigkeit (vite: `git status` 3,7 s mit `-uno`, sonst 5,5 s). Der Hybrid ist damit die Voraussetzung für den Direkt-Modus, löst das Kernproblem aber nicht für große Repos.

### Wie soll das Egress-Gateway gebaut werden?

Das Prinzip aus ARCHITECTURE.md 3.5 funktioniert: internes Netz ohne Route und ohne externes DNS, Gateway in beiden Netzen, Filterung über den CONNECT-Host ohne TLS-Aufbruch, npm und git über `HTTP(S)_PROXY`. Für den eigenen Rust-Proxy ergeben sich aus dem Spike diese Anforderungen:

1. **Das Gateway löst DNS selbst auf und prüft jede aufgelöste IP vor dem Verbindungsaufbau.** Gesperrt sind: `127.0.0.0/8`, `::1`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `100.64.0.0/10` (CGNAT/Tailscale, inkl. `100.100.100.100`), `169.254.0.0/16`, `fc00::/7`, `fe80::/10`, Multicast und Broadcast, die Podman-Subnetze sowie alle Adressen des Hosts. Das gilt auch bei „Offen“. Gemessen: Ohne diese Prüfung sind Router-Oberfläche, Windows-Dienste über die LAN-IP und das Tailnet erreichbar.
2. **Verbindung zur geprüften IP aufbauen** (kein zweites Auflösen), sonst ist DNS-Rebinding möglich. Das betrifft auch erlaubte Domains, die auf private IPs zeigen. IP-Literale wie Hostnamen behandeln.
3. **Ports beschränken:** CONNECT standardmäßig nur auf 443, git über SSH (22) nur auf ausdrückliche Freigabe. Plain-HTTP-Requests (absolute URI) ebenfalls filtern.
4. **Nur auf den internen Netzen lauschen**, nicht auf dem externen Interface. tinyproxy band im Spike an `0.0.0.0`.
5. **Ein internes Netz pro Instanz.** Im selben internen Netz sehen sich Container gegenseitig. Instanzen nur über das Gateway verbinden, das selbst nicht routet (`ip_forward=0`, bestätigt).
6. DNS im Instanz-Netz liefert nur Containernamen (aardvark löst keine externen Namen auf, bestätigt), es gibt also keinen DNS-Exfiltrationskanal. Optional `--disable-dns` für Instanz-Netze und das Gateway als feste IP in `HTTP_PROXY`.
7. Werkzeuge, die `HTTP_PROXY` ignorieren, scheitern geschlossen (keine Route). Das ist erwünscht, die UI sollte es aber erklären.

### Was bleibt für den Hyper-V-Provider offen?

- Läuft der Hyper-V-Provider von Podman auf Windows ARM64 tatsächlich? Das Image existiert, Laufzeit, gvproxy und vsock sind ungetestet. Dafür muss der Nutzer einmalig Hyper-V aktivieren, neu starten und sich in „Hyper-V-Administratoren“ eintragen (Befehle in 7).
- Ob `podman machine init`/`start`/`stop` mit der Gruppenmitgliedschaft ganz ohne UAC auskommen.
- 9p-Performance von Podmans eigenem Server im Vergleich zu drvfs: `bench-run.sh` und `git-pingpong.sh` erneut laufen lassen. Davon hängt ab, ob der Hybrid dort reicht.
- Freigaben ändern ohne `rm` + `init`: offiziell nicht möglich. Prüfen, ob das Bearbeiten der Machine-JSON stabil funktioniert. Sonst muss `harnessd` die Neuanlage samt Sicherung der Volumes übernehmen. Die Standard-Freigaben (inkl. `C:\`) immer explizit überschreiben.
- Netzwerk-Tests (`net-tests.sh`) wiederholen. Unter Hyper-V läuft der Verkehr über gvproxy, Host- und LAN-Erreichbarkeit können sich anders verhalten.
- RAM im Leerlauf: Hyper-V-VMs haben feste Größen (`-m`), WSL ist dynamisch.
- Verhalten nach Standby/Resume.
- Ob auf Rechnern, auf denen Docker Desktop oder andere Podman-Machines laufen, eine WSL- und eine Hyper-V-Machine nebeneinander existieren können (ARCHITECTURE.md 3.1 sagt nein, das sollte geprüft werden).

### Vorgeschlagene Änderungen an ARCHITECTURE.md (nicht vorgenommen)

- 3.4: Die Risikotabelle („Die VM sieht nur die freigegebenen Wurzelordner“) für den WSL-Provider korrigieren, inklusive Interop. Härtung der eigenen WSL-Machine über `wsl.conf` als Aufgabe für Phase 2.
- 3.2: `/etc/gitconfig` mit `core.checkStat=minimal` und `core.trustctime=false` ergänzen.
- 3.5: IP-Prüfung nach der Auflösung, Rebinding-Schutz und die Liste der gesperrten Netze (inkl. `100.64.0.0/10`) ergänzen.
- 9: Den Isoliert-Modus als Fallback für große Repos nach vorne ziehen. In `harnessd` einen Watchdog für Machine und Instanzen nach Standby vorsehen.
- 11: Das 9p-Risiko ist jetzt quantifiziert (etwa 2 ms pro Metadaten-Zugriff).

## 12. Hyper-V-Provider (Nachtest am 11.10.2026)

**Setup:** Hyper-V aktiviert (`Microsoft-Hyper-V-All`), Nutzer in „Hyper-V-Administratoren“ (`S-1-5-32-578`). Machine `harness-hv`: 4 CPUs, max. 4 GB RAM, 30 GB Disk, **eine** Freigabe `spikes\phase-0\work` → `/mnt/work` (9p). Angelegt mit `CONTAINERS_MACHINE_PROVIDER=hyperv podman machine init …` in einer Admin-PowerShell.

### 12.1 Befunde

1. **Jeder Machine-Befehl braucht Admin-Rechte.** `init`, `start`, `stop` und sogar `list` und `inspect` brechen ohne erhöhte Rechte ab („hyperv machines require admin authority“). Die Gruppe Hyper-V-Administratoren reicht für Podman nicht. Hyper-V selbst (`Get-VM`, `Set-VMMemory`, `Stop-VM`, WMI unter `root\virtualization\v2`) funktioniert für Gruppenmitglieder dagegen **ohne** Admin.
2. **Startspeicher = Maximum.** Podman aktiviert zwar Dynamic Memory, setzt aber den Startwert gleich dem Maximum (4 GB). Hyper-V reserviert den Startwert beim Start komplett. Bei 3 GB freiem RAM scheitert der Start mit Job-Fehler **32778** (Ereignis 3122 „nicht genügend Arbeitsspeicher“ im Log `Hyper-V-Worker-Admin`). Abhilfe ohne Admin: `Set-VMMemory -VMName harness-hv -StartupBytes 1GB -MinimumBytes 512MB -MaximumBytes 4GB`. Danach läuft die VM mit 1 GB Zuteilung, der Bedarf nach dem Boot liegt bei etwa **620 MB**.
3. **Kein Aufräumen nach einem fehlgeschlagenen Start, Teil 1: Ignition-KVP.** Podman übergibt die Ignition-Konfiguration als KVP-Items (`ignition.config.0` bis `.n`, je ≤ 990 Zeichen) über den Datenaustausch-Integrationsdienst an die VM. Scheitert der Start, bleiben die Items am VM-Objekt hängen. Jeder weitere Start bricht dann mit **„Illegal argument (key already exists?) (32773)“** ab (Ereignis 4120 im Log `Hyper-V-VMMS-Admin`). Abhilfe ohne Admin: Items über `Msvm_VirtualSystemManagementService.RemoveKvpItems` entfernen (Snippet in 12.2). Die Items werden bei jedem Startversuch neu gesetzt.
4. **vsock-Registrierungen.** Unter `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Virtualization\GuestCommunicationServices\` legt `init` pro Machine Schlüssel `<Port als 8-stellige Hex-Zahl>-FACB-11E6-BD58-64006A7986D3` an, mit den Werten `Purpose` (`Events` = Ready-Signal, `Network` = gvproxy, `Fileserver` = je eine 9p-Freigabe) und `MachineName`. Die Ports stehen in der Machine-JSON (`HyperVHypervisor.ReadyVsock`, `.NetworkVSock`, `Mounts[].VSockNumber`). Alle Einträge werden beim **Start vorausgesetzt**, und das Schreiben braucht Admin-Rechte. Fehlt der Fileserver-Eintrag, wartet der Gast beim 9p-Mount vergeblich („dialing vsock port 54647 … connection timed out“). Podman bricht dann ab, beendet gvproxy und win-sshproxy, und die VM bleibt halb gestartet zurück.
   *Fehler im Spike:* Dieser Eintrag wurde zunächst fälschlich für einen Rest gehalten und gelöscht. Der eigentliche Verursacher war Befund 3.

### 12.2 Hilfsmittel

KVP-Items einer VM auflisten bzw. entfernen (als Mitglied der Hyper-V-Administratoren, ohne Admin):

```powershell
$ns = 'root\virtualization\v2'
$vm = Get-CimInstance -Namespace $ns -ClassName Msvm_ComputerSystem -Filter "ElementName='harness-hv'"
$vssd = Get-CimAssociatedInstance -InputObject $vm -ResultClassName Msvm_VirtualSystemSettingData |
  Where-Object VirtualSystemType -eq 'Microsoft:Hyper-V:System:Realized'
$kvp = Get-CimAssociatedInstance -InputObject $vssd -ResultClassName Msvm_KvpExchangeComponentSettingData
$kvp.HostExchangeItems   # CIM-XML je Item (Name, Data, Source)
$vsms = Get-CimInstance -Namespace $ns -ClassName Msvm_VirtualSystemManagementService
Invoke-CimMethod -InputObject $vsms -MethodName RemoveKvpItems -Arguments @{ TargetSystem = $vm; DataItems = [string[]]$kvp.HostExchangeItems }
```

Container-Befehle laufen über SSH (`spikes/phase-0/pm.sh` mit `PODMAN_SSH_PORT=64411 PODMAN_SSH_USER=core`). Sie brauchen kein Admin, solange die VM läuft. Benchmarks: `hv-setup.sh`, danach `BENCH_VM_WORK=/mnt/work bench-run.sh …`.

### 12.3 Folgerungen für den Windows-Dienst (`harness-vmd`)

Entschieden: **Windows-Dienst statt geplanter Aufgabe.** Bei einer Aufgabe liest der elevierte Podman eine Konfiguration aus dem Benutzerprofil, in die jeder Prozess des Nutzers schreiben kann. Der dann elevierte 9p-Server würde beliebige eingetragene Pfade mit Admin-Rechten freigeben, das wäre eine Rechteausweitung.

Anforderungen aus diesem Test:

- Konfiguration (Freigaben, Ressourcen) in `%ProgramData%`, nur für Admins schreibbar. Freigaben werden gegen eine Regel geprüft, z. B. „nur unterhalb des Benutzerprofils“.
- Schmale IPC-Schnittstelle (Named Pipe, ACL nur für den eingerichteten Nutzer): VM anlegen, starten, stoppen, Status.
- Vor jedem Start: Startspeicher klein setzen (Dynamic Memory) und verwaiste Ignition-KVP-Items entfernen. vsock-Registrierungen gegen die Machine-Konfiguration prüfen und fehlende anlegen.
- Nach jedem gescheiterten Start: VM hart ausschalten, KVP-Items entfernen, Fehler mit Ereignis-ID an die App melden.
- Überwachung: VM-Zustand und Hilfsprozesse (gvproxy, 9p-Server) beobachten, nach Standby neu starten (vgl. Abschnitt 10).

### 12.4 Was die VM vom Host sieht

| | WSL-Provider | Hyper-V-Provider |
|---|---|---|
| Host-Ordner in der VM | ganz `C:\` (rw) | nur die bei `init` angegebene Freigabe (`/mnt/work`) |
| Windows-Programme startbar (Interop) | ja | nein (keine binfmt-Einträge, keine `.exe`) |
| `sudo` ohne Passwort in der VM | ja | ja (Container laufen rootless, die VM ist die Grenze) |
| RAM | 7,7 GB vergeben, unabhängig von `--memory` | 1 GB zugeteilt, Bedarf 0,6–1,0 GB auch während der Benchmarks |
| VM | WSL2-Kernel 6.18, Fedora 44 | Kernel 7.1, Fedora 44 (aarch64), 4 vCPU |

Container-Befehle laufen ohne Admin-Rechte über SSH (`pm.sh`), sobald die VM läuft.

### 12.5 Datei-I/O (`bench-run.sh volume bind hybrid`, Rohdaten `work/bench-results-hv.txt`)

| Workload | Volume | Bind (9p) | Hybrid | zum Vergleich WSL Bind |
|---|---:|---:|---:|---:|
| 5.000 Dateien anlegen | 0,14 | 17,7 | – | 17,4 |
| 5.000 Dateien lesen | 0,05 | 18,3 | – | 13,1 |
| 5.000 × `stat` | 0,02 | 16,4 | – | 10,0 |
| 5.000 Dateien löschen | 0,09 | **EIO** | – | 4,8 |
| `git clone` | 0,4 (lokal) / 2,3 (Netz) | **scheitert** | – | 32–40 |
| `npm ci` | 4,9 / 3,6 | **scheitert** (70 s bis zum Fehler) | 3,9 / 3,6 | 70–80 |
| `npm run build` | 2,2 / 1,8 | – | **scheitert** | 7,8 |
| `vitest run` | 2,1 / 1,3 | – | 2,3 / 1,6 | 24 |

**Die 9p-Freigabe des Hyper-V-Providers ist für Entwicklungsarbeit unbrauchbar**, nicht nur langsam:

- `chmod` scheitert mit EPERM. Damit bricht `git clone` ab (`chmod on .git/config.lock failed`), ebenso `npm ci` (`chmod …/bin/babel-parser.js`).
- `copyfile` scheitert mit EPERM, weil Node dabei die Rechte übernimmt. Dadurch bricht `vite build` ab, auch im Hybrid-Modus, weil `public/` in `dist/` kopiert wird.
- Rekursives Löschen von Verzeichnissen scheitert mit EIO.
- Wo es funktioniert, ist es genauso langsam wie WSL, etwa 3 ms pro Dateioperation.

Volumes in der VM sind dagegen so schnell wie unter WSL und schneller als Windows nativ.

### 12.6 Netzwerk (`net-tests.sh`, `net-tests-hv.sh`)

- **Internes Netz mit Gateway:** identisch zu WSL. Ohne Proxy gibt es weder DNS noch Route. Über das Gateway gehen nur die Allowlist-Domains (npm, git), alles andere bekommt 403. Host (`192.168.127.254`), LAN, Router, Tailscale und Metadaten sind nicht erreichbar.
- **Normales Netz (Standard-Rootless und Bridge, also auch das Gateway selbst):** gvproxy bildet `192.168.127.254` (`host.containers.internal`) auf **`127.0.0.1` des Windows-Hosts** ab. Erreichbar waren der Harness-Server (`127.0.0.1:8080`) und SMB (`445`), außerdem LAN (`192.168.1.43:445`), Router, Tailscale-DNS und das Internet. Nicht erreichbar waren Vite (lauscht nur auf `[::1]`) und Cloud-Metadaten.
- **Folge:** „Nur auf localhost“ ist gegenüber VM und Gateway **keine** Grenze. Instanz-Container dürfen nie in einem normalen Netz hängen. Das Gateway muss zusätzlich zu den privaten Bereichen ausdrücklich `192.168.127.0/24` und `host.containers.internal` blockieren und ist selbst sicherheitskritischer Code.

### 12.7 Empfehlung (vom Nutzer zu bestätigen)

1. **Hyper-V-Provider ist das Ziel unter Windows Pro/Enterprise.** Die Isolation ist deutlich besser (nur explizite Freigaben, kein Interop) und RAM-sparsam mit dynamischem Speicher. Preis: der privilegierte Dienst `harness-vmd`.
2. **Der Direkt-Modus per Bind-Mount entfällt unter Windows.** Unter Hyper-V ist er kaputt, unter WSL zu langsam und unsicher. **Standard wird der Isoliert-Modus:** Die Arbeitskopie liegt in einem Volume in der VM, Austausch mit dem Host-Repo über Git (Git-Worker). Für Nutzer, die Dateien live in ihrer IDE sehen wollen, kommt später ein **Sync-Modus** in Frage (Mutagen-artig: Volume als Quelle, Spiegelung auf den Host).
3. Die Machine bekommt **keine** Host-Freigaben (`init` ohne `--volume`, Podman setzt sonst `C:\` und das Profil ein). Dateien kommen über Git oder `podman cp` hinein.

## Anhang: Skripte und Rohdaten

| Datei | Zweck |
|---|---|
| `spikes/phase-0/pm.sh` | `podman.exe`-Wrapper mit isolierter `known_hosts` |
| `spikes/phase-0/measure-vmmem.ps1` | Speicher von `vmmem*` messen |
| `spikes/phase-0/bench-run.sh`, `bench-step.sh`, `smallfiles.mjs` | Datei-I/O-Benchmark (`bench-run.sh containerfs volume bind hybrid`) |
| `spikes/phase-0/app-overlay/` | `package.json`, `vite.config.ts` und Test für die Benchmark-App |
| `spikes/phase-0/git-pingpong.sh` | Windows-git ↔ Linux-git auf demselben Repo |
| `spikes/phase-0/watch.mjs`, `watch-host.ps1` | Dateiänderungs-Ereignisse im Container bzw. unter Windows |
| `spikes/phase-0/proxy/` | tinyproxy-Gateway (Containerfile, Konfiguration, Allowlist) |
| `spikes/phase-0/net-tests.sh` | Erreichbarkeitstests im Client-Container |
| `spikes/phase-0/measure-app-ram.ps1` | RAM einer App samt WebView2-Kindprozessen (für den Tauri-Test) |
| `spikes/phase-0/work/bench-results.txt`, `native-results.txt`, `net-*.txt`, `server-tests-postgres.txt`, `tauri-build.log` | Rohdaten |

Aufgeräumt wurden die Benchmark- und Netz-Container, die Netze `harness-int-test*`/`harness-ext-test` und die Volumes `bench-*`. Behalten wurden die Images `node:24-bookworm`, `postgres:17-alpine`, `alpine:3.22` und `localhost/harness-spike-proxy`, außerdem `harness-pg` mit seinem Daten-Volume und die Machine `harness-spike`. Der verwaiste `win-sshproxy` aus der Sitzung vor dem Standby läuft noch.
