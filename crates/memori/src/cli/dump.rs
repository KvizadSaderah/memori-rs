use anyhow::Result;
use memori_core::Memory;

use crate::cli::ui;
use crate::ipc::{client, proto::Request};

pub async fn run(
    tags: Vec<String>,
    source: Option<String>,
    limit: usize,
    json: bool,
    full: bool,
    md: bool,
) -> Result<()> {
    let lim = if limit == 0 { 50 } else { limit };

    let req = Request::List {
        limit: lim,
        cursor: None,
        tags: tags.clone(),
        source: source.clone(),
    };

    // Try IPC first (MCP server running) — fallback to direct LanceDB
    let (items, next_cursor) = if let Some(data) = client::call(&req).await? {
        let items = data["items"].as_array().cloned().unwrap_or_default();
        let next_cursor = data["next_cursor"].as_str().map(|s| s.to_string());
        (items, next_cursor)
    } else {
        direct_list(lim, tags, source).await?
    };

    if items.is_empty() {
        println!("No memories stored.");
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if md {
        // Plain Markdown — pipe into a file or an Obsidian vault. No ANSI, no
        // width clamping, full content. Stable and copy-paste friendly.
        for r in &items {
            let id = r["id"].as_str().unwrap_or("-");
            let created = r["created_at"].as_str().unwrap_or("-");
            let source_val = r["source"].as_str();
            let tags_val: Vec<&str> = r["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let content = r["content"].as_str().unwrap_or("");

            println!("## {}", &id[..id.len().min(8)]);
            println!();
            println!("- **id**: `{id}`");
            println!("- **created**: {}", fmt_created(created));
            if let Some(s) = source_val {
                println!("- **source**: {s}");
            }
            if !tags_val.is_empty() {
                println!(
                    "- **tags**: {}",
                    tags_val
                        .iter()
                        .map(|t| format!("#{t}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            println!();
            println!("{content}");
            println!();
            println!("---");
            println!();
        }
        return Ok(());
    }

    // Clamp to a readable measure: text past ~100 cols is hard to read even on
    // a very wide terminal.
    let term_width = terminal_width().unwrap_or(100);

    if full {
        // Plain, Obsidian-like layout: no box walls, wrapped at a readable width.
        let width = term_width.min(96);
        let rule = ui::dim(&"─".repeat(width));

        for (i, r) in items.iter().enumerate() {
            let id = r["id"].as_str().unwrap_or("-");
            let short_id = &id[..id.len().min(8)];
            let created = r["created_at"].as_str().unwrap_or("-");
            let source_val = r["source"].as_str();
            let tags_val: Vec<&str> = r["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let content = r["content"].as_str().unwrap_or("");

            if i > 0 {
                println!();
            }
            // Header line: id · created · source
            let sep = ui::dim(" · ");
            let mut header = format!(
                "{}{}{}",
                ui::cyan(&ui::bold(short_id)),
                sep,
                ui::dim(&fmt_created(created))
            );
            if let Some(src) = source_val {
                header.push_str(&sep);
                header.push_str(&ui::magenta(src));
            }
            println!("{header}");
            if !tags_val.is_empty() {
                let tags_colored = tags_val
                    .iter()
                    .map(|t| ui::yellow(&format!("#{t}")))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("{tags_colored}");
            }
            println!("{rule}");
            for line in word_wrap(content, width) {
                println!("{line}");
            }
        }
    } else {
        let term_width = term_width.min(120);
        let content_width = term_width.saturating_sub(10 + 2 + 20 + 2 + 20 + 2 + 14 + 2);
        println!(
            "{}",
            ui::bold(&format!(
                "{:<10}  {:<20}  {:<20}  {:<14}  {}",
                "ID", "CREATED", "SOURCE", "TAGS", "CONTENT"
            ))
        );
        println!("{}", ui::dim(&"─".repeat(term_width.min(120))));
        for r in &items {
            let id = r["id"].as_str().unwrap_or("-");
            let short_id = &id[..id.len().min(8)];
            let created = r["created_at"].as_str().unwrap_or("-");
            let src = r["source"].as_str().unwrap_or("-");
            let tags_val: Vec<&str> = r["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let content = r["content"].as_str().unwrap_or("");
            let tags_str = if tags_val.is_empty() {
                "-".to_string()
            } else {
                tags_val.join(",")
            };

            // Pad to column width *before* coloring so ANSI escapes don't break
            // alignment (format-width counts the escape bytes otherwise).
            println!(
                "{}  {}  {}  {}  {}",
                ui::cyan(&format!("{:<10}", short_id)),
                ui::dim(&format!("{:<20}", fmt_created(created))),
                ui::magenta(&format!("{:<20}", truncate(src, 20))),
                ui::yellow(&format!("{:<14}", truncate(&tags_str, 14))),
                truncate(content, content_width.max(20)),
            );
        }
    }

    if next_cursor.is_some() {
        println!("\n  … showing first {lim} results. Use --limit to see more.");
    }

    Ok(())
}

// Fallback: direct LanceDB access when MCP server not running
async fn direct_list(
    limit: usize,
    tags: Vec<String>,
    source: Option<String>,
) -> Result<(Vec<serde_json::Value>, Option<String>)> {
    let data_dir = Memory::default_data_dir();
    if !data_dir.exists() {
        return Ok((vec![], None));
    }
    let memory = Memory::open(&data_dir).await?;
    let (records, next_cursor) = memory.list(limit, None, tags, source).await?;
    let items = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "content": r.content,
                "created_at": r.created_at.to_rfc3339(),
                "tags": r.tags,
                "source": r.source,
            })
        })
        .collect();
    Ok((items, next_cursor))
}

fn fmt_created(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|_| rfc3339[..rfc3339.len().min(16)].to_string())
}

pub(crate) fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= max_width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

pub(crate) fn terminal_width() -> Option<usize> {
    if let Ok(cols) = std::env::var("COLUMNS")
        && let Ok(n) = cols.parse::<usize>()
    {
        return Some(n);
    }
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdout().as_raw_fd();
        let mut ws: WinSize = unsafe { std::mem::zeroed() };
        if unsafe { tiocgwinsz(fd, &mut ws) } == 0 && ws.ws_col > 0 {
            return Some(ws.ws_col as usize);
        }
    }
    None
}

#[cfg(unix)]
#[repr(C)]
struct WinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[cfg(unix)]
unsafe fn tiocgwinsz(fd: i32, ws: *mut WinSize) -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const TIOCGWINSZ: u64 = 0x5413;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const TIOCGWINSZ: u64 = 0x40087468; // macOS / BSD

    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    unsafe { ioctl(fd, TIOCGWINSZ, ws) }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
