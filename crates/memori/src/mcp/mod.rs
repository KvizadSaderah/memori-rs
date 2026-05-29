// The `#[tool(tool_box)]` proc-macro generates code that references structs and
// functions in `server`, but rustc's dead-code pass doesn't see through macro
// expansion — suppress the false positives here.
#[allow(dead_code, unused_imports)]
pub mod server;

pub(super) fn parse_duration_ago(s: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    let s = s.trim();
    let (num_str, unit) = if let Some(stripped) = s.strip_suffix('d') {
        (stripped, 'd')
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 'h')
    } else {
        return Err(format!("--older-than format must be NNd or NNh, got: {s}"));
    };
    let n: u64 = num_str
        .parse()
        .map_err(|_| format!("--older-than format must be NNd or NNh, got: {s}"))?;
    let secs = match unit {
        'd' => n * 86400,
        'h' => n * 3600,
        _ => unreachable!(),
    };
    Ok(chrono::Utc::now() - chrono::Duration::seconds(secs as i64))
}
