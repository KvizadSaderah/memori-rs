use anyhow::Result;
use memori_core::{ForgetFilter, MemoriError, Memory, Query};

use super::parse_duration_ago;
use rmcp::{
    Error as McpError, ServerHandler, ServiceExt, handler::server::tool::ToolCallContext, model::*,
    schemars, tool,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct MemoriServer {
    memory: Arc<Memory>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StoreArgs {
    /// The textual memory to persist
    pub content: String,
    /// Optional tags for filtering
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional source agent identifier
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecallArgs {
    /// Free-text query to search memories
    pub query: String,
    /// Number of results to return (default 5, max 25)
    pub top_k: Option<usize>,
    /// Filter results to memories with these tags
    #[serde(default)]
    pub tag_filter: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Max results per page (default 20, max 100)
    pub limit: Option<usize>,
    /// Pagination cursor from previous response
    pub cursor: Option<String>,
    #[serde(default)]
    pub tag_filter: Vec<String>,
    pub source_filter: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs {
    /// Delete a specific memory by UUID
    pub id: Option<String>,
    /// Delete memories older than this (e.g. "7d", "24h")
    pub older_than: Option<String>,
    /// Delete memories with these tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Delete memories from this source
    pub source: Option<String>,
}

fn to_mcp_err(e: MemoriError) -> McpError {
    match e {
        MemoriError::InvalidInput(msg) => McpError::invalid_params(msg, None),
        other => McpError::internal_error(other.to_string(), None),
    }
}

fn text_result(text: impl Serialize) -> Result<CallToolResult, McpError> {
    let json =
        serde_json::to_string(&text).map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

#[tool(tool_box)]
impl MemoriServer {
    #[tool(description = "Persist a textual memory for future recall")]
    async fn store(&self, #[tool(aggr)] args: StoreArgs) -> Result<CallToolResult, McpError> {
        let record = self
            .memory
            .store(args.content, args.tags, args.source)
            .await
            .map_err(to_mcp_err)?;

        #[derive(Serialize)]
        struct Out {
            id: String,
            created_at: String,
        }
        text_result(Out {
            id: record.id.to_string(),
            created_at: record.created_at.to_rfc3339(),
        })
    }

    #[tool(description = "Recall memories relevant to a query, ranked by semantic similarity")]
    async fn recall(&self, #[tool(aggr)] args: RecallArgs) -> Result<CallToolResult, McpError> {
        let query = Query {
            text: args.query,
            top_k: args.top_k.unwrap_or(5).clamp(1, 25),
            tag_filter: args.tag_filter,
            source_filter: None,
        };
        let results = self.memory.recall(query).await.map_err(to_mcp_err)?;

        #[derive(Serialize)]
        struct Item {
            id: String,
            content: String,
            score: f32,
            created_at: String,
            tags: Vec<String>,
            source: Option<String>,
        }
        #[derive(Serialize)]
        struct Out {
            results: Vec<Item>,
        }
        text_result(Out {
            results: results
                .into_iter()
                .map(|r| Item {
                    id: r.record.id.to_string(),
                    content: r.record.content,
                    score: r.score,
                    created_at: r.record.created_at.to_rfc3339(),
                    tags: r.record.tags,
                    source: r.record.source,
                })
                .collect(),
        })
    }

    #[tool(description = "List stored memories with optional filters and cursor-based pagination")]
    async fn list(&self, #[tool(aggr)] args: ListArgs) -> Result<CallToolResult, McpError> {
        let (records, next_cursor) = self
            .memory
            .list(
                args.limit.unwrap_or(20),
                args.cursor.as_deref(),
                args.tag_filter,
                args.source_filter,
            )
            .await
            .map_err(to_mcp_err)?;

        #[derive(Serialize)]
        struct Item {
            id: String,
            content: String,
            created_at: String,
            tags: Vec<String>,
            source: Option<String>,
        }
        #[derive(Serialize)]
        struct Out {
            items: Vec<Item>,
            next_cursor: Option<String>,
        }
        text_result(Out {
            items: records
                .into_iter()
                .map(|r| Item {
                    id: r.id.to_string(),
                    content: r.content,
                    created_at: r.created_at.to_rfc3339(),
                    tags: r.tags,
                    source: r.source,
                })
                .collect(),
            next_cursor,
        })
    }

    #[tool(
        description = "Delete memories by exact UUID or by filter (older_than, tags, source). Supply id OR filter criteria, not both."
    )]
    async fn forget(&self, #[tool(aggr)] args: ForgetArgs) -> Result<CallToolResult, McpError> {
        let has_id = args.id.is_some();
        let has_filter =
            args.older_than.is_some() || !args.tags.is_empty() || args.source.is_some();

        let (id, filter) = match (has_id, has_filter) {
            (false, false) => {
                return Err(McpError::invalid_params("must supply id or filter", None));
            }
            (true, true) => {
                return Err(McpError::invalid_params(
                    "supply id or filter, not both",
                    None,
                ));
            }
            (true, false) => {
                let id_str = args.id.unwrap();
                let id = id_str
                    .parse::<Uuid>()
                    .map_err(|_| McpError::invalid_params("id is not a valid UUID", None))?;
                (Some(id), None)
            }
            (false, true) => {
                let older_than = if let Some(s) = args.older_than {
                    Some(parse_duration_ago(&s).map_err(|e| McpError::invalid_params(e, None))?)
                } else {
                    None
                };
                (
                    None,
                    Some(ForgetFilter {
                        older_than,
                        tags: args.tags,
                        source: args.source,
                    }),
                )
            }
        };

        let deleted_count = self.memory.forget(id, filter).await.map_err(to_mcp_err)?;

        #[derive(Serialize)]
        struct Out {
            deleted_count: u64,
        }
        text_result(Out { deleted_count })
    }
}

// Four tools registered: store, recall, list, forget (4 of 5 budget per DESIGN §2.2)

impl ServerHandler for MemoriServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "memori-rs".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "Use memori.store to persist memories and memori.recall to retrieve them by semantic similarity.".into(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: PaginatedRequestParam,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            next_cursor: None,
            tools: Self::tool_box().list(),
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ToolCallContext::new(self, request, context);
        Self::tool_box().call(ctx).await
    }
}

pub async fn run() -> anyhow::Result<()> {
    let data_dir = Memory::default_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    let memory = Arc::new(Memory::open(&data_dir).await.map_err(|e| {
        eprintln!("memori: failed to open storage: {e}");
        e
    })?);

    // Start IPC socket server (for CLI commands) in background
    let ipc_mem = Arc::clone(&memory);
    tokio::spawn(async move {
        if let Err(e) = crate::ipc::server::run(ipc_mem).await {
            tracing::warn!("IPC server error: {e}");
        }
    });

    let server = MemoriServer { memory };

    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
