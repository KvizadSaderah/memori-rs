use std::path::Path;

use anyhow::Result;
use memori_core::{Memory, Query};
use serde::Serialize;
use uuid::Uuid;

use crate::ipc::{client, proto::Request};

#[derive(Serialize)]
struct Check {
    check: &'static str,
    status: &'static str,
    message: String,
    fix: Option<String>,
}

pub async fn run(json: bool) -> Result<()> {
    let mut checks: Vec<Check> = Vec::new();
    let data_dir = Memory::default_data_dir();

    // 1. Data directory
    if !data_dir.exists() {
        checks.push(Check {
            check: "data_dir",
            status: "fail",
            message: format!("{} not found", data_dir.display()),
            fix: Some("Run `memori init` to create the data directory".into()),
        });
    } else {
        let tmp = data_dir.join(".write_test");
        match std::fs::write(&tmp, b"ok") {
            Ok(_) => {
                let _ = std::fs::remove_file(&tmp);
                checks.push(Check {
                    check: "data_dir",
                    status: "pass",
                    message: format!("{} OK", data_dir.display()),
                    fix: None,
                });
            }
            Err(e) => {
                checks.push(Check {
                    check: "data_dir",
                    status: "fail",
                    message: format!("{} not writable: {e}", data_dir.display()),
                    fix: Some("Check directory permissions".into()),
                });
            }
        }
    }

    // 2. Embedding model
    {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
        match TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15).with_show_download_progress(false),
        ) {
            Ok(_) => checks.push(Check {
                check: "embedding_model",
                status: "pass",
                message: "BGE-small-en-v1.5 OK".into(),
                fix: None,
            }),
            Err(e) => checks.push(Check {
                check: "embedding_model",
                status: "fail",
                message: format!("model not available: {e}"),
                fix: Some("Re-install memori-rs (the model is bundled)".into()),
            }),
        }
    }

    // 3. End-to-end roundtrip (store → recall → forget).
    // Only meaningful if both the data dir and the embedding model are healthy;
    // skip otherwise so we don't pile a confusing second failure on top.
    let stack_healthy = checks.iter().all(|c| c.status != "fail");
    if stack_healthy {
        checks.push(roundtrip_check(&data_dir).await);
    } else {
        checks.push(Check {
            check: "roundtrip",
            status: "warn",
            message: "skipped (fix the failures above first)".into(),
            fix: None,
        });
    }

    // 4. MCP client integrations
    let binary = std::env::current_exe().unwrap_or_else(|_| "memori".into());
    let clients = [
        ("Claude Desktop", claude_desktop_config_path()),
        ("Claude Code", claude_code_config_path()),
        ("Cursor", cursor_config_path()),
        ("Continue.dev", continue_config_path()),
    ];
    for (name, path_opt) in &clients {
        let Some(path) = path_opt else { continue };
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(path).unwrap_or_default();
        let integrated = raw.contains("\"memori\"");
        checks.push(Check {
            check: "client_integration",
            status: if integrated { "pass" } else { "warn" },
            message: format!("{name}: {}", path.display()),
            fix: if integrated {
                None
            } else {
                Some(format!(
                    "Run `memori init` or add manually: \"memori\": {{\"command\": \"{}\", \"args\": [\"mcp\"]}}",
                    binary.display()
                ))
            },
        });
    }

    let any_fail = checks.iter().any(|c| c.status == "fail");

    if json {
        println!("{}", serde_json::to_string_pretty(&checks)?);
    } else {
        for c in &checks {
            let icon = match c.status {
                "pass" => "✓",
                "fail" => "✗",
                _ => "!",
            };
            println!("{icon} [{:20}] {}", c.check, c.message);
            if let Some(fix) = &c.fix {
                println!("  Fix: {fix}");
            }
        }
    }

    if any_fail {
        std::process::exit(1);
    }
    Ok(())
}

