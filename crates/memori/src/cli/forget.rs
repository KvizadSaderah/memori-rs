use anyhow::Result;
use memori_core::{ForgetFilter, Memory};
use uuid::Uuid;

use super::parse_duration_ago;
use crate::ipc::{client, proto::Request};

pub async fn run(
    id: Option<String>,
    tags: Vec<String>,
    source: Option<String>,
    older_than: Option<String>,
    dry_run: bool,
) -> Result<()> {
    let has_id = id.is_some();
    let has_filter = older_than.is_some() || !tags.is_empty() || source.is_some();

    if !has_id && !has_filter {
        eprintln!(
            "Error: must supply --id or at least one filter flag (--tag, --source, --older-than)"
        );
        std::process::exit(2);
    }
    if has_id && has_filter {
        eprintln!("Error: --id and filter flags are mutually exclusive");
        std::process::exit(2);
    }

    // Validate inputs up front so we fail fast regardless of transport.
    let id_uuid = if let Some(ref s) = id {
        Some(
            s.parse::<Uuid>()
                .map_err(|_| anyhow::anyhow!("--id is not a valid UUID: {s}"))?,
        )
    } else {
        None
    };
    let cutoff = older_than
        .as_ref()
        .map(|s| parse_duration_ago(s).map_err(anyhow::Error::msg))
        .transpose()?;

    if dry_run {
        return preview(id_uuid, &tags, &source, cutoff).await;
    }

    // Live delete — route through the running MCP server when present, so
    // LanceDB always has a single owner. Falls back to direct access.
    let req = Request::Forget {
        id: id_uuid.map(|u| u.to_string()),
        tags: tags.clone(),
        source: source.clone(),
        older_than: cutoff.map(|t| t.to_rfc3339()),
    };

    let count = if let Some(data) = client::call(&req).await? {
        data["deleted_count"].as_u64().unwrap_or(0)
    } else {
        let data_dir = Memory::default_data_dir();
        let memory = Memory::open(&data_dir).await?;
        let filter = if id_uuid.is_some() {
            None
        } else {
            Some(ForgetFilter {
                older_than: cutoff,
                tags,
                source,
            })
        };
        memory.forget(id_uuid, filter).await?
    };

    println!(
        "Deleted {count} memor{}.",
        if count == 1 { "y" } else { "ies" }
    );
    Ok(())
}

/// Show what a live delete would remove, without touching anything.
async fn preview(
    id_uuid: Option<Uuid>,
    tags: &[String],
    source: &Option<String>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<()> {
    // For id deletes we don't filter server-side; for filter deletes we let the
    // server/storage apply tag+source filters, then narrow by cutoff here.
    let (filter_tags, filter_source) = if id_uuid.is_some() {
        (vec![], None)
    } else {
        (tags.to_vec(), source.clone())
    };

    let req = Request::List {
        limit: 100,
        cursor: None,
        tags: filter_tags.clone(),
        source: filter_source.clone(),
    };

    let items: Vec<serde_json::Value> = if let Some(data) = client::call(&req).await? {
        data["items"].as_array().cloned().unwrap_or_default()
    } else {
        let data_dir = Memory::default_data_dir();
        let memory = Memory::open(&data_dir).await?;
        let (records, _) = memory.list(100, None, filter_tags, filter_source).await?;
        records
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "content": r.content,
                    "created_at": r.created_at.to_rfc3339(),
                })
            })
            .collect()
    };

    let matching: Vec<&serde_json::Value> = items
        .iter()
        .filter(|r| {
            if let Some(target) = id_uuid {
                r["id"].as_str() == Some(target.to_string().as_str())
            } else if let Some(t) = cutoff {
                r["created_at"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc) < t)
                    .unwrap_or(false)
            } else {
                true
            }
        })
        .collect();

    if matching.is_empty() {
        println!("Would delete 0 memories. Nothing matched.");
    } else {
        let n = matching.len();
        println!(
            "Would delete {n} memor{}:",
            if n == 1 { "y" } else { "ies" }
        );
        for r in &matching {
            let id = r["id"].as_str().unwrap_or("-");
            let created = r["created_at"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| "-".into());
            println!(
                "  {} — {} — {}",
                &id[..id.len().min(8)],
                created,
                truncate(r["content"].as_str().unwrap_or(""), 60),
            );
        }
        if n == 100 {
            println!("  (preview capped at 100; more may match)");
        }
        println!("Re-run without --dry-run to confirm.");
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}
