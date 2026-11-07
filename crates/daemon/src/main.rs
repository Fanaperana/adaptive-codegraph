//! # File-watching Daemon
//!
//! Watches the workspace and keeps the index current. Each burst of file
//! events (debounced) triggers one incremental update; events that arrive
//! while an update runs are picked up by the next one, so no change is lost.
//!
//! The MCP server also refreshes on demand, so the daemon is optional: it
//! just makes the first query after a big change instant.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use notify_debouncer_mini::new_debouncer;

use adaptive_codegraph_core::service::Workspace;

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

    /// Debounce interval in milliseconds
    #[arg(long, default_value = "500")]
    debounce_ms: u64,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let base = PathBuf::from(&cli.base).canonicalize()?;
    let mut ws = Workspace::open(&base)?;
    ws.auto_refresh = false;
    if let Some(note) = ws.ensure_fresh()? {
        tracing::info!("{note}");
    }
    ws.refresh(false)?;
    let index_dir = ws.index_dir();

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(cli.debounce_ms), tx)?;
    debouncer
        .watcher()
        .watch(&base, notify::RecursiveMode::Recursive)?;
    tracing::info!("Watching {}", base.display());

    loop {
        let events = match rx.recv() {
            Ok(Ok(events)) => events,
            Ok(Err(e)) => {
                tracing::warn!("watch error: {e:?}");
                continue;
            }
            Err(_) => break,
        };
        // Ignore our own writes and VCS internals.
        let relevant = events.iter().any(|e| {
            !e.path.starts_with(&index_dir)
                && !e.path.components().any(|c| c.as_os_str() == ".git")
        });
        if !relevant {
            continue;
        }
        // Coalesce anything else already queued.
        while rx.try_recv().is_ok() {}

        let t = Instant::now();
        match ws.refresh(false) {
            Ok(Some(note)) => tracing::info!("{note} in {:.2}s", t.elapsed().as_secs_f64()),
            Ok(None) => {}
            Err(e) => tracing::error!("reindex failed: {e}"),
        }
    }
    Ok(())
}
