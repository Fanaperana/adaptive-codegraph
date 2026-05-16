//! # Incremental Reindex
//!
//! Uses git diff (or mtime fallback) to detect changed files and
//! re-extracts only those, preserving cross-file edges where possible.

use crate::config::Config;
use crate::embed::{self, VectorIndex};
use crate::extract::ExtractorRegistry;
use crate::index::IndexState;
use crate::search::SearchIndex;
use crate::store::Store;
use std::path::{Path, PathBuf};

/// Result of incremental reindex.
pub struct IncrementalResult {
    pub files_updated: usize,
    pub files_deleted: usize,
    pub symbols_added: usize,
    pub symbols_removed: usize,
}

/// Perform incremental reindex based on git diff from last indexed HEAD.
pub fn incremental_reindex(
    config: &Config,
    registry: &ExtractorRegistry,
    store: &mut Store,
    search: &SearchIndex,
    vectors: &mut VectorIndex,
) -> anyhow::Result<IncrementalResult> {
    let base = config.base.as_deref().unwrap_or(Path::new("."));
    let index_dir = base.join(&config.index_dir);
    let state_path = index_dir.join("state.json");

    let prev_state = IndexState::load(&state_path).unwrap_or_default();

    // Detect changed files
    let (changed, deleted) = detect_changes(config, &prev_state)?;

    tracing::info!(
        "Incremental: {} changed, {} deleted",
        changed.len(),
        deleted.len()
    );

    let mut writer = search.writer(50_000_000)?;
    let embedder = embed::create_embedder();

    let mut symbols_removed = 0;
    let mut symbols_added = 0;

    // Remove deleted files
    for path in &deleted {
        let path_str = path.to_string_lossy();
        symbols_removed += store.remove_file(&path_str);
        search.remove_file(&writer, &path_str);
    }

    // Re-extract changed files
    for path in &changed {
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };
        let extractor = match registry.for_extension(ext) {
            Some(e) => e,
            None => continue,
        };
        let content = match std::fs::read(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Remove old data for this file
        let path_str = path.to_string_lossy().to_string();
        symbols_removed += store.remove_file(&path_str);
        search.remove_file(&writer, &path_str);

        // Re-extract
        match extractor.extract(path, &content) {
            Ok(result) => {
                symbols_added += result.symbols.len();

                for sym in &result.symbols {
                    store.insert_symbol(sym.clone());
                    search.index_symbol(
                        &writer,
                        &sym.id,
                        &sym.name,
                        &sym.fqname,
                        &sym.file,
                        sym.signature.as_deref(),
                        &sym.kind,
                        &sym.lang,
                    );

                    // Update vector
                    if let Ok(vec) = embedder.embed_one(&sym.name) {
                        vectors.insert(sym.id, vec);
                    }
                }

                for edge in &result.edges {
                    store.insert_edge(edge.clone());
                }
            }
            Err(e) => {
                tracing::warn!("Re-extract failed for {}: {e}", path.display());
            }
        }
    }

    writer.commit()?;

    // Save updated state
    let new_state = IndexState {
        git_head: detect_current_head(config),
        indexed_at: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        ),
        file_count: store.symbol_count(), // approximation
    };
    new_state.save(&state_path)?;
    store.save(&index_dir.join("graph.bin"))?;
    vectors.save(&index_dir.join("vectors.bin"))?;

    Ok(IncrementalResult {
        files_updated: changed.len(),
        files_deleted: deleted.len(),
        symbols_added,
        symbols_removed,
    })
}

/// Detect changed and deleted files since last index.
fn detect_changes(
    config: &Config,
    prev_state: &IndexState,
) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let base = config.base.as_deref().unwrap_or(Path::new("."));

    // Try git-based detection first
    if let Some(prev_head) = &prev_state.git_head {
        if let Ok((changed, deleted)) = git_diff_files(base.to_str().unwrap_or("."), prev_head) {
            return Ok((changed, deleted));
        }
    }

    // Fallback: mtime-based detection
    mtime_diff_files(
        base.to_str().unwrap_or("."),
        prev_state.indexed_at.unwrap_or(0),
    )
}

/// Use `git diff --name-status` to find changed/deleted files.
fn git_diff_files(root: &str, prev_head: &str) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-status", prev_head, "HEAD"])
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("git diff failed");
    }

    let mut changed = Vec::new();
    let mut deleted = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let status = parts[0];
        let file = PathBuf::from(root).join(parts[1]);

        match status {
            "D" => deleted.push(file),
            _ => changed.push(file),
        }
    }

    // Also include working-tree changes
    let wt_output = std::process::Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(root)
        .output()?;

    for line in String::from_utf8_lossy(&wt_output.stdout).lines() {
        let file = PathBuf::from(root).join(line.trim());
        if !changed.contains(&file) {
            changed.push(file);
        }
    }

    Ok((changed, deleted))
}

/// Fallback: find files modified after the given unix timestamp.
fn mtime_diff_files(root: &str, since_epoch: u64) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let since = std::time::UNIX_EPOCH + std::time::Duration::from_secs(since_epoch);
    let mut changed = Vec::new();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if mtime > since {
                    changed.push(entry.into_path());
                }
            }
        }
    }

    Ok((changed, Vec::new()))
}

fn detect_current_head(config: &Config) -> Option<String> {
    let base = config.base.as_deref().unwrap_or(Path::new("."));
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(base)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
