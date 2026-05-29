# memori-rs

[![crates.io](https://img.shields.io/crates/v/memori-rs.svg)](https://crates.io/crates/memori-rs)
[![docs.rs](https://img.shields.io/docsrs/memori-core)](https://docs.rs/memori-core)
[![license](https://img.shields.io/crates/l/memori-rs.svg)](LICENSE)

> Persistent semantic memory for AI coding agents — single binary, zero cloud.

Gives Claude Code, Cursor, and Continue.dev a `memori.store` / `memori.recall` tool backed by a local vector database. Context from past sessions survives across restarts. Everything stays on your machine.

```
memori init      # wire up your AI clients (writes MCP config)
memori doctor    # verify everything is working
```

---

## How it works

```
your AI client (Claude Code / Cursor / ...)
        │  MCP stdio (JSON-RPC 2.0)
        ▼
   memori mcp                     ← single binary, spawned by the client
        │
        ├── fastembed (BGE-small-en-v1.5, ONNX, local)
        └── LanceDB (embedded vector store, ~/.local/share/memori/)
```

No daemon. No API key. No outbound traffic after the first model download (~25 MB).

---

## Install

**Prebuilt binary (macOS / Linux)** — no toolchain needed:

```bash
curl -fsSL https://raw.githubusercontent.com/KvizadSaderah/memori-rs/main/install.sh | bash
```

Downloads the right binary for your platform, drops it in `~/.local/bin`, and runs
`memori init` + `memori doctor` for you.

**From [crates.io](https://crates.io/crates/memori-rs)** (any platform, needs a [Rust toolchain](https://rustup.rs)):

```bash
cargo install memori-rs
memori init
memori doctor
```

> Installs a binary named `memori`. If you already have the unrelated `memori`
> crate (a Rust benchmarking tool) installed, cargo will refuse to overwrite its
> binary — either remove it first with `cargo uninstall memori`, or force ours
> with `cargo install memori-rs --force`.

**From source** (latest unreleased changes):

```bash
cargo install --git https://github.com/KvizadSaderah/memori-rs memori-rs
memori init
memori doctor
```

Then restart your AI client. In Claude Code, run `/mcp` — it should list `memori` with its tools.

> Note: `memori doctor` runs a real store → recall → forget roundtrip, so a green
> `roundtrip` check means the whole stack (embeddings + vector store) is working.

Full walkthrough: [docs/getting-started.md](docs/getting-started.md)

---

## MCP tools (4 total)

| Tool | Description |
|---|---|
| `memori.store` | Persist text with optional tags and source label |
| `memori.recall` | Semantic search, top-k results by similarity |
| `memori.list` | Paginate all memories with filters |
| `memori.forget` | Delete by UUID or filter (older\_than, tags, source) |

Reference: [docs/mcp-tools.md](docs/mcp-tools.md)

---

## CLI

```bash
memori dump                      # list stored memories
memori forget --older-than 30d   # prune stale context
memori forget --dry-run --tags tmp
```

Reference: [docs/cli.md](docs/cli.md)

---

## Making agents actually use it

Add a short instruction to your project's `CLAUDE.md`:

```markdown
## Memory
Call `memori.recall` at session start with the current task.
Call `memori.store` when you learn something durable about this codebase.
Tag memories by area. Set source to "claude-code".
```

Guide with examples: [docs/system-prompt.md](docs/system-prompt.md)

---

## Storage

| Path | What |
|---|---|
| `~/Library/Application Support/memori/` (macOS) | LanceDB vector store |
| `~/.local/share/memori/` (Linux) | LanceDB vector store |
| `.fastembed_cache/` | ONNX model cache (~25 MB, first run only) |

---

## NFRs (verified)

| | Target | Actual |
|---|---|---|
| Binary size | ≤ 50 MB | 35 MB |
| Cold start | ≤ 500 ms | ~80 ms (model already cached) |
| Recall p95 at 10k records | ≤ 100 ms | TBD (bench in progress) |
| Outbound traffic (normal ops) | zero | zero |

---

## License

MIT — see [LICENSE](LICENSE).
