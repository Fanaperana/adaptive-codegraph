use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "adaptive-codegraph", version, about = "Language-agnostic code graph indexer and search")]
struct Cli {
    /// Path to workspace root (default: current directory)
    #[arg(long, default_value = ".")]
    base: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build or rebuild the full index
    Index,
    /// Incremental reindex (git-aware)
    Reindex,
    /// Search symbols by name/text (BM25)
    Search {
        /// Query string
        query: String,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Semantic/vector search
    #[cfg(feature = "fastembed")]
    SemanticSearch {
        query: String,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Find a symbol by name
    Find {
        /// Symbol name (case-insensitive)
        name: String,
    },
    /// Show callers of a symbol
    Callers {
        name: String,
    },
    /// Show callees of a symbol
    Callees {
        name: String,
    },
    /// BFS neighborhood around a symbol
    Neighborhood {
        name: String,
        #[arg(long, default_value = "2")]
        depth: usize,
        #[arg(long, default_value = "50")]
        cap: usize,
    },
    /// Show index status
    Status,
    /// List detected languages
    Languages,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Index => {
            println!("Full index not yet wired up — core library ready.");
        }
        Commands::Reindex => {
            println!("Incremental reindex not yet wired up — core library ready.");
        }
        Commands::Search { query, limit } => {
            println!("BM25 search for '{}' (limit {}) — core library ready.", query, limit);
        }
        #[cfg(feature = "fastembed")]
        Commands::SemanticSearch { query, limit } => {
            println!("Semantic search for '{}' (limit {}) — core library ready.", query, limit);
        }
        Commands::Find { name } => {
            println!("Find '{}' — core library ready.", name);
        }
        Commands::Callers { name } => {
            println!("Callers of '{}' — core library ready.", name);
        }
        Commands::Callees { name } => {
            println!("Callees of '{}' — core library ready.", name);
        }
        Commands::Neighborhood { name, depth, cap } => {
            println!(
                "Neighborhood of '{}' (depth={}, cap={}) — core library ready.",
                name, depth, cap
            );
        }
        Commands::Status => {
            println!("Index status — core library ready.");
        }
        Commands::Languages => {
            let langs = adaptive_codegraph_core::config::list_builtin_languages();
            println!("Built-in languages:");
            for lang in &langs {
                println!(
                    "  {} — extensions: {}",
                    lang.id,
                    lang.extensions.join(", ")
                );
            }
        }
    }

    Ok(())
}
