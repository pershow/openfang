# Getting Started with OpenParlant

This guide walks you through installing OpenParlant, configuring your first LLM provider, spawning an agent, and chatting with it.

## Table of Contents

- [Installation](#installation)
- [Configuration](#configuration)
- [Spawn Your First Agent](#spawn-your-first-agent)
- [Chat with an Agent](#chat-with-an-agent)
- [Start the Daemon](#start-the-daemon)
- [Using the WebChat UI](#using-the-webchat-ui)
- [Next Steps](#next-steps)

---

## Installation

### Option 1: Desktop App (Windows / macOS / Linux)

Download the installer for your platform from the [latest release](https://github.com/RightNow-AI/openparlant/releases/latest):

| Platform | File |
|---|---|
| Windows | `.msi` installer |
| macOS | `.dmg` disk image |
| Linux | `.AppImage` or `.deb` |

The desktop app includes the full OpenParlant system with a native window, system tray, auto-updates, and OS notifications. Updates are installed automatically in the background.

### Option 2: Shell Installer (Linux / macOS)

```bash
curl -sSf https://openparlant.sh | sh
```

This downloads the latest CLI binary and installs it to `~/.openparlant/bin/`.

### Option 3: PowerShell Installer (Windows)

```powershell
irm https://openparlant.sh/install.ps1 | iex
```

Downloads the latest CLI binary, verifies its SHA256 checksum, and adds it to your user PATH.

### Option 4: Cargo Install (Any Platform)

Requires Rust 1.75+:

```bash
cargo install --git https://github.com/RightNow-AI/openparlant openparlant-cli
```

Or build from source:

```bash
git clone https://github.com/RightNow-AI/openparlant.git
cd openparlant
cargo install --path crates/openparlant-cli
```

### Option 5: Docker

```bash
docker pull ghcr.io/RightNow-AI/openparlant:latest

docker run -d \
  --name openparlant \
  -p 4200:4200 \
  -e ANTHROPIC_API_KEY=$ANTHROPIC_API_KEY \
  -v openparlant-data:/data \
  ghcr.io/RightNow-AI/openparlant:latest
```

Or use Docker Compose:

```bash
git clone https://github.com/RightNow-AI/openparlant.git
cd openparlant
# Set your API keys in environment or .env file
docker compose up -d
```

### Verify Installation

```bash
openparlant --version
```

---

## Configuration

### Initialize

Run the init command to create the `~/.openparlant/` directory and a default config file:

```bash
openparlant init
```

This creates:

```
~/.openparlant/
  config.toml    # Main configuration
  data/          # Database and runtime data
  agents/        # Agent manifests (optional)
```

### Set Up an API Key

OpenParlant needs at least one LLM provider API key. Set it as an environment variable:

```bash
# Anthropic (Claude)
export ANTHROPIC_API_KEY=sk-ant-...

# Or OpenAI
export OPENAI_API_KEY=sk-...

# Or Groq (free tier available)
export GROQ_API_KEY=gsk_...
```

Add the export to your shell profile (`~/.bashrc`, `~/.zshrc`, etc.) to persist it.

### Edit the Config

The default config uses Anthropic. To change the provider, edit `~/.openparlant/config.toml`:

```toml
[default_model]
provider = "groq"                      # anthropic, openai, groq, ollama, etc.
model = "llama-3.3-70b-versatile"      # Model identifier for the provider
api_key_env = "GROQ_API_KEY"           # Env var holding the API key

[memory]
decay_rate = 0.05                      # Memory confidence decay rate

[network]
listen_addr = "127.0.0.1:4200"        # OFP listen address
```

### Verify Your Setup

```bash
openparlant doctor
```

This checks that your config exists, API keys are set, and the toolchain is available.

---

## Spawn Your First Agent

### Using a Built-in Template

OpenParlant ships with 30 agent templates. Spawn the hello-world agent:

```bash
openparlant agent spawn agents/hello-world/agent.toml
```

Output:

```
Agent spawned successfully!
  ID:   a1b2c3d4-e5f6-...
  Name: hello-world
```

### Using a Custom Manifest

Create your own `my-agent.toml`:

```toml
name = "my-assistant"
version = "0.1.0"
description = "A helpful assistant"
author = "you"
module = "builtin:chat"

[model]
provider = "groq"
model = "llama-3.3-70b-versatile"

[capabilities]
tools = ["file_read", "file_list", "web_fetch"]
memory_read = ["*"]
memory_write = ["self.*"]
```

Then spawn it:

```bash
openparlant agent spawn my-agent.toml
```

### List Running Agents

```bash
openparlant agent list
```

Output:

```
ID                                     NAME             STATE      PROVIDER     MODEL
-----------------------------------------------------------------------------------------------
a1b2c3d4-e5f6-...                     hello-world      Running    groq         llama-3.3-70b-versatile
```

---

## Chat with an Agent

Start an interactive chat session using the agent ID:

```bash
openparlant agent chat a1b2c3d4-e5f6-...
```

Or use the quick chat command (picks the first available agent):

```bash
openparlant chat
```

Or specify an agent by name:

```bash
openparlant chat hello-world
```

Example session:

```
Chat session started (daemon mode). Type 'exit' or Ctrl+C to quit.

you> Hello! What can you do?

agent> I'm the hello-world agent running on OpenParlant. I can:
- Read files from the filesystem
- List directory contents
- Fetch web pages

Try asking me to read a file or look up something on the web!

  [tokens: 142 in / 87 out | iterations: 1]

you> List the files in the current directory

agent> Here are the files in the current directory:
- Cargo.toml
- Cargo.lock
- README.md
- agents/
- crates/
- docs/
...

you> exit
Chat session ended.
```

---

## Start the Daemon

For persistent agents, multi-user access, and the WebChat UI, start the daemon:

```bash
openparlant start
```

Output:

```
Starting OpenParlant daemon...
OpenParlant daemon running on http://127.0.0.1:4200
Press Ctrl+C to stop.
```

The daemon provides:
- **REST API** at `http://127.0.0.1:4200/api/`
- **WebSocket** endpoint at `ws://127.0.0.1:4200/api/agents/{id}/ws`
- **WebChat UI** at `http://127.0.0.1:4200/`
- **OFP networking** on port 4200

### Check Status

```bash
openparlant status
```

### Stop the Daemon

Press `Ctrl+C` in the terminal running the daemon, or:

```bash
curl -X POST http://127.0.0.1:4200/api/shutdown
```

---

## Using the WebChat UI

With the daemon running, open your browser to:

```
http://127.0.0.1:4200/
```

The embedded WebChat UI allows you to:
- View all running agents
- Chat with any agent in real-time (via WebSocket)
- See streaming responses as they are generated
- View token usage per message

---

## Next Steps

Now that you have OpenParlant running:

- **Explore agent templates**: Browse the `agents/` directory for 30 pre-built agents (coder, researcher, writer, ops, analyst, security-auditor, and more).
- **Create custom agents**: Write your own `agent.toml` manifests. See the [Architecture guide](architecture) for details on capabilities and scheduling.
- **Set up channels**: Connect any of 40 messaging platforms (Telegram, Discord, Slack, WhatsApp, LINE, Mastodon, and 34 more). See [Channel Adapters](channel-adapters).
- **Use bundled skills**: 60 expert knowledge skills are pre-installed (GitHub, Docker, Kubernetes, security audit, prompt engineering, etc.). See [Skill Development](skill-development).
- **Build custom skills**: Extend agents with Python, WASM, or prompt-only skills. See [Skill Development](skill-development).
- **Use the API**: 76 REST/WS/SSE endpoints, including an OpenAI-compatible `/v1/chat/completions`. See [API Reference](api-reference).
- **Switch LLM providers**: 20 providers supported (Anthropic, OpenAI, Gemini, Groq, DeepSeek, xAI, Ollama, and more). Per-agent model overrides.
- **Set up workflows**: Chain multiple agents together. Use `openparlant workflow create` with a TOML workflow definition.
- **Use MCP**: Connect to external tools via Model Context Protocol. Configure in `config.toml` under `[[mcp_servers]]`.
- **Migrate from OpenClaw**: Run `openparlant migrate --from openclaw`. See [MIGRATION.md](../MIGRATION.md).
- **Desktop app**: Run `cargo tauri dev` for a native desktop experience with system tray.
- **Run diagnostics**: `openparlant doctor` checks your entire setup.

### Useful Commands Reference

```bash
openparlant init                          # Initialize ~/.openparlant/
openparlant start                         # Start the daemon
openparlant status                        # Check daemon status
openparlant doctor                        # Run diagnostic checks

openparlant agent spawn <manifest.toml>   # Spawn an agent
openparlant agent list                    # List all agents
openparlant agent chat <id>               # Chat with an agent
openparlant agent kill <id>               # Kill an agent

openparlant workflow list                 # List workflows
openparlant workflow create <file.json>   # Create a workflow
openparlant workflow run <id> <input>     # Run a workflow

openparlant trigger list                  # List event triggers
openparlant trigger create <args>         # Create a trigger
openparlant trigger delete <id>           # Delete a trigger

openparlant skill install <source>        # Install a skill
openparlant skill list                    # List installed skills
openparlant skill search <query>          # Search FangHub
openparlant skill create                  # Scaffold a new skill

openparlant channel list                  # List channel status
openparlant channel setup <channel>       # Interactive setup wizard

openparlant config show                   # Show current config
openparlant config edit                   # Open config in editor

openparlant chat [agent]                  # Quick chat (alias)
openparlant migrate --from openclaw       # Migrate from OpenClaw
openparlant mcp                           # Start MCP server (stdio)
```
