use anyhow::{Result, bail};
use memori_core::Memory;

use crate::cli::ui;
use crate::ipc::{client, proto::Request};

pub async fn run(id_prefix: String, json: bool) -> Result<()> {
    let req = Request::Get {
        id_prefix: id_prefix.clone(),
    };

    let record = if let Some(data) = client::call(&req).await? {
        data
    } else {
        // Fallback: direct LanceDB
        direct_get(&id_prefix).await?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&record)?);
        return Ok(());
    }

    let id = record["id"].as_str().unwrap_or("-");
    let created = record["created_at"].as_str().unwrap_or("-");
    let src = record["source"].as_str().unwrap_or("-");
    let tags: Vec<&str> = record["tags"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let content = record["content"].as_str().unwrap_or("");

    let label = |s: &str| ui::dim(&format!("{s:<8}"));
    println!("{} {}", label("id"), ui::cyan(id));
    println!("{} {}", label("created"), fmt_created(created));
    println!("{} {}", label("source"), ui::magenta(src));
    let tags_str = if tags.is_empty() {
        ui::dim("-")
    } else {
        tags.iter()
            .map(|t| ui::yellow(t))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("{} {}", label("tags"), tags_str);
    println!();
    println!("{content}");

    Ok(())
}

async fn direct_get(id_prefix: &str) -> Result<serde_json::Value> {
    let data_dir = Memory::default_data_dir();
    if !data_dir.exists() {
        bail!("No memories stored. Run `memori init` first.");
    }
    let memory = Memory::open(&data_dir).await?;
    let (records, _) = memory.list(100, None, vec![], None).await?;
    let prefix = id_prefix.to_lowercase();
    let matches: Vec<_> = records
        .iter()
        .filter(|r| r.id.to_string().starts_with(&prefix))
        .collect();
    match matches.len() {
        0 => bail!("No memory found with id starting with '{id_prefix}'"),
        n if n > 1 => bail!("Ambiguous prefix '{id_prefix}' matches {n} records"),
        _ => {}
    }
    let r = matches[0];
    Ok(serde_json::json!({
        "id": r.id,
        "content": r.content,
        "created_at": r.created_at.to_rfc3339(),
        "tags": r.tags,
        "source": r.source,
    }))
}

fn fmt_created(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|_| rfc3339.to_string())
}
