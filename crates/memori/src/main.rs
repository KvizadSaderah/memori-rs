use anyhow::Result;
use clap::{Parser, Subcommand};

mod cli;
mod ipc;
mod mcp;

#[derive(Parser)]
#[command(name = "memori", about = "Local-first AI memory layer", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Detect MCP clients and write integration config
    Init {
        #[arg(long, help = "Print changes without writing")]
        dry_run: bool,
    },
    /// Run the MCP stdio server (invoked by AI clients)
    Mcp,
    /// Diagnose installation health
    Doctor {
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// List stored memories
    Dump {
        #[arg(long = "tag", help = "Filter by tag")]
        tags: Vec<String>,
        #[arg(long, help = "Filter by source agent")]
        source: Option<String>,
        #[arg(long, default_value = "50", help = "Max results (≤ 100)")]
        limit: usize,
        #[arg(long, help = "Output as JSON")]
        json: bool,
        #[arg(long, help = "Show full content (default: truncated table)")]
        full: bool,
        #[arg(
            long,
            help = "Output as plain Markdown (for piping to a file / Obsidian)"
        )]
        md: bool,
    },
    /// Semantic search over stored memories
    Recall {
        /// Free-text query
        query: String,
        #[arg(long, default_value = "5", help = "Number of results (1–25)")]
        top_k: usize,
        #[arg(long = "tag", help = "Filter by tag")]
        tags: Vec<String>,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Show one memory by id (prefix is enough, e.g. 'memori show ee66bfb5')
    Show {
        /// Memory id or unique prefix
        id: String,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// Edit a memory's content in $EDITOR (re-embeds in place, keeps id)
    Edit {
        /// Memory id or unique prefix
        id: String,
    },
    /// Delete memories
    Forget {
        #[arg(long, help = "Delete by exact UUID", conflicts_with_all = ["tags", "source", "older_than"])]
        id: Option<String>,
        #[arg(long = "tag", help = "Delete by tag")]
        tags: Vec<String>,
        #[arg(long, help = "Delete by source agent")]
        source: Option<String>,
        #[arg(long, help = "Delete memories older than duration (e.g. 7d, 24h)")]
        older_than: Option<String>,
        #[arg(long, help = "Preview without deleting")]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Init { dry_run } => cli::init::run(dry_run).await,
        Command::Mcp => mcp::server::run().await,
        Command::Doctor { json } => cli::doctor::run(json).await,
        Command::Dump {
            tags,
            source,
            limit,
            json,
            full,
            md,
        } => cli::dump::run(tags, source, limit, json, full, md).await,
        Command::Recall {
            query,
            top_k,
            tags,
            json,
        } => cli::recall::run(query, top_k, tags, json).await,
        Command::Show { id, json } => cli::show::run(id, json).await,
        Command::Edit { id } => cli::edit::run(id).await,
        Command::Forget {
            id,
            tags,
            source,
            older_than,
            dry_run,
        } => cli::forget::run(id, tags, source, older_than, dry_run).await,
    }
}
