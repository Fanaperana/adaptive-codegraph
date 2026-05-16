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
/// Search order for the `languages/` directory:
/// 1. Next to the running binary (e.g. `target/release/languages/`)
/// 2. In the project being indexed (`base/languages/`)
///
/// This means you do NOT need to symlink `languages/` into every project.
pub fn build_registry(base: &Path) -> anyhow::Result<ExtractorRegistry> {
    let mut registry = ExtractorRegistry::new();

    // Try to find languages/ next to the binary first
    let lang_dir = find_languages_dir(base);
    let Some(lang_dir) = lang_dir else {
        tracing::warn!(
            "No languages/ directory found (checked binary dir and {base})",
            base = base.display()
        );
        return Ok(registry);
    };
    tracing::info!("Using languages/ from {}", lang_dir.display());

    // The base for resolving query file paths is the parent of languages/
    let lang_base = lang_dir.parent().unwrap_or(base);

    for entry in std::fs::read_dir(&lang_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            match load_language(&path, lang_base) {
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

/// Find the `languages/` directory by checking (in order):
/// 1. ADAPTIVE_CODEGRAPH_LANGUAGES env var
/// 2. Next to the current executable
/// 3. Workspace root up from the executable (for dev builds)
/// 4. In the project base directory
fn find_languages_dir(base: &Path) -> Option<PathBuf> {
    // 1. Check environment variable
    if let Ok(env_path) = std::env::var("ADAPTIVE_CODEGRAPH_LANGUAGES") {
        let p = PathBuf::from(&env_path);
        if p.is_dir() {
            return Some(p);
        }
        tracing::warn!("ADAPTIVE_CODEGRAPH_LANGUAGES={env_path} is not a directory");
    }

    // 2. Check next to the binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(LANGUAGES_DIR);
            if candidate.is_dir() {
                return Some(candidate);
            }
            // Also check one level up (e.g. target/release/../.. = workspace root)
            if let Some(parent) = exe_dir.parent() {
                if let Some(grandparent) = parent.parent() {
                    let candidate = grandparent.join(LANGUAGES_DIR);
                    if candidate.is_dir() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    // 3. Check XDG data home (~/.local/share/adaptive-codegraph/languages/)
    if let Ok(home) = std::env::var("HOME") {
        let xdg_data =
            std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
        let candidate = PathBuf::from(xdg_data)
            .join("adaptive-codegraph")
            .join(LANGUAGES_DIR);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    // 4. Fall back to the project directory
    let candidate = base.join(LANGUAGES_DIR);
    if candidate.is_dir() {
        return Some(candidate);
    }

    None
}

/// Load a single language definition and create its extractor.
fn load_language(
    toml_path: &Path,
    base: &Path,
) -> anyhow::Result<(TreeSitterExtractor, Vec<String>)> {
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
fn resolve_grammar(grammar: &str, lang_id: &str) -> anyhow::Result<tree_sitter::Language> {
    if grammar == "builtin" || grammar.is_empty() {
        match lang_id {
            "c" => Ok(tree_sitter_c::LANGUAGE.into()),
            "javascript" => Ok(tree_sitter_javascript::LANGUAGE.into()),
            "rust" => Ok(tree_sitter_rust::LANGUAGE.into()),
            "python" => Ok(tree_sitter_python::LANGUAGE.into()),
            "go" => Ok(tree_sitter_go::LANGUAGE.into()),
            "typescript" => Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            "tsx" => Ok(tree_sitter_typescript::LANGUAGE_TSX.into()),
            _ => anyhow::bail!(
                "No built-in grammar for '{}'. Add a tree-sitter-{} crate.",
                lang_id,
                lang_id
            ),
        }
    } else {
        anyhow::bail!(
            "External grammar loading not yet implemented for '{}': {}",
            lang_id,
            grammar
        );
    }
}
