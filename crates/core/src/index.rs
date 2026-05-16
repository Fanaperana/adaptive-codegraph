//! # Indexing Pipeline
//!
//! Walks the workspace, dispatches files to extractors, builds the graph,
//! and populates search + vector indexes.

use crate::config::Config;
use crate::embed::{self, VectorIndex};
use crate::extract::plugin::PluginRegistry;
use crate::extract::ExtractorRegistry;
use crate::model::{ExtractionResult, SymbolId};
use crate::search::SearchIndex;
use crate::store::Store;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Index state saved between runs for incremental reindexing.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct IndexState {
    pub git_head: Option<String>,
    pub indexed_at: Option<u64>,
    pub file_count: usize,
}

impl IndexState {
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}

/// Full index build: walk all files, extract, build graph + search + vectors.
pub fn full_index(
    config: &Config,
    registry: &ExtractorRegistry,
    plugins: &PluginRegistry,
) -> anyhow::Result<(Store, SearchIndex, VectorIndex)> {
    let base = config.base.as_deref().unwrap_or(Path::new("."));
    let files = collect_files(config, registry, base)?;
    tracing::info!("Collected {} files to index", files.len());

    let failed_count = AtomicUsize::new(0);

    // Extract in parallel
    let results: Vec<(PathBuf, ExtractionResult, Vec<u8>)> = files
        .par_iter()
        .filter_map(|path| {
            let ext = path.extension()?.to_str()?;
            let extractor = registry.for_extension(ext)?;
            let content = std::fs::read(path).ok()?;
            match extractor.extract(path, &content) {
                Ok(result) => Some((path.clone(), result, content)),
                Err(e) => {
                    tracing::warn!("Extract failed for {}: {e}", path.display());
                    failed_count.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        })
        .collect();

    let failed = failed_count.load(Ordering::Relaxed);
    if failed > 0 {
        tracing::warn!("{failed} file(s) failed extraction (see warnings above)");
    }

    // Build store
    let mut store = Store::new();
    let mut all_results = ExtractionResult::new();

    for (_path, result, _content) in &results {
        all_results.merge(result.clone());
    }

    // Insert symbols first (needed for plugin symbol_index)
    for sym in &all_results.symbols {
        store.insert_symbol(sym.clone());
    }

    // Apply plugin patterns to each file's results
    if !plugins.pattern_names().is_empty() {
        let symbol_index: HashMap<String, SymbolId> = all_results
            .symbols
            .iter()
            .map(|s| (s.name.clone(), s.id))
            .collect();

        for (path, result, content) in &results {
            let path_str = path.to_string_lossy();
            if let Ok(content_str) = std::str::from_utf8(content) {
                let mut plugin_result = result.clone();
                plugins.apply_all(&path_str, content_str, &mut plugin_result, &symbol_index);
                // Collect any new edges from plugins
                for edge in &plugin_result.edges {
                    if !all_results.edges.contains(edge) {
                        all_results.edges.push(edge.clone());
                    }
                }
                for ue in &plugin_result.unresolved_edges {
                    all_results.unresolved_edges.push(ue.clone());
                }
            }
        }
    }

    // Resolve and insert edges: map unresolved callee names to actual symbols
    let mut resolved_count = 0usize;
    for ue in &all_results.unresolved_edges {
        // Try exact match first, then substring
        let candidates = store.find_by_name_exact(&ue.to_name);
        let target = if candidates.len() == 1 {
            Some(candidates[0])
        } else if candidates.len() > 1 {
            // Prefer same-file match for ambiguous names
            let from_sym = store.get(&ue.from);
            let from_file = from_sym.map(|s| s.file.as_str());
            candidates
                .iter()
                .find(|s| from_file == Some(s.file.as_str()))
                .or(candidates.first())
                .copied()
        } else {
            None
        };

        if let Some(target) = target {
            let resolved = crate::model::Edge {
                from: ue.from,
                to: target.id,
                kind: ue.kind.clone(),
            };
            if store.insert_edge(resolved) {
                resolved_count += 1;
            }
        }
    }

    // Also insert pre-resolved edges
    for edge in &all_results.edges {
        store.insert_edge(edge.clone());
    }

    tracing::info!(
        "Edges: {} unresolved, {} resolved, {} pre-resolved",
        all_results.unresolved_edges.len(),
        resolved_count,
        all_results.edges.len()
    );

    tracing::info!(
        "Graph: {} symbols, {} edges",
        store.symbol_count(),
        store.edge_count()
    );

    // Build search index
    let index_dir = base.join(&config.index_dir);
    let search = SearchIndex::open(&index_dir)?;
    let mut writer = search.writer(50_000_000)?;
    for sym in &all_results.symbols {
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
    }
    writer.commit()?;

    // Build vector index
    let embedder = embed::create_embedder();
    let mut vectors = VectorIndex::new(embedder.dim());

    let texts: Vec<&str> = all_results
        .symbols
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    if !texts.is_empty() {
        match embedder.embed_batch(&texts) {
            Ok(vecs) => {
                for (sym, vec) in all_results.symbols.iter().zip(vecs) {
                    vectors.insert(sym.id, vec);
                }
            }
            Err(e) => tracing::warn!("Embedding failed: {e}"),
        }
    }

    // Save state
    let state = IndexState {
        git_head: detect_git_head(config),
        indexed_at: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        ),
        file_count: files.len(),
    };
    std::fs::create_dir_all(&index_dir)?;
    state.save(&index_dir.join("state.json"))?;
    store.save(&index_dir.join("graph.bin"))?;
    vectors.save(&index_dir.join("vectors.bin"))?;

    Ok((store, search, vectors))
}

/// Collect all indexable files from configured roots.
fn collect_files(
    config: &Config,
    registry: &ExtractorRegistry,
    base: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for root in &config.roots {
        let abs_root = base.join(root);
        let walker = ignore::WalkBuilder::new(&abs_root)
            .hidden(true)
            .git_ignore(true)
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if registry.for_extension(ext).is_some() {
                    files.push(path.to_path_buf());
                }
            }
        }
    }

    Ok(files)
}

/// Detect current git HEAD SHA.
fn detect_git_head(config: &Config) -> Option<String> {
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
