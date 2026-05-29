# CLI Reference

The `memori` binary is used both as the MCP server (by AI clients) and as a human-facing operator tool.

---

## `memori init`

Detect installed AI clients and write the MCP server entry into each config.

```bash
memori init
```

Output:
```
  data dir: /Users/you/Library/Application Support/memori (created)

  CLIENT               STATUS  CONFIG
  Claude Desktop       ✓       ~/Library/Application Support/Claude/claude_desktop_config.json
  Claude Code          ✓       ~/.claude.json
  Cursor               —       ~/.cursor/mcp.json (not found)
  Continue.dev         —       ~/.continue/config.json (not found)
```

`✓` = config written. `—` = client not installed, skipped.

Safe to re-run — uses atomic write (temp file → rename), never corrupts existing config.

---

## `memori mcp`

Start the MCP stdio server. This is what AI clients invoke — you don't run it directly.

```bash
memori mcp
```

Reads/writes JSON-RPC 2.0 on stdin/stdout per the MCP spec. Logs go to stderr.

---

## `memori doctor`

Check that everything is correctly configured.

```bash
memori doctor
memori doctor --json   # machine-readable output
```

Checks:
- `data_dir` — exists and is writable
- `embedding_model` — BGE-small-en-v1.5 loads successfully
- `roundtrip` — a real store → recall → forget cycle on a throwaway probe memory
  (goes through the running MCP server over IPC if one is up, otherwise opens the
  store directly; the probe is always cleaned up afterwards)
- `client_integration` — each supported client's config contains the MCP entry

Exit code 0 = all green. Exit code 1 = one or more failures.

---

## `memori dump`

List stored memories in a human-readable table.

```bash
memori dump                     # truncated, scannable table
memori dump --full              # full content, wrapped (no boxes)
memori dump --md                # plain Markdown (pipe into a file / Obsidian)
memori dump --json              # JSON array output
memori dump --limit 50          # default 50
memori dump --tag rust          # filter by tag (repeatable)
memori dump --source claude-code
```

Output (table):
```
ID          CREATED               SOURCE        TAGS      CONTENT
────────────────────────────────────────────────────────────────────────────
a1b2c3d4    2026-05-29 10:00 UTC  claude-code   rust      We pin LanceDB to 0.29 — upgrading b…
```

`--full` and `--md` print the entire content. `--md` emits stable, copy-paste-friendly
Markdown — handy for archiving to an Obsidian vault: `memori dump --md > memories.md`.

Colors are emitted only on a TTY and respect `NO_COLOR`.

---

## `memori recall`

Semantic search over stored memories from the terminal — same ranking the AI client gets.

```bash
memori recall "how do we handle pagination"
memori recall "lancedb version" --top-k 10
memori recall "rust" --tag core
memori recall "..." --json
```

Each result shows its similarity score (0–1, higher is closer), id, source and tags,
followed by the wrapped content.

---

## `memori show`

Show a single memory by id (a unique prefix is enough).

```bash
memori show ee66bfb5
memori show ee66bfb5 --json
```

---

## `memori edit`

Open a memory's content in your editor; on save it is re-embedded in place,
keeping the same id, creation time, tags and source.

```bash
memori edit ee66bfb5
```

Uses `$VISUAL`, then `$EDITOR`, falling back to `vi`. Saving an unchanged or empty
buffer is a no-op (use `memori forget` to delete).

---

## `memori forget`

Delete memories from the command line.

```bash
# By UUID (exact)
memori forget --id a1b2c3d4-...

# By filter
memori forget --older-than 30d
memori forget --tags tmp
memori forget --source cursor
memori forget --tags tmp --older-than 7d   # AND

# Preview before deleting
memori forget --older-than 30d --dry-run
```

`--dry-run` prints what would be deleted without touching the database.

`--older-than` accepts `Nd` (days) or `Nh` (hours): `7d`, `24h`, `90d`.
