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
    Doctor(DoctorArgs),
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

    #[arg(long, conflicts_with = "rebuild", help = "Index only changed files")]
    pub changed_only: bool,

    #[arg(long, help = "Skip local vector embedding rebuild")]
    pub no_embeddings: bool,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    #[arg(value_name = "QUERY", num_args = 1..)]
    pub query: Vec<String>,

    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, value_enum)]
    pub mode: Option<SearchModeArg>,

    #[arg(long, alias = "limit", default_value_t = 10)]
    pub top: usize,

    #[arg(
        long = "expand-graph",
        alias = "graph",
        help = "Expand results to directly linked notes/backlinks"
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
pub struct DoctorArgs {
    #[arg(long, help = "Path to the Obsidian vault")]
    pub vault: Option<PathBuf>,

    #[arg(long, help = "Emit machine-readable JSON")]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SearchModeArg {
    Bm25,
    Vector,
    Hybrid,
}
