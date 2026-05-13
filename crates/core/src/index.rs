//! # Indexing Pipeline
//!
//! Walks the workspace, dispatches files to extractors, builds the graph,
//! and populates search + vector indexes.

use crate::config::Config;
use crate::embed::{self, Embedder, VectorIndex};
use crate::extract::ExtractorRegistry;
use crate::extract::plugin::PluginRegistry;
use crate::model::ExtractionResult;
use crate::search::SearchIndex;
use crate::store::Store;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    let files = collect_files(config, registry)?;
    tracing::info!("Collected {} files to index", files.len());

    // Extract in parallel
    let results: Vec<(PathBuf, ExtractionResult)> = files
        .par_iter()
        .filter_map(|path| {
            let ext = path.extension()?.to_str()?;
            let extractor = registry.for_extension(ext)?;
            let content = std::fs::read(path).ok()?;
            match extractor.extract(path, &content) {
                Ok(result) => Some((path.clone(), result)),
                Err(e) => {
                    tracing::warn!("Extract failed for {}: {e}", path.display());
                    None
                }
            }
        })
        .collect();

    // Build store
    let mut store = Store::new();
    let mut all_results = ExtractionResult::new();

    for (_path, result) in &results {
        all_results.merge(result.clone());
    }

    // Insert symbols
    for sym in &all_results.symbols {
        store.insert_symbol(sym.clone());
    }

    // Resolve and insert edges
    for edge in &all_results.edges {
        store.insert_edge(edge.clone());
    }

    tracing::info!(
        "Graph: {} symbols, {} edges",
        store.symbol_count(),
        store.edge_count()
    );

    // Build search index
    let index_dir = Path::new(&config.index_dir);
    let search = SearchIndex::open(index_dir)?;
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
    std::fs::create_dir_all(index_dir)?;
    state.save(&index_dir.join("state.json"))?;
    store.save(&index_dir.join("graph.bin"))?;
    vectors.save(&index_dir.join("vectors.bin"))?;

    Ok((store, search, vectors))
}

/// Collect all indexable files from configured roots.
fn collect_files(config: &Config, registry: &ExtractorRegistry) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for root in &config.roots {
        let walker = ignore::WalkBuilder::new(root)
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
    let root = config.roots.first()?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
