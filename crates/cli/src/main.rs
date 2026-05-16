use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Instant;

use adaptive_codegraph_core::{
    config::Config,
    embed::{self, VectorIndex},
    extract::plugin::PluginRegistry,
    incremental,
    index::{self, IndexState},
    lang,
    query,
    search::SearchIndex,
    store::Store,
};

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
    Index {
        /// Only re-index files changed since last index (git-aware)
        #[arg(long)]
        incremental: bool,
    },
    /// Search symbols by name/text (BM25)
    Search {
        query: String,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Semantic similarity search (vector embeddings)
    SemanticSearch {
        query: String,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Find a symbol by name
    Find {
        name: String,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show callers of a symbol
    Callers { name: String },
    /// Show callees of a symbol
    Callees { name: String },
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
    let base = PathBuf::from(&cli.base).canonicalize()?;

    match cli.command {
        Commands::Index { incremental } => {
            if incremental {
                cmd_incremental_index(&base)?;
            } else {
                cmd_index(&base)?;
            }
        }
        Commands::Search { query, limit } => cmd_search(&base, &query, limit)?,
        Commands::SemanticSearch { query, limit } => cmd_semantic_search(&base, &query, limit)?,
        Commands::Find { name, limit } => cmd_find(&base, &name, limit)?,
        Commands::Callers { name } => cmd_callers(&base, &name)?,
        Commands::Callees { name } => cmd_callees(&base, &name)?,
        Commands::Neighborhood { name, depth, cap } => cmd_neighborhood(&base, &name, depth, cap)?,
        Commands::Status => cmd_status(&base)?,
        Commands::Languages => cmd_languages(&base)?,
    }

    Ok(())
}

fn cmd_index(base: &PathBuf) -> Result<()> {
    let t0 = Instant::now();
    let config = Config::load(base)?;
    let registry = lang::build_registry(base)?;
    let plugins = PluginRegistry::new();

    eprintln!("Languages loaded: {:?}", registry.languages());
    let (store, _search, _vectors) = index::full_index(&config, &registry, &plugins)?;
    eprintln!(
        "Indexed {} symbols, {} edges in {:.2}s",
        store.symbol_count(),
        store.edge_count(),
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

fn cmd_incremental_index(base: &PathBuf) -> Result<()> {
    let t0 = Instant::now();
    let config = Config::load(base)?;
    let index_dir = base.join(&config.index_dir);

    // Check if a full index exists
    if !index_dir.join("graph.bin").exists() {
        eprintln!("No existing index found. Running full index instead.");
        return cmd_index(base);
    }

    let registry = lang::build_registry(base)?;
    let mut store = Store::load(&index_dir.join("graph.bin"))?;
    let search = SearchIndex::open(&index_dir)?;
    let dim = embed::create_embedder().dim();
    let mut vectors = VectorIndex::load(&index_dir.join("vectors.bin"))
        .unwrap_or_else(|_| VectorIndex::new(dim));

    let result = incremental::incremental_reindex(
        &config, &registry, &mut store, &search, &mut vectors,
    )?;

    eprintln!(
        "Incremental: {} updated, {} deleted, +{} symbols, -{} symbols in {:.2}s",
        result.files_updated,
        result.files_deleted,
        result.symbols_added,
        result.symbols_removed,
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

fn load_index(base: &PathBuf) -> Result<(Config, Store, SearchIndex)> {
    let config = Config::load(base)?;
    let index_dir = base.join(&config.index_dir);
    if !index_dir.join("graph.bin").exists() {
        anyhow::bail!(
            "No index found at {}. Run `adaptive-codegraph index` first.",
            index_dir.display()
        );
    }
    let store = Store::load(&index_dir.join("graph.bin"))?;
    let search = SearchIndex::open(&index_dir)?;
    Ok((config, store, search))
}

fn cmd_search(base: &PathBuf, q: &str, limit: usize) -> Result<()> {
    let (_config, _store, search) = load_index(base)?;
    let hits = search.search(q, limit)?;
    if hits.is_empty() {
        println!("No results.");
        return Ok(());
    }
    for hit in &hits {
        println!(
            "{} ({}, {}) — {} [score: {:.3}]",
            hit.name, hit.kind, hit.lang, hit.file, hit.score
        );
    }
    Ok(())
}

fn cmd_semantic_search(base: &PathBuf, q: &str, limit: usize) -> Result<()> {
    let (config, store, _search) = load_index(base)?;
    let index_dir = base.join(&config.index_dir);
    let vectors = VectorIndex::load(&index_dir.join("vectors.bin"))?;
    let embedder = embed::create_embedder();
    let query_vec = embedder.embed_one(q)?;
    let results = vectors.search(&query_vec, limit);
    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }
    for (id, score) in &results {
        if let Some(sym) = store.get(id) {
            println!(
                "{} ({}, {}) — {} [similarity: {:.3}]",
                sym.name, sym.kind, sym.lang, sym.file, score
            );
        }
    }
    Ok(())
}

fn cmd_find(base: &PathBuf, name: &str, limit: usize) -> Result<()> {
    let (_config, store, _search) = load_index(base)?;
    let results = store.find_by_name(name);
    if results.is_empty() {
        println!("No symbols matching '{}'.", name);
        return Ok(());
    }
    for sym in results.iter().take(limit) {
        println!(
            "{} ({}, {}) — {}:{}-{}  [{}]",
            sym.name, sym.kind, sym.lang, sym.file,
            sym.span.start_line, sym.span.end_line, sym.id.to_hex()
        );
    }
    Ok(())
}

fn cmd_callers(base: &PathBuf, name: &str) -> Result<()> {
    let (_config, store, _search) = load_index(base)?;
    let id = query::resolve_symbol(&store, name)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", name))?;
    let callers = store.callers(&id);
    if callers.is_empty() {
        println!("No callers found for '{}'.", name);
        return Ok(());
    }
    for (sym, kind) in &callers {
        println!("  {} --[{}]--> {}", sym.name, kind, name);
    }
    Ok(())
}

fn cmd_callees(base: &PathBuf, name: &str) -> Result<()> {
    let (_config, store, _search) = load_index(base)?;
    let id = query::resolve_symbol(&store, name)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", name))?;
    let callees = store.callees(&id);
    if callees.is_empty() {
        println!("No callees found for '{}'.", name);
        return Ok(());
    }
    for (sym, kind) in &callees {
        println!("  {} --[{}]--> {}", name, kind, sym.name);
    }
    Ok(())
}

fn cmd_neighborhood(base: &PathBuf, name: &str, depth: usize, cap: usize) -> Result<()> {
    let (_config, store, _search) = load_index(base)?;
    let id = query::resolve_symbol(&store, name)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", name))?;
    let neighborhood = query::expand_neighborhood(&store, id, depth, cap);
    print!("{}", query::format_neighborhood(&store, &neighborhood));
    Ok(())
}

fn cmd_status(base: &PathBuf) -> Result<()> {
    let config = Config::load(base)?;
    let index_dir = base.join(&config.index_dir);
    let state_path = index_dir.join("state.json");
    if !state_path.exists() {
        println!("No index found. Run `adaptive-codegraph index` first.");
        return Ok(());
    }
    let state = IndexState::load(&state_path)?;
    println!("Git HEAD at index: {}", state.git_head.as_deref().unwrap_or("(none)"));
    println!("Indexed at (unix): {}", state.indexed_at.unwrap_or(0));
    println!("Files indexed:     {}", state.file_count);

    let graph_path = index_dir.join("graph.bin");
    if graph_path.exists() {
        let store = Store::load(&graph_path)?;
        println!("Symbols:           {}", store.symbol_count());
        println!("Edges:             {}", store.edge_count());
    }
    Ok(())
}

fn cmd_languages(base: &PathBuf) -> Result<()> {
    let registry = lang::build_registry(base)?;
    let langs = registry.languages();
    if langs.is_empty() {
        println!("No languages loaded. Make sure languages/ directory exists.");
        let builtins = adaptive_codegraph_core::config::list_builtin_languages();
        println!("\nBuilt-in languages available:");
        for b in &builtins {
            println!("  {} — extensions: {}", b.id, b.extensions.join(", "));
        }
    } else {
        println!("Loaded languages:");
        for lang in &langs {
            println!("  {}", lang);
        }
    }
    Ok(())
}
