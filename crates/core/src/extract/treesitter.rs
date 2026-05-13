//! # Generic Tree-sitter Extractor
//!
//! Extracts symbols and edges from any language that has:
//! 1. A tree-sitter grammar (compiled as a shared library or built-in)
//! 2. A `.scm` query file defining symbol and edge patterns
//!
//! The query file uses tree-sitter's query language with special capture names:
//! - `@symbol.name` — the name node of a symbol definition
//! - `@symbol.def`  — the entire symbol definition node
//! - `@call.name`   — the name of a called function
//! - `@import.path` — an import/include path
//!
//! Example query for Python:
//! ```scheme
//! ; Functions
//! (function_definition name: (identifier) @symbol.name) @symbol.def
//!
//! ; Classes
//! (class_definition name: (identifier) @symbol.name) @symbol.def
//!
//! ; Calls
//! (call function: (identifier) @call.name)
//!
//! ; Imports
//! (import_from_statement module_name: (dotted_name) @import.path)
//! ```

use crate::extract::Extractor;
use crate::model::{ExtractionResult, Span, Symbol};
use streaming_iterator::StreamingIterator;
use std::path::Path;

/// Configuration for a tree-sitter based extractor.
pub struct TreeSitterConfig {
    /// Language identifier.
    pub lang_id: String,
    /// File extensions.
    pub extensions: Vec<String>,
    /// The tree-sitter language grammar.
    pub ts_language: tree_sitter::Language,
    /// Query source for symbol extraction.
    pub symbol_query: String,
    /// Query source for edge extraction (calls, imports).
    pub edge_query: String,
}

/// A generic extractor powered by tree-sitter + query files.
pub struct TreeSitterExtractor {
    config: TreeSitterConfig,
    symbol_query: tree_sitter::Query,
    edge_query: tree_sitter::Query,
}

impl TreeSitterExtractor {
    pub fn new(config: TreeSitterConfig) -> anyhow::Result<Self> {
        let symbol_query =
            tree_sitter::Query::new(&config.ts_language, &config.symbol_query)
                .map_err(|e| anyhow::anyhow!("symbol query error for {}: {e}", config.lang_id))?;
        let edge_query =
            tree_sitter::Query::new(&config.ts_language, &config.edge_query)
                .map_err(|e| anyhow::anyhow!("edge query error for {}: {e}", config.lang_id))?;

        Ok(Self {
            config,
            symbol_query,
            edge_query,
        })
    }

    /// Run the symbol query and extract Symbol structs.
    fn extract_symbols(
        &self,
        path: &Path,
        tree: &tree_sitter::Tree,
        source: &[u8],
    ) -> Vec<Symbol> {
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&self.symbol_query, tree.root_node(), source);

        let name_idx = self
            .symbol_query
            .capture_index_for_name("symbol.name");
        let def_idx = self
            .symbol_query
            .capture_index_for_name("symbol.def");

        let mut symbols = Vec::new();
        let path_str = path.to_string_lossy().to_string();

        while let Some(m) = matches.next() {
            let name_node = name_idx.and_then(|idx| {
                m.captures.iter().find(|c| c.index == idx).map(|c| c.node)
            });
            let def_node = def_idx.and_then(|idx| {
                m.captures.iter().find(|c| c.index == idx).map(|c| c.node)
            });

            if let Some(name_node) = name_node {
                let name = name_node.utf8_text(source).unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }

                let span_node = def_node.unwrap_or(name_node);
                let kind = infer_kind(span_node.kind());
                let fqname = format!("{}::{}", path_str, name);

                let span = Span {
                    start_byte: span_node.start_byte(),
                    end_byte: span_node.end_byte(),
                    start_line: span_node.start_position().row as u32,
                    end_line: span_node.end_position().row as u32,
                };

                // Try to extract the signature (first line of the definition)
                let signature = def_node.map(|n| {
                    let text = n.utf8_text(source).unwrap_or("");
                    text.lines().next().unwrap_or("").to_string()
                });

                let mut sym =
                    Symbol::new(&self.config.lang_id, &kind, &name, &fqname, &path_str, span);
                sym.signature = signature;
                symbols.push(sym);
            }
        }

        symbols
    }

    /// Run the edge query and collect unresolved edge references.
    fn extract_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        result: &mut ExtractionResult,
    ) {
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&self.edge_query, tree.root_node(), source);

        let call_idx = self.edge_query.capture_index_for_name("call.name");
        let import_idx = self.edge_query.capture_index_for_name("import.path");

        // Build a map of byte-range → symbol ID for edge resolution
        let sym_ranges: Vec<(std::ops::Range<usize>, crate::model::SymbolId)> = result
            .symbols
            .iter()
            .map(|s| (s.span.start_byte..s.span.end_byte, s.id))
            .collect();

        while let Some(m) = matches.next() {
            // Call edges
            if let Some(idx) = call_idx {
                for cap in m.captures.iter().filter(|c| c.index == idx) {
                    let callee_name = cap.node.utf8_text(source).unwrap_or("");
                    if callee_name.is_empty() {
                        continue;
                    }

                    // Find which symbol this call is inside
                    let call_byte = cap.node.start_byte();
                    if let Some((_, caller_id)) = sym_ranges
                        .iter()
                        .find(|(range, _)| range.contains(&call_byte))
                    {
                        result.add_edge(*caller_id, callee_name, "calls");
                    }
                }
            }

            // Import edges
            if let Some(idx) = import_idx {
                for cap in m.captures.iter().filter(|c| c.index == idx) {
                    let import_path = cap.node.utf8_text(source).unwrap_or("");
                    if import_path.is_empty() {
                        continue;
                    }
                    // Import edges are file-level, use first symbol or a synthetic one
                    if let Some(sym) = result.symbols.first() {
                        result.add_edge(sym.id, import_path, "imports");
                    }
                }
            }
        }
    }
}

impl Extractor for TreeSitterExtractor {
    fn language(&self) -> &str {
        &self.config.lang_id
    }

    fn extensions(&self) -> &[&str] {
        // This is a bit awkward — we need 'static references but have owned strings.
        // In practice, the extractor lives for the program lifetime so this is fine
        // via a leaked slice. For a cleaner API, we'd use Cow<'static, str>.
        // For now, return an empty slice — matching is done via ExtractorRegistry.
        &[]
    }

    fn extract(&self, path: &Path, content: &[u8]) -> anyhow::Result<ExtractionResult> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.config.ts_language)?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter parse failed for {}", path.display()))?;

        let mut result = ExtractionResult::new();
        let symbols = self.extract_symbols(path, &tree, content);
        for sym in symbols {
            result.add_symbol(sym);
        }
        self.extract_edges(&tree, content, &mut result);

        Ok(result)
    }
}

/// Infer a kind string from the tree-sitter node type.
fn infer_kind(node_kind: &str) -> String {
    match node_kind {
        "function_definition" | "function_declaration" | "function_item" | "method_definition" => {
            "function".to_string()
        }
        "class_definition" | "class_declaration" => "class".to_string(),
        "struct_item" | "struct_specifier" => "struct".to_string(),
        "enum_item" | "enum_specifier" => "enum".to_string(),
        "type_alias_declaration" | "type_item" => "type_alias".to_string(),
        "interface_declaration" => "interface".to_string(),
        "impl_item" => "impl".to_string(),
        "trait_item" => "trait".to_string(),
        "module" | "mod_item" => "module".to_string(),
        _ => "definition".to_string(),
    }
}
