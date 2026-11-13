# Hearth

[Deutsch](README.de.md)

Hearth is a self-hosted chat for large language models. Several people share one server, each with their own API keys. A chat can also hand its work to a coding agent that runs in an isolated sandbox on the user's own Windows PC, so the agent can build, test and commit code without access to the rest of the machine.

## Features

**Chat (server and web app)**
- Bring your own keys: Anthropic, OpenAI, Google Gemini, Mistral, OpenRouter, Ollama and any OpenAI-compatible endpoint such as DeepSeek. Keys are stored encrypted and are only used for their owner's requests.
- Research mode with web search (SearXNG, Brave or Tavily). The model reads only the relevant sections of long pages and cites its sources per paragraph.
- Web pages from hosts the user did not mention load only after approval, unless the user switches on free web access. Text that looks like a prompt injection is flagged with the exact location, and a helper model can rate the warning.
- Images, PDF, Word, Excel and PowerPoint files in chats, with text recognition for scanned PDFs. Long documents are searched instead of sent in full.
- Per-chat options: thinking effort, answer length and tone. Long chats are condensed and summarized automatically to keep the context affordable.
- A memory the user controls, tags that group chats into folders, and costs per reply and per model call.
- OpenRouter routing to the cheapest host, or to a fixed host chosen per model, with its country of operation shown.
- Accounts with admin roles, closed registration by default, two-factor sign-in and a daily cost limit.
- Installable as an app on phones and desktops (PWA) when the server is reached over HTTPS. A chat open on several devices stays in step: a message written on the phone appears on the laptop, with its reply as it is written.
- Search across chat titles, tags and the text of the messages.

**Agents on your PC (Windows client)**
- The [pi](https://www.npmjs.com/package/@earendil-works/pi-coding-agent) coding agent runs in a hardened container inside a Linux VM (Hyper-V and Podman). The VM has no access to the host's files.
- All network traffic of the sandbox goes through a gateway with per-project rules. Unknown hosts need approval in the chat or terminal.
- API keys never enter the sandbox. The gateway forwards model requests to the server, which adds the key.
- The agent works on a copy of the checked-out branch. Its commits come back only after a review with fixed rules, a check of new dependencies against the OSV database and an optional AI assessment. Merges a web chat asks for need approval on the PC unless the project allows them there.
- Toolchains such as C/C++, Rust, Go, Java, Deno and Bun can be switched on per project, as can your own programs.
- A tray icon shows the state of the VM and asks for approvals. The VM stops on its own when idle.

## How it fits together

```
Browser ──HTTPS──► Server (API, web app, SQLite or Postgres)
                     ▲
                     │ WebSocket tunnel, device token
                     │
Windows PC: hearth background service ──► Hyper-V VM (Podman)
                                            ├─ gateway (network rules, approvals)
                                            └─ sandbox with the agent and a copy of the project
```

[`docs/how-it-works.md`](docs/how-it-works.md) explains the parts and the reasoning behind them (German).

## Requirements

- **Server:** Docker with Compose 2.24 or newer, on amd64 or arm64. A Raspberry Pi 5 with 4 GB is enough.
- **Client (optional, for agents):** Windows 11 Pro, Enterprise or Education (Hyper-V is required, Windows Home does not have it), x64 or ARM64, 16 GB of RAM recommended. Git and Podman are installed by the setup program.

## Quick start

### Server

```sh
mkdir hearth && cd hearth
curl -LO https://raw.githubusercontent.com/johnnyjack123/hearth/main/docker-compose.yml
mkdir -p data && sudo chown 1000:1000 data
echo "PUBLIC_URL=http://<address-of-this-machine>:8080" > .env
docker compose up -d
docker compose logs server | grep "setup code"
```

Open `PUBLIC_URL`, create the first account with the setup code from the log (it becomes the admin) and add an API key under Settings → API keys.

Postgres, web search, HTTPS and running the server on the internet are covered in [`docs/deploy.md`](docs/deploy.md).

### Windows client

1. Download `hearth-setup-x64.exe` or `hearth-setup-arm64.exe` from the [latest release](https://github.com/johnnyjack123/hearth/releases/latest) and run it. It checks Hyper-V, installs Git and Podman if needed and creates the VM (a download of about 1.5 GB).
2. **Connect the PC to your server.** Near the end, the setup asks for the server's address, the `PUBLIC_URL` (e.g. `https://hearth.example.org`). It shows a code and opens a confirmation page in the browser: sign in there with your account and confirm the code. From then on the PC's agents use the models of your account; the API keys never leave the server. You can also give the address up front with `hearth setup --server <address>`, pair again later with `hearth login <address>` (also after the server moved to a new address), and undo the pairing with `hearth logout`.
3. In a project folder, run `hearth init`. The project then appears in the web chat under the gear menu → Environment.
4. Alternatively, run `hearth` in the project folder to work with the agent in the terminal.

Details and troubleshooting: [`docs/client.md`](docs/client.md).

## Documentation

The detailed documentation is in German.

| Document | Contents |
|---|---|
| [`docs/deploy.md`](docs/deploy.md) | Server: installation, configuration, updates, backups, HTTPS, internet exposure |
| [`docs/client.md`](docs/client.md) | Windows client: setup, projects, web chat and terminal, updates, troubleshooting |
| [`docs/how-it-works.md`](docs/how-it-works.md) | How the parts work and why they were built this way |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | The original design document |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Phases and what comes next |

## Development

```sh
pnpm install
pnpm --filter @hearth/server dev     # API on :8080
pnpm --filter @hearth/web dev        # web app on :5173
pnpm --filter @hearth/server test
pnpm --filter @hearth/web e2e        # browser test against an isolated stack
cargo build
cargo test
```

The server and web app are TypeScript (Hono, React, AI SDK); the client, the VM service and the network gateway are Rust.

## Status

Hearth is in active development and has not reached 1.0. The Windows programs are not code-signed yet, so SmartScreen and Microsoft Defender may warn about them.

## License

Hearth is free software under the [GNU Affero General Public License v3.0 or later](LICENSE.md). If you run a modified version as a service for others, you must offer them its source code.
