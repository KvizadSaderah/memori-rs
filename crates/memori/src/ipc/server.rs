use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use memori_core::{ForgetFilter, Memory, Query};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use super::proto::{Request, Response, socket_path};

/// Start the IPC server on the default socket path. Runs forever.
pub async fn run(memory: Arc<Memory>) -> Result<()> {
    let sock_path = socket_path(&Memory::default_data_dir());
    serve_on(memory, &sock_path).await
}

/// Start the IPC server bound to an explicit socket path. Runs forever; cancel
/// by dropping the task. Used by `run` and by integration tests.
pub async fn serve_on(memory: Arc<Memory>, sock_path: &Path) -> Result<()> {
    // Remove stale socket from a previous run
    let _ = std::fs::remove_file(sock_path);

    let listener = UnixListener::bind(sock_path)?;
    tracing::info!("IPC socket listening at {}", sock_path.display());

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                // Transient accept errors should not kill the server.
                tracing::warn!("IPC accept error: {e}");
                continue;
            }
        };
        let mem = Arc::clone(&memory);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, mem).await {
                tracing::warn!("IPC connection error: {e}");
            }
        });
    }
}

async fn handle_connection(stream: tokio::net::UnixStream, memory: Arc<Memory>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // One request per connection
    if let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => dispatch(req, &memory).await,
            Err(e) => Response::err(format!("parse error: {e}")),
        };
        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        writer.write_all(out.as_bytes()).await?;
    }

    Ok(())
}

async fn dispatch(req: Request, memory: &Memory) -> Response {
    match req {
        Request::List {
            limit,
            cursor,
            tags,
            source,
        } => {
            let lim = if limit == 0 { 50 } else { limit };
            match memory.list(lim, cursor.as_deref(), tags, source).await {
                Ok((records, next_cursor)) => {
                    let items: Vec<_> = records
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
                    Response::ok(serde_json::json!({ "items": items, "next_cursor": next_cursor }))
                }
                Err(e) => Response::err(e.to_string()),
            }
        }

        Request::Get { id_prefix } => {
            // List all and find by prefix — simple enough for MVP
            match memory.list(100, None, vec![], None).await {
                Ok((records, _)) => {
                    let prefix = id_prefix.to_lowercase();
                    let matches: Vec<_> = records
                        .iter()
                        .filter(|r| r.id.to_string().starts_with(&prefix))
                        .collect();
                    match matches.len() {
                        0 => Response::err(format!("no memory with id prefix '{id_prefix}'")),
                        n if n > 1 => Response::err(format!(
                            "ambiguous prefix '{id_prefix}' matches {n} records"
                        )),
                        _ => {
                            let r = matches[0];
                            Response::ok(serde_json::json!({
                                "id": r.id,
                                "content": r.content,
                                "created_at": r.created_at.to_rfc3339(),
                                "tags": r.tags,
                                "source": r.source,
                            }))
                        }
                    }
                }
                Err(e) => Response::err(e.to_string()),
            }
        }

        Request::Recall { query, top_k, tags } => {
            let q = Query {
                text: query,
                top_k: top_k.clamp(1, 25),
                tag_filter: tags,
                source_filter: None,
            };
            match memory.recall(q).await {
                Ok(results) => {
                    let items: Vec<_> = results
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
                        .collect();
                    Response::ok(serde_json::json!({ "results": items }))
                }
                Err(e) => Response::err(e.to_string()),
            }
        }

        Request::Update { id, content } => {
            let uuid = match id.parse::<uuid::Uuid>() {
                Ok(u) => u,
                Err(_) => return Response::err("id is not a valid UUID"),
            };
            match memory.update(uuid, content).await {
                Ok(r) => Response::ok(serde_json::json!({
                    "id": r.id,
                    "created_at": r.created_at.to_rfc3339(),
                })),
                Err(e) => Response::err(e.to_string()),
            }
        }

        Request::Store {
            content,
            tags,
            source,
        } => match memory.store(content, tags, source).await {
            Ok(r) => Response::ok(serde_json::json!({
                "id": r.id,
                "created_at": r.created_at.to_rfc3339(),
            })),
            Err(e) => Response::err(e.to_string()),
        },

        Request::Forget {
            id,
            tags,
            source,
            older_than,
        } => {
            let uuid = if let Some(ref s) = id {
                match s.parse::<uuid::Uuid>() {
                    Ok(u) => Some(u),
                    Err(_) => return Response::err("id is not a valid UUID"),
                }
            } else {
                None
            };

            let filter = if uuid.is_none() {
                let older_than_dt = if let Some(ref s) = older_than {
                    match chrono::DateTime::parse_from_rfc3339(s) {
                        Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
                        Err(_) => {
                            return Response::err("older_than is not a valid RFC-3339 timestamp");
                        }
                    }
                } else {
                    None
                };
                Some(ForgetFilter {
                    older_than: older_than_dt,
                    tags,
                    source,
                })
            } else {
                None
            };

            match memory.forget(uuid, filter).await {
                Ok(n) => Response::ok(serde_json::json!({ "deleted_count": n })),
                Err(e) => Response::err(e.to_string()),
            }
        }
    }
}
