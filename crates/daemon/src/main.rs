//! # File-watching Daemon
//!
//! Watches the workspace for file changes and triggers incremental reindex.
//! Runs as a background process alongside the MCP server.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};

use adaptive_codegraph_core::{
    config::Config,
    embed::{self},
    incremental, lang,
    search::SearchIndex,
    store::Store,
};

#[derive(Parser)]
#[command(
    name = "adaptive-codegraph-daemon",
    version,
    about = "Watch files and incrementally reindex"
)]
struct Cli {
    /// Path to workspace root
    #[arg(long, default_value = ".")]
    base: String,

    /// Debounce interval in seconds
    #[arg(long, default_value = "2")]
    debounce: u64,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let base = PathBuf::from(&cli.base).canonicalize()?;
    let config = Config::load(&base)?;
    let index_dir = base.join(&config.index_dir);

    if !index_dir.join("graph.bin").exists() {
        anyhow::bail!(
            "No index found at {}. Run `adaptive-codegraph index` first.",
            index_dir.display()
        );
    }

    let registry = lang::build_registry(&base)?;
    let debounce_secs = Duration::from_secs(cli.debounce);

    tracing::info!(
        "Daemon started, watching {:?} (debounce: {}s)",
        &config.roots,
        cli.debounce
    );

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(debounce_secs, tx)?;

    for root in &config.roots {
        let watch_path = base.join(root);
        if watch_path.is_dir() {
            debouncer
                .watcher()
                .watch(&watch_path, notify::RecursiveMode::Recursive)?;
            tracing::info!("Watching: {}", watch_path.display());
        } else {
            tracing::warn!("Root not found, skipping: {}", watch_path.display());
        }
    }

    let mut last_reindex = Instant::now();

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let has_file_changes = events.iter().any(|e| {
                    e.kind == DebouncedEventKind::Any
                        && e.path.is_file()
                        && registry
                            .for_extension(
                                e.path
                                    .extension()
                                    .and_then(|ext| ext.to_str())
                                    .unwrap_or(""),
                            )
                            .is_some()
                });

                if !has_file_changes {
                    continue;
                }

                if last_reindex.elapsed() < debounce_secs {
                    continue;
                }

                tracing::info!("File changes detected, starting incremental reindex...");
                last_reindex = Instant::now();

                match run_incremental_reindex(&base, &config, &registry, &index_dir) {
                    Ok(result) => {
                        tracing::info!(
                            "Incremental reindex: {} updated, {} deleted, +{} -{} symbols",
                            result.files_updated,
                            result.files_deleted,
                            result.symbols_added,
                            result.symbols_removed,
                        );
                    }
                    Err(e) => {
                        tracing::error!("Incremental reindex failed: {e}");
                    }
                }
            }
            Ok(Err(errors)) => {
                tracing::warn!("Watch errors: {:?}", errors);
            }
            Err(e) => {
                tracing::error!("Channel receive error: {e}");
                break;
            }
        }
    }

    Ok(())
}

fn run_incremental_reindex(
    _base: &PathBuf,
    config: &Config,
    registry: &adaptive_codegraph_core::extract::ExtractorRegistry,
    index_dir: &PathBuf,
) -> Result<incremental::IncrementalResult> {
    let mut store = Store::load(&index_dir.join("graph.bin"))?;
    let search = SearchIndex::open(index_dir)?;
    let dim = embed::create_embedder().dim();
    let mut vectors =
        adaptive_codegraph_core::embed::VectorIndex::load(&index_dir.join("vectors.bin"))
            .unwrap_or_else(|_| adaptive_codegraph_core::embed::VectorIndex::new(dim));

    incremental::incremental_reindex(config, registry, &mut store, &search, &mut vectors)
}
