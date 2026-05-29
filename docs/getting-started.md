# Getting Started

## Installation

### Option A — build from source (current)

```bash
git clone https://github.com/Dmitry-Efremov/memori-rs
cd memori-rs
cargo build --release
cp target/release/memori ~/.local/bin/
```

### Option B — cargo install (once published)

```bash
cargo install memori-rs
```

Binary size: ~35 MB. No runtime dependencies.

---

## Setup (2 commands)

```bash
memori init    # detect AI clients, write MCP config, create data dir
memori doctor  # verify everything is wired up
```

`memori init` writes the MCP server entry to every AI client config it finds:

| Client | Config path |
|---|---|
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Claude Code | `~/.claude/settings.json` |
| Cursor | `~/.cursor/mcp.json` |
| Continue.dev | `~/.continue/config.json` |

**After `memori init`: restart your AI client** so it picks up the new MCP server.

---

## Verify the MCP server is live

In Claude Code, type `/mcp` — you should see `memori` in the list with 4 tools:

```
memori  ●  store  recall  list  forget
```

If it's missing: run `memori doctor` to see what's wrong.

---

## First real use

Tell Claude to store something:

> "Remember that in this project we pin LanceDB to 0.29 — upgrading breaks Arrow compatibility."

Or store it yourself:

```bash
memori dump                          # see what's stored
memori forget --older-than 30d       # clean up old memories
memori forget --source claude-code   # clean up by agent
```

---

## Data location

Everything lives locally:

```
~/Library/Application Support/memori/   (macOS)
~/.local/share/memori/                  (Linux)
  └── memories.lance/                   ← LanceDB vector store
```

The embedding model (~25 MB) is downloaded on first use to `.fastembed_cache/` in the working directory.

No outbound traffic after the first model download.
