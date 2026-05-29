# System Prompt Guide

memori doesn't automatically store or recall anything — the AI agent decides when to use the tools based on its instructions. This page gives practical system prompt snippets.

---

## Minimal (Claude Code CLAUDE.md)

Add to `CLAUDE.md` at the project root:

```markdown
## Memory

You have access to a persistent memory tool (memori). Use it as follows:

- **On session start**: call `memori.recall` with the current task or file you're working on
  to pull in relevant context from past sessions.
- **When you learn something durable**: call `memori.store` — architecture decisions,
  gotchas, non-obvious conventions, dependency constraints.
- **Tag memories** with the area: e.g. `["storage", "lancedb"]`, `["api", "auth"]`.
- **Source**: always set `source` to `"claude-code"` so memories are attributable.
- Do NOT store transient state, TODO lists, or things already in the codebase.
```

---

## What to store — good examples

```
"LanceDB 0.29 requires Arrow 58 — downgrading to 0.19 causes type mismatch on Scannable"
"fastembed TextEmbedding::try_new requires the hf-hub feature flag"
"The #[tool(tool_box)] macro does NOT auto-wire ServerHandler — must explicitly implement call_tool"
"Binary size target: ≤ 50 MB. Release profile: opt-level=z + lto + strip"
"tags column stores JSON array as text, not native array — LIKE '%\"tag\"%' for filtering"
```

## What NOT to store

```
"Fix the bug in storage.rs"        ← transient task, not durable knowledge
"The test failed"                  ← noise
"TODO: add pagination"             ← use the issue tracker
"user.name = 'Alice'"              ← runtime data, not architecture knowledge
```

---

## Recall query tips

Recall uses semantic similarity, not keyword matching. Write queries as natural language:

```
# Good
"how do we handle errors in the storage layer"
"what version constraints apply to arrow"
"authentication flow and token handling"

# Less effective
"error"
"arrow"
"auth"
```

---

## Manual curation

Keep the memory clean with periodic review:

```bash
memori dump                          # see everything
memori forget --older-than 90d       # prune stale context
memori forget --source cursor        # clear another agent's memories
memori forget --id <uuid>            # remove one wrong memory
```
