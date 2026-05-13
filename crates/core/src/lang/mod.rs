//! # Language Registry
//!
//! Maps language IDs to their tree-sitter grammars and query files.
//! Supports built-in grammars (compiled into the binary) and external
//! grammars (loaded from shared libraries at runtime).

use crate::extract::treesitter::{TreeSitterConfig, TreeSitterExtractor};
use crate::extract::{Extractor, ExtractorRegistry};
use std::path::{Path, PathBuf};

/// A language definition loaded from a `.toml` config file.
#[derive(Debug, serde::Deserialize)]
pub struct LanguageDef {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    /// Path to the symbol extraction query file (.scm).
    pub symbol_query: Option<PathBuf>,
    /// Path to the edge extraction query file (.scm).
    pub edge_query: Option<PathBuf>,
    /// Tree-sitter grammar: "builtin" or path to shared library.
    pub grammar: String,
}

/// Directory containing language definition files.
const LANGUAGES_DIR: &str = "languages";

/// Build an extractor registry from the languages/ directory.
///
/// Each `.toml` file in languages/ defines a language. The corresponding
/// `.scm` files in languages/queries/ define the extraction queries.
pub fn build_registry(base: &Path) -> anyhow::Result<ExtractorRegistry> {
    let mut registry = ExtractorRegistry::new();
    let lang_dir = base.join(LANGUAGES_DIR);

    if !lang_dir.exists() {
        tracing::warn!("No languages/ directory found at {}", lang_dir.display());
        return Ok(registry);
    }

    for entry in std::fs::read_dir(&lang_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            match load_language(&path, base) {
                Ok((ext, extensions)) => {
                    tracing::info!("Loaded language: {}", ext.language());
                    let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
                    registry.register_with_extensions(Box::new(ext), &ext_refs);
                }
                Err(e) => {
                    tracing::warn!("Failed to load language from {}: {e}", path.display());
                }
            }
        }
    }

    Ok(registry)
}

/// Load a single language definition and create its extractor.
fn load_language(toml_path: &Path, base: &Path) -> anyhow::Result<(TreeSitterExtractor, Vec<String>)> {
    let text = std::fs::read_to_string(toml_path)?;
    let def: LanguageDef = toml::from_str(&text)?;

    let queries_dir = base.join(LANGUAGES_DIR).join("queries");

    // Load query files
    let symbol_query_path = def
        .symbol_query
        .clone()
        .map(|p| base.join(p))
        .unwrap_or_else(|| queries_dir.join(format!("{}.scm", def.id)));

    let edge_query_path = def
        .edge_query
        .clone()
        .map(|p| base.join(p))
        .unwrap_or_else(|| queries_dir.join(format!("{}_edges.scm", def.id)));

    let symbol_query = if symbol_query_path.exists() {
        std::fs::read_to_string(&symbol_query_path)?
    } else {
        tracing::warn!(
            "No symbol query file for {}: {}",
            def.id,
            symbol_query_path.display()
        );
        String::new()
    };

    let edge_query = if edge_query_path.exists() {
        std::fs::read_to_string(&edge_query_path)?
    } else {
        // Edge queries are optional
        String::new()
    };

    // Resolve the tree-sitter grammar
    let ts_language = resolve_grammar(&def.grammar, &def.id)?;

    let extensions = def.extensions.clone();
    let config = TreeSitterConfig {
        lang_id: def.id,
        extensions: def.extensions,
        ts_language,
        symbol_query,
        edge_query,
    };

    Ok((TreeSitterExtractor::new(config)?, extensions))
}

/// Resolve a grammar reference to a tree-sitter Language.
///
/// Currently supports "builtin" only. Future: load from .so/.dylib files.
fn resolve_grammar(grammar: &str, lang_id: &str) -> anyhow::Result<tree_sitter::Language> {
    if grammar == "builtin" || grammar.is_empty() {
        // Return a placeholder — actual grammar loading depends on which
        // tree-sitter-* crates are compiled in. This will be wired up
        // when specific language crates are added as dependencies.
        anyhow::bail!(
            "Built-in grammar for '{}' not yet compiled in. \
             Add the tree-sitter-{} crate as a dependency.",
            lang_id,
            lang_id
        );
    }

    // Future: load from shared library path
    anyhow::bail!(
        "External grammar loading not yet implemented for '{}': {}",
        lang_id,
        grammar
    );
}
