use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "obsidian-kb")]
#[command(about = "Local-first hybrid retrieval for Obsidian vaults")]
pub struct Cli {
    #[arg(long, global = true, help = "Path to an obsidian-kb config file")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init(InitArgs),
    Index(IndexArgs),
    Search(SearchArgs),
    Show(ShowArgs),
    Graph(GraphArgs),
    Stats(StatsArgs),
    Tags(TagsArgs),
    Properties(PropertiesArgs),
    Doctor(DoctorArgs),
    Mcp(McpArgs),
    #[command(about = "Print version information")]
    Version,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long, id = "init-vault", value_name = "VAULT")]
    pub vault: Option<PathBuf>,

    #[arg(value_name = "VAULT", hide = true)]
    pub legacy_vault: Option<PathBuf>,

    #[arg(
        long,
        help = "Sidecar index directory; defaults to <vault>/.obsidian-kb"
    )]
    pub index_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct IndexArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, help = "Rebuild all local indexes")]
    pub rebuild: bool,

    #[arg(
        long,
        conflicts_with = "rebuild",
        help = "Deprecated: regular indexing refreshes SQLite/Tantivy and reuses unchanged embeddings"
    )]
    pub changed_only: bool,

    #[arg(long, help = "Skip local vector embedding rebuild")]
    pub no_embeddings: bool,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    #[arg(value_name = "QUERY", num_args = 0..)]
    pub query: Vec<String>,

    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, value_enum)]
    pub mode: Option<SearchModeArg>,

    #[arg(long, alias = "limit", default_value_t = 10)]
    pub top: usize,

    #[arg(
        long = "tag",
        value_name = "TAG",
        help = "Require an indexed tag; repeat for AND filters"
    )]
    pub tags: Vec<String>,

    #[arg(
        long = "property",
        value_name = "FILTER",
        help = "Require an indexed frontmatter property filter such as KEY=VALUE, KEY!=VALUE, or KEY>=VALUE; repeat for AND filters"
    )]
    pub properties: Vec<String>,

    #[arg(
        long = "expand-graph",
        alias = "graph",
        help = "Expand results through linked notes/backlinks using configured graph depth"
    )]
    pub expand_graph: bool,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,

    #[arg(
        long,
        requires = "json",
        help = "Include compact chunk text in JSON output"
    )]
    pub include_text: bool,

    #[arg(
        long,
        default_value_t = 1200,
        requires = "include_text",
        value_name = "CHARS",
        help = "Maximum characters per included chunk text; use 0 for full chunk text"
    )]
    pub max_chars: usize,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    pub chunk_id: String,

    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct GraphArgs {
    pub note: String,

    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, default_value_t = 1)]
    pub depth: usize,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StatsArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TagsArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, alias = "limit", default_value_t = 50)]
    pub top: usize,

    #[arg(long, value_name = "PREFIX", help = "Only list tags with this prefix")]
    pub prefix: Option<String>,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct PropertiesArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, alias = "limit", default_value_t = 50)]
    pub top: usize,

    #[arg(long, value_name = "KEY", help = "List values for one property key")]
    pub key: Option<String>,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct McpArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(
        long,
        value_name = "SECONDS",
        help = "Override the MCP vector model idle unload timeout; 0 disables auto-unload"
    )]
    pub idle_unload_seconds: Option<u64>,

    #[arg(
        long,
        help = "Initialize the local embedding model when the MCP server starts"
    )]
    pub preload_embedder: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SearchModeArg {
    Bm25,
    Vector,
    Hybrid,
}
