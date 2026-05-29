# MCP Tools Reference

memori exposes exactly 4 tools over MCP stdio. No more will be added at MVP — schema size is a first-class concern.

---

## `memori.store`

Persist a text memory.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `content` | string | ✓ | Text to store. Max 64 KiB. |
| `tags` | string[] | | Labels for filtering. |
| `source` | string | | Agent or user identifier (e.g. `"claude-code"`). |

**Response:**
```json
{ "id": "uuid", "created_at": "2026-05-29T10:00:00Z" }
```

**Example prompt to Claude:**
> "Store this as a memory: we use `dirs::data_local_dir()` for the data path so it respects XDG on Linux."

---

## `memori.recall`

Semantic search — returns top-k memories ranked by cosine similarity to the query.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `query` | string | — | Free-text search query. |
| `top_k` | integer | 5 | How many results. Max 25. |
| `tag_filter` | string[] | `[]` | Only return memories with ALL these tags. |

**Response:**
```json
{
  "results": [
    { "id": "uuid", "content": "...", "score": 0.87, "created_at": "...", "tags": [], "source": null }
  ]
}
```

Score is in [0, 1] — higher is more similar. Anything above ~0.6 is a strong match for BGE-small.

**Example:**
> "Recall what you know about how we handle errors in this codebase."

---

## `memori.list`

Paginate all memories, newest first. Useful for auditing or bulk operations.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `limit` | integer | 20 | Records per page. Max 100. |
| `cursor` | string | | Pagination cursor from previous response. |
| `tag_filter` | string[] | `[]` | Filter by tags. |
| `source_filter` | string | | Filter by source agent. |

**Response:**
```json
{ "items": [...], "next_cursor": "base64-opaque-token" }
```

Pass `next_cursor` back as `cursor` to get the next page. `null` means no more pages.

---

## `memori.forget`

Delete memories. Supply **either** `id` **or** filter criteria — not both.

| Parameter | Type | Description |
|---|---|---|
| `id` | string (UUID) | Delete one specific memory. |
| `older_than` | string | Delete memories older than this. Format: `"7d"`, `"24h"`. |
| `tags` | string[] | Delete memories with ALL these tags. |
| `source` | string | Delete all memories from this source. |

Multiple filter criteria are ANDed together.

**Response:**
```json
{ "deleted_count": 3 }
```

**Examples:**
```
# Delete one
memori.forget(id="d2c04325-...")

# Delete everything from a specific agent
memori.forget(source="cursor")

# Delete stale context
memori.forget(older_than="30d")

# Delete by tag + age
memori.forget(tags=["tmp"], older_than="1d")
```
