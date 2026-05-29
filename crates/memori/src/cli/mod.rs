pub mod doctor;
pub mod dump;
pub mod edit;
pub mod forget;
pub mod init;
pub mod recall;
pub mod show;
pub mod ui;

pub fn parse_duration_ago(s: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
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

#[cfg(test)]
mod tests {
    use super::parse_duration_ago;

    #[test]
    fn parses_days_and_hours() {
        let now = chrono::Utc::now();
        let d = parse_duration_ago("7d").unwrap();
        let h = parse_duration_ago("24h").unwrap();
        // 7d and 24h cutoffs should be in the past, 7d older than 24h.
        assert!(d < now && h < now);
        assert!(d < h, "7d ago must be earlier than 24h ago");
        // ~7 days in seconds, allow a small clock delta.
        let delta = (now - d).num_seconds();
        assert!((604_700..=604_900).contains(&delta), "delta was {delta}");
    }

    #[test]
    fn trims_whitespace() {
        assert!(parse_duration_ago("  3d  ").is_ok());
    }

    #[test]
    fn rejects_bad_formats() {
        for bad in ["", "7", "d", "7x", "abc", "-3d", "7days", "1.5h"] {
            assert!(parse_duration_ago(bad).is_err(), "{bad} should be rejected");
        }
    }
}
