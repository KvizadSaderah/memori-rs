use anyhow::{Context, Result, bail};
use memori_core::Memory;
use uuid::Uuid;

use crate::cli::ui;
use crate::ipc::{client, proto::Request};

pub async fn run(id_prefix: String) -> Result<()> {
    // 1. Resolve the record (full id + current content).
    let record = if let Some(data) = client::call(&Request::Get {
        id_prefix: id_prefix.clone(),
    })
    .await?
    {
        data
    } else {
        direct_get(&id_prefix).await?
    };

    let full_id = record["id"]
        .as_str()
        .context("record has no id")?
        .to_string();
    let original = record["content"].as_str().unwrap_or("").to_string();

    // 2. Open the user's editor on the content.
    let edited = edit_in_editor(&original)?;

    if edited.trim() == original.trim() {
        println!("No changes — nothing to update.");
        return Ok(());
    }
    if edited.trim().is_empty() {
        bail!("Refusing to save empty content. Use `memori forget` to delete a memory.");
    }

    // 3. Persist via the server when running, else directly.
    let req = Request::Update {
        id: full_id.clone(),
        content: edited.clone(),
    };
    if client::call(&req).await?.is_some() {
        // ok
    } else {
        let data_dir = Memory::default_data_dir();
        let memory = Memory::open(&data_dir).await?;
        let uuid = full_id
            .parse::<Uuid>()
            .context("stored id is not a valid UUID")?;
        memory.update(uuid, edited).await?;
    }

    let short = &full_id[..full_id.len().min(8)];
    println!("{} {}", ui::green("Updated"), ui::cyan(short));
    Ok(())
}

/// Write `content` to a temp file, open `$EDITOR` (or `$VISUAL`, falling back to
/// `vi`), wait for it to close, then read the result back.
fn edit_in_editor(content: &str) -> Result<String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let path = std::env::temp_dir().join(format!("memori-edit-{}.md", Uuid::new_v4()));
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write temp file {}", path.display()))?;

    // Editors like "code --wait" come as a command + args.
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let args: Vec<&str> = parts.collect();

    let status = std::process::Command::new(program)
        .args(&args)
        .arg(&path)
        .status()
        .with_context(|| format!("failed to launch editor '{editor}'"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&path);
        bail!("editor exited with a non-zero status; aborting");
    }

    let edited = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read back temp file {}", path.display()))?;
    let _ = std::fs::remove_file(&path);
    Ok(edited)
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
    }))
}
