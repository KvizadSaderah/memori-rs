/// Internal IPC protocol between `memori mcp` (server) and CLI commands (client).
///
/// Transport: Unix domain socket at `{data_dir}/memori.sock`.
/// Framing: newline-delimited JSON — one Request line, one Response line per connection.
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    List {
        limit: usize,
        cursor: Option<String>,
        tags: Vec<String>,
        source: Option<String>,
    },
    Get {
        id_prefix: String,
    },
    Recall {
        query: String,
        top_k: usize,
        tags: Vec<String>,
    },
    Store {
        content: String,
        tags: Vec<String>,
        source: Option<String>,
    },
    Update {
        id: String,
        content: String,
    },
    Forget {
        id: Option<String>,
        tags: Vec<String>,
        source: Option<String>,
        /// RFC-3339 timestamp — delete memories older than this
        older_than: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { data: serde_json::Value },
    Err { message: String },
}

impl Response {
    pub fn ok(data: impl Serialize) -> Self {
        Self::Ok {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self::Err {
            message: msg.into(),
        }
    }
}

/// Canonical socket path given the data directory.
pub fn socket_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("memori.sock")
}
