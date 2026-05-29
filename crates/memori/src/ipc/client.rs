use std::path::Path;

use anyhow::{Result, bail};
use memori_core::Memory;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::proto::{Request, Response, socket_path};

/// Send a request to a server listening at `sock`.
/// Returns `None` if the socket is absent or unreachable (server not running).
pub async fn send_to(req: &Request, sock: &Path) -> Result<Option<Response>> {
    if !sock.exists() {
        return Ok(None);
    }

    // Socket file may be stale (server crashed without cleanup). Treat any
    // connect failure as "server not running" and fall back to direct access.
    let stream = match UnixStream::connect(sock).await {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let (reader, mut writer) = stream.into_split();

    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.shutdown().await?;

    let mut response_line = String::new();
    BufReader::new(reader).read_line(&mut response_line).await?;

    let response: Response = serde_json::from_str(response_line.trim())?;
    Ok(Some(response))
}

/// Send a request to the running MCP server via the default socket path.
/// Returns `None` if the socket doesn't exist (server not running).
pub async fn send(req: &Request) -> Result<Option<Response>> {
    let sock = socket_path(&Memory::default_data_dir());
    send_to(req, &sock).await
}

/// Convenience: send and unwrap — bail if server returned an error.
pub async fn call(req: &Request) -> Result<Option<serde_json::Value>> {
    match send(req).await? {
        None => Ok(None), // server not running
        Some(Response::Ok { data }) => Ok(Some(data)),
        Some(Response::Err { message }) => bail!("memori server error: {message}"),
    }
}
