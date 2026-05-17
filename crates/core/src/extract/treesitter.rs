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
use std::path::Path;
use streaming_iterator::StreamingIterator;

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
        let symbol_query = tree_sitter::Query::new(&config.ts_language, &config.symbol_query)
            .map_err(|e| anyhow::anyhow!("symbol query error for {}: {e}", config.lang_id))?;
        let edge_query = tree_sitter::Query::new(&config.ts_language, &config.edge_query)
            .map_err(|e| anyhow::anyhow!("edge query error for {}: {e}", config.lang_id))?;

        Ok(Self {
            config,
            symbol_query,
            edge_query,
        })
    }

    /// Run the symbol query and extract Symbol structs.
    fn extract_symbols(&self, path: &Path, tree: &tree_sitter::Tree, source: &[u8]) -> Vec<Symbol> {
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&self.symbol_query, tree.root_node(), source);

        let name_idx = self.symbol_query.capture_index_for_name("symbol.name");
        let def_idx = self.symbol_query.capture_index_for_name("symbol.def");

        let mut symbols = Vec::new();
        let path_str = path.to_string_lossy().to_string();

        while let Some(m) = matches.next() {
            let name_node =
                name_idx.and_then(|idx| m.captures.iter().find(|c| c.index == idx).map(|c| c.node));
            let def_node =
                def_idx.and_then(|idx| m.captures.iter().find(|c| c.index == idx).map(|c| c.node));

            if let Some(name_node) = name_node {
                let name = name_node.utf8_text(source).unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }

                let span_node = def_node.unwrap_or(name_node);
                let kind = infer_kind(span_node.kind());

                // Build scoped fqname by walking parent nodes
                let fqname = build_fqname(&path_str, &name, name_node, source);

                let span = Span {
                    start_byte: span_node.start_byte(),
                    end_byte: span_node.end_byte(),
                    start_line: span_node.start_position().row as u32,
                    end_line: span_node.end_position().row as u32,
                };

                // Extract multi-line signature (up to closing paren/brace)
                let signature = def_node.map(|n| extract_signature(n, source));

                // Extract doc comment from preceding sibling nodes
                let doc = def_node.and_then(|n| extract_doc_comment(n, source));

                let mut sym =
                    Symbol::new(&self.config.lang_id, &kind, &name, &fqname, &path_str, span);
                sym.signature = signature;
                sym.doc = doc;
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
        "function_definition"
        | "function_declaration"
        | "function_item"
        | "method_definition"
        | "method_declaration"
        | "generator_function_declaration"
        | "abstract_method_signature" => "function".to_string(),
        "class_definition" | "class_declaration" => "class".to_string(),
        "struct_item" | "struct_specifier" => "struct".to_string(),
        "enum_item" | "enum_specifier" | "enum_declaration" => "enum".to_string(),
        "enum_variant" | "enumerator" | "enum_assignment" => "enum_variant".to_string(),
        "type_alias_declaration" | "type_item" | "type_definition" => "type_alias".to_string(),
        "interface_declaration" => "interface".to_string(),
        "impl_item" => "impl".to_string(),
        "trait_item" => "trait".to_string(),
        "module" | "mod_item" => "module".to_string(),
        "field_declaration" | "public_field_definition" | "field_definition" => "field".to_string(),
        "macro_definition" | "preproc_def" | "preproc_function_def" => "macro".to_string(),
        "const_item" | "const_spec" | "const_declaration" => "constant".to_string(),
        "static_item" => "static".to_string(),
        "union_specifier" => "union".to_string(),
        "type_spec" | "type_declaration" => "type".to_string(),
        "var_declaration" | "var_spec" => "variable".to_string(),
        "decorated_definition" => "decorated".to_string(),
        "lexical_declaration" | "variable_declarator" => "variable".to_string(),
        "assignment" => "variable".to_string(),
        _ => "definition".to_string(),
    }
}

/// Walk parent nodes to build a fully qualified name like `file::Class::method`.
fn build_fqname(path: &str, name: &str, node: tree_sitter::Node, source: &[u8]) -> String {
    let scope_kinds = [
        "class_definition",
        "class_declaration",
        "struct_item",
        "struct_specifier",
        "impl_item",
        "trait_item",
        "module",
        "mod_item",
        "function_definition",
        "function_declaration",
        "function_item",
    ];

    let mut scope_chain = Vec::new();
    let mut current = node.parent();

    while let Some(parent) = current {
        if scope_kinds.contains(&parent.kind()) {
            // Look for a `name:` child to get the scope name
            if let Some(name_child) = parent.child_by_field_name("name") {
                if let Ok(scope_name) = name_child.utf8_text(source) {
                    if !scope_name.is_empty() {
                        scope_chain.push(scope_name.to_string());
                    }
                }
            }
        }
        current = parent.parent();
    }

    scope_chain.reverse();
    if scope_chain.is_empty() {
        format!("{}::{}", path, name)
    } else {
        format!("{}::{}::{}", path, scope_chain.join("::"), name)
    }
}

/// Extract a multi-line signature (up to the first `{`, `=>`, or `:` at depth 0).
fn extract_signature(node: tree_sitter::Node, source: &[u8]) -> String {
    let text = node.utf8_text(source).unwrap_or("");
    // Find the opening brace/colon that starts the body
    let mut depth = 0i32;
    let mut end_idx = text.len();

    for (i, ch) in text.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '{' if depth == 0 => {
                end_idx = i;
                break;
            }
            ':' if depth == 0 && i > 0 => {
                // Python-style: `def foo(x):` — include the colon
                // But not for Rust's `pub fn foo() -> Type {`
                let after = text.get(i + 1..).unwrap_or("");
                if after.starts_with('\n') || after.starts_with(' ') || after.is_empty() {
                    // Likely Python-style class/function body start
                    end_idx = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    let sig = text[..end_idx].trim();
    // Collapse internal whitespace runs
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract doc comments from preceding sibling nodes (e.g., /// or /** */).
fn extract_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut sibling = node.prev_sibling();

    while let Some(sib) = sibling {
        let kind = sib.kind();
        if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
            if let Ok(text) = sib.utf8_text(source) {
                lines.push(text.to_string());
            }
            sibling = sib.prev_sibling();
        } else {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    let cleaned: Vec<String> = lines
        .iter()
        .map(|l| {
            l.trim()
                .trim_start_matches("///")
                .trim_start_matches("//!")
                .trim_start_matches("//")
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .trim_start_matches('*')
                .trim_start_matches('#')
                .trim()
                .to_string()
        })
        .collect();

    let doc = cleaned.join("\n").trim().to_string();
    if doc.is_empty() {
        None
    } else {
        Some(doc)
    }
}
