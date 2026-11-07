//! # Extraction System
//!
//! Defines the `Extractor` trait and provides:
//! - A generic tree-sitter extractor driven by `.scm` query files
//! - A plugin system for custom cross-language edge patterns

pub mod plugin;
pub mod treesitter;

use crate::model::ExtractionResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How a language scopes names across files. Set in the language `.toml`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LangProfile {
    /// `"file"` (default): a module is a file (Rust, Python, TS, JS).
    /// `"directory"`: a module/package is a directory (Go).
    #[serde(default = "default_module_mode")]
    pub module_mode: String,
    /// `"import"` (default): names from other modules must be imported.
    /// `"global"`: top-level names are visible everywhere (C).
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_module_mode() -> String {
    "file".into()
}
fn default_scope() -> String {
    "import".into()
}

impl Default for LangProfile {
    fn default() -> Self {
        Self {
            module_mode: default_module_mode(),
            scope: default_scope(),
        }
    }
}

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

    /// Cross-file scoping rules for this language.
    fn profile(&self) -> LangProfile {
        LangProfile::default()
    }
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

    /// Scoping profile of every registered language.
    pub fn profiles(&self) -> std::collections::HashMap<String, LangProfile> {
        self.extractors
            .iter()
            .map(|e| (e.language().to_string(), e.profile()))
            .collect()
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
