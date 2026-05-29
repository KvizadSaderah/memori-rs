use anyhow::Result;
use memori_core::{Memory, Query};

use crate::cli::ui;
use crate::ipc::{client, proto::Request};

pub async fn run(query: String, top_k: usize, tags: Vec<String>, json: bool) -> Result<()> {
    let k = top_k.clamp(1, 25);
    let req = Request::Recall {
        query: query.clone(),
        top_k: k,
        tags: tags.clone(),
    };

    let results = if let Some(data) = client::call(&req).await? {
        data["results"].as_array().cloned().unwrap_or_default()
    } else {
        direct_recall(query, k, tags).await?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    if results.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    let width = super::dump::terminal_width().unwrap_or(100).min(96);
    for (i, r) in results.iter().enumerate() {
        let id = r["id"].as_str().unwrap_or("-");
        let short_id = &id[..id.len().min(8)];
        let score = r["score"].as_f64().unwrap_or(0.0);
        let src = r["source"].as_str();
        let tags_val: Vec<&str> = r["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let content = r["content"].as_str().unwrap_or("");

        if i > 0 {
            println!();
        }
        let sep = ui::dim(" · ");
        let mut header = format!(
            "{} {}",
            ui::green(&format!("{score:.3}")),
            ui::cyan(&ui::bold(short_id)),
        );
        if let Some(s) = src {
            header.push_str(&sep);
            header.push_str(&ui::magenta(s));
        }
        if !tags_val.is_empty() {
            header.push_str(&sep);
            header.push_str(
                &tags_val
                    .iter()
                    .map(|t| ui::yellow(&format!("#{t}")))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        println!("{header}");
        for line in super::dump::word_wrap(content, width) {
            println!("  {line}");
        }
    }

    Ok(())
}

async fn direct_recall(
    query: String,
    top_k: usize,
    tags: Vec<String>,
) -> Result<Vec<serde_json::Value>> {
    let data_dir = Memory::default_data_dir();
    if !data_dir.exists() {
        return Ok(vec![]);
    }
    let memory = Memory::open(&data_dir).await?;
    let results = memory
        .recall(Query {
            text: query,
            top_k,
            tag_filter: tags,
            source_filter: None,
        })
        .await?;
    Ok(results
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.record.id,
                "content": r.record.content,
                "score": r.score,
                "created_at": r.record.created_at.to_rfc3339(),
                "tags": r.record.tags,
                "source": r.record.source,
            })
        })
        .collect())
}
