use anyhow::Result;
use memori_core::Memory;
use std::path::{Path, PathBuf};

struct ClientDef {
    name: &'static str,
    config_path: fn() -> Option<PathBuf>,
    mcp_key: &'static str,
}

fn claude_desktop_config() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join("Claude/claude_desktop_config.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|c| c.join("Claude/claude_desktop_config.json"))
    }
}

fn claude_code_config() -> Option<PathBuf> {
    // Claude Code reads MCP servers from the top-level `mcpServers` key in
    // ~/.claude.json — NOT ~/.claude/settings.json.
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

fn cursor_config() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cursor/mcp.json"))
}

fn continue_config() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".continue/config.json"))
}

const CLIENTS: &[ClientDef] = &[
    ClientDef {
        name: "Claude Desktop",
        config_path: claude_desktop_config,
        mcp_key: "mcpServers",
    },
    ClientDef {
        name: "Claude Code",
        config_path: claude_code_config,
        mcp_key: "mcpServers",
    },
    ClientDef {
        name: "Cursor",
        config_path: cursor_config,
        mcp_key: "mcpServers",
    },
    ClientDef {
        name: "Continue.dev",
        config_path: continue_config,
        mcp_key: "mcpServers",
    },
];

pub async fn run(dry_run: bool) -> Result<()> {
    let data_dir = Memory::default_data_dir();

    // Ensure data directory
    if data_dir.exists() {
        println!("  data dir: {} (already exists)", data_dir.display());
    } else if dry_run {
        println!("  data dir: {} (would create)", data_dir.display());
    } else {
        std::fs::create_dir_all(&data_dir)?;
        println!("  data dir: {} (created)", data_dir.display());
    }

    // Get binary path
    let binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("memori"));

    let mut integrated = 0usize;
    println!("\n  {:20} {:6}  CONFIG", "CLIENT", "STATUS");

    for client in CLIENTS {
        let Some(config_path) = (client.config_path)() else {
            println!("  {:20} {:6}  (unsupported on this OS)", client.name, "—");
            continue;
        };

        if !config_path.exists() {
            println!("  {:20} {:6}  {}", client.name, "—", config_path.display());
            continue;
        }

        match write_config(&config_path, &binary, client.mcp_key, dry_run) {
            Ok(already) => {
                let status = if dry_run {
                    "DRY"
                } else if already {
                    "SKIP"
                } else {
                    "✓"
                };
                println!(
                    "  {:20} {:6}  {}",
                    client.name,
                    status,
                    config_path.display()
                );
                integrated += 1;
            }
            Err(e) => {
                println!(
                    "  {:20} {:6}  {} — {e}",
                    client.name,
                    "✗",
                    config_path.display()
                );
            }
        }
    }

    if integrated == 0 && !dry_run {
        eprintln!("\nNo clients integrated. Add manually:\n");
        eprintln!(
            r#"  "mcpServers": {{ "memori": {{ "command": "{}", "args": ["mcp"] }} }}"#,
            binary.display()
        );
        std::process::exit(1);
    }

    Ok(())
}

fn write_config(path: &Path, binary: &Path, key: &str, dry_run: bool) -> Result<bool> {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".into());
    let mut json: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::json!({}));

    let mcp_servers = json
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("config is not a JSON object"))?
        .entry(key)
        .or_insert_with(|| serde_json::json!({}));

    if mcp_servers.get("memori").is_some() {
        return Ok(true); // already present
    }

    mcp_servers["memori"] = serde_json::json!({
        "command": binary.to_string_lossy().as_ref(),
        "args": ["mcp"]
    });

    if !dry_run {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&json)?)?;
        std::fs::rename(&tmp, path)?;
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_targets_dot_claude_json() {
        // Regression: Claude Code reads ~/.claude.json, NOT ~/.claude/settings.json.
        let p = claude_code_config().unwrap();
        assert!(
            p.ends_with(".claude.json"),
            "claude code config should be ~/.claude.json, got {}",
            p.display()
        );
    }

    #[test]
    fn write_config_adds_memori_and_preserves_existing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "theme": "dark", "mcpServers": { "other": { "command": "x" } } }"#,
        )
        .unwrap();

        let added = write_config(&path, Path::new("/bin/memori"), "mcpServers", false).unwrap();
        assert!(!added, "first write returns false (newly added)");

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // unrelated keys preserved
        assert_eq!(json["theme"], "dark");
        assert_eq!(json["mcpServers"]["other"]["command"], "x");
        // memori added correctly
        assert_eq!(json["mcpServers"]["memori"]["command"], "/bin/memori");
        assert_eq!(json["mcpServers"]["memori"]["args"][0], "mcp");
    }

    #[test]
    fn write_config_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();

        assert!(!write_config(&path, Path::new("/bin/memori"), "mcpServers", false).unwrap());
        // second call: memori already present -> Ok(true) = SKIP
        assert!(write_config(&path, Path::new("/bin/memori"), "mcpServers", false).unwrap());
    }

    #[test]
    fn write_config_dry_run_does_not_touch_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();

        let _ = write_config(&path, Path::new("/bin/memori"), "mcpServers", true).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{}",
            "dry run must not write"
        );
    }

    #[test]
    fn write_config_handles_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let added = write_config(&path, Path::new("/bin/memori"), "mcpServers", false).unwrap();
        assert!(!added);
        assert!(path.exists());
    }
}