/// Prove the full read/write path works: store a uniquely-tagged probe memory,
/// recall it back, then delete it. Goes through the running MCP server over IPC
/// when one is up (the server owns LanceDB exclusively); otherwise opens the
/// store directly. The probe is always cleaned up, even on a mismatch.
async fn roundtrip_check(data_dir: &Path) -> Check {
    let probe_id = Uuid::new_v4();
    let marker = format!("memori doctor self-test probe {probe_id}");
    let via_server = client::send(&Request::List {
        limit: 1,
        cursor: None,
        tags: vec![],
        source: None,
    })
    .await
    .ok()
    .flatten()
    .is_some();

    let result = if via_server {
        roundtrip_ipc(&marker).await
    } else {
        roundtrip_direct(data_dir, &marker).await
    };

    let via = if via_server {
        "via running server"
    } else {
        "direct"
    };
    match result {
        Ok(()) => Check {
            check: "roundtrip",
            status: "pass",
            message: format!("store → recall → forget OK ({via})"),
            fix: None,
        },
        Err(e) => Check {
            check: "roundtrip",
            status: "fail",
            message: format!("store/recall/forget failed ({via}): {e}"),
            fix: Some("Check disk space and that the data dir isn't corrupt".into()),
        },
    }
}

async fn roundtrip_ipc(marker: &str) -> Result<()> {
    let stored = client::call(&Request::Store {
        content: marker.to_string(),
        tags: vec!["memori-doctor".into()],
        source: Some("memori-doctor".into()),
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("server stopped responding"))?;
    let id = stored["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("store returned no id"))?
        .to_string();

    // Recall, then forget unconditionally so the probe never lingers.
    let recall = client::call(&Request::Recall {
        query: marker.to_string(),
        top_k: 1,
        tags: vec![],
    })
    .await;
    let _ = client::call(&Request::Forget {
        id: Some(id.clone()),
        tags: vec![],
        source: None,
        older_than: None,
    })
    .await;

    let data = recall?.ok_or_else(|| anyhow::anyhow!("server stopped responding"))?;
    verify_recall(data["results"].as_array(), &id)
}

async fn roundtrip_direct(data_dir: &Path, marker: &str) -> Result<()> {
    let memory = Memory::open(data_dir).await?;
    let record = memory
        .store(
            marker.to_string(),
            vec!["memori-doctor".into()],
            Some("memori-doctor".into()),
        )
        .await?;
    let id = record.id;

    let recall = memory
        .recall(Query {
            text: marker.to_string(),
            top_k: 1,
            tag_filter: vec![],
            source_filter: None,
        })
        .await;
    let _ = memory.forget(Some(id), None).await;

    let results = recall?;
    let top = results.first().map(|r| r.record.id.to_string());
    match top {
        Some(found) if found == id.to_string() => Ok(()),
        Some(other) => anyhow::bail!("recall returned a different memory ({other})"),
        None => anyhow::bail!("recall returned no results"),
    }
}

fn verify_recall(results: Option<&Vec<serde_json::Value>>, expected_id: &str) -> Result<()> {
    let top = results
        .and_then(|r| r.first())
        .and_then(|r| r["id"].as_str());
    match top {
        Some(found) if found == expected_id => Ok(()),
        Some(other) => anyhow::bail!("recall returned a different memory ({other})"),
        None => anyhow::bail!("recall returned no results"),
    }
}

fn claude_desktop_config_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    return dirs::home_dir()
        .map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"));
    #[cfg(target_os = "windows")]
    return dirs::data_dir().map(|d| d.join("Claude/claude_desktop_config.json"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return dirs::config_dir().map(|c| c.join("Claude/claude_desktop_config.json"));
}

fn claude_code_config_path() -> Option<std::path::PathBuf> {
    // Claude Code reads MCP servers from ~/.claude.json (NOT ~/.claude/settings.json).
    // Must match the path `memori init` writes to (see cli/init.rs).
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

fn cursor_config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".cursor/mcp.json"))
}

fn continue_config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".continue/config.json"))
}
