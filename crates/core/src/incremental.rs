//! # Incremental Reindex
//!
//! Re-parses only files whose mtime/size changed (works with or without git,
//! and sees uncommitted edits), then re-resolves the whole project so the
//! result is identical to a full index.

use crate::config::Config;
use crate::embed::VectorIndex;
use crate::extract::plugin::PluginRegistry;
use crate::extract::ExtractorRegistry;
use crate::index;
use crate::search::SearchIndex;
use crate::store::Store;

/// Result of incremental reindex.
pub struct IncrementalResult {
    pub files_updated: usize,
    pub files_deleted: usize,
    pub symbols_added: usize,
    pub symbols_removed: usize,
}

/// Bring `store`, `vectors` (and the on-disk search index) up to date.
pub fn incremental_reindex(
    config: &Config,
    registry: &ExtractorRegistry,
    store: &mut Store,
    _search: &SearchIndex,
    vectors: &mut VectorIndex,
) -> anyhow::Result<IncrementalResult> {
    let prev = std::mem::take(store);
    let plugins = PluginRegistry::new();
    let (new_store, _search, new_vectors, stats) =
        index::update_index(config, registry, &plugins, Some(prev))?;
    *store = new_store;
    *vectors = new_vectors;
    Ok(IncrementalResult {
        files_updated: stats.files_parsed,
        files_deleted: stats.files_deleted,
        symbols_added: stats.symbols_added,
        symbols_removed: stats.symbols_removed,
    })
}
