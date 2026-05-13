//! # Extraction System
//!
//! Defines the `Extractor` trait and provides:
//! - A generic tree-sitter extractor driven by `.scm` query files
//! - A plugin system for custom cross-language edge patterns

pub mod plugin;
pub mod treesitter;

use crate::model::ExtractionResult;
use std::path::Path;

/// Trait for language-specific symbol/edge extractors.
///
/// Each language registers an extractor. The indexer calls `extract()` for
/// every file matching the extractor's language.
pub trait Extractor: Send + Sync {
    /// Language identifier, e.g. "python", "rust", "c".
    fn language(&self) -> &str;

    /// File extensions this extractor handles.
    fn extensions(&self) -> &[&str];

    /// Extract symbols and edges from a single file.
    ///
    /// - `path`: workspace-relative file path
    /// - `content`: file contents as bytes
    fn extract(&self, path: &Path, content: &[u8]) -> anyhow::Result<ExtractionResult>;
}

/// Registry of all available extractors.
pub struct ExtractorRegistry {
    extractors: Vec<Box<dyn Extractor>>,
    /// Extension → extractor index mapping for fast lookup.
    ext_map: std::collections::HashMap<String, usize>,
}

impl ExtractorRegistry {
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
            ext_map: std::collections::HashMap::new(),
        }
    }

    /// Register an extractor.
    pub fn register(&mut self, ext: Box<dyn Extractor>) {
        let idx = self.extractors.len();
        for e in ext.extensions() {
            self.ext_map.insert(e.to_string(), idx);
        }
        self.extractors.push(ext);
    }

    /// Register an extractor with explicit extensions (for extractors that
    /// return empty from `extensions()` like TreeSitterExtractor).
    pub fn register_with_extensions(&mut self, ext: Box<dyn Extractor>, extensions: &[&str]) {
        let idx = self.extractors.len();
        for e in extensions {
            self.ext_map.insert(e.to_string(), idx);
        }
        self.extractors.push(ext);
    }

    /// Find the extractor for a given file extension.
    pub fn for_extension(&self, ext: &str) -> Option<&dyn Extractor> {
        self.ext_map
            .get(ext)
            .and_then(|&idx| self.extractors.get(idx))
            .map(|e| e.as_ref())
    }

    /// List all registered languages.
    pub fn languages(&self) -> Vec<&str> {
        self.extractors.iter().map(|e| e.language()).collect()
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
