//! # Plugin System for Custom Edge Patterns
//!
//! Allows projects to register custom patterns that create cross-language
//! or domain-specific edges. For example:
//!
//! - Django: `path("users/", views.UserList.as_view())` → endpoint edge
//! - React Router: `<Route path="/users" component={UserList} />` → renders edge
//! - WebChart: `WCGetLayout(s, "E-Chart", "Summary")` → renders_layout edge
//! - Spring: `@GetMapping("/api/users")` → endpoint edge

use crate::model::{ExtractionResult, SymbolId};
use std::collections::HashMap;

/// A pattern that matches source text and produces edges.
pub trait EdgePattern: Send + Sync {
    /// Human-readable name for this pattern.
    fn name(&self) -> &str;

    /// Apply this pattern to extracted symbols and raw source content.
    /// May add new edges (or even new symbols) to the result.
    fn apply(
        &self,
        file_path: &str,
        content: &str,
        result: &mut ExtractionResult,
        symbol_index: &HashMap<String, SymbolId>,
    );
}

/// A simple regex-based edge pattern.
pub struct RegexEdgePattern {
    pub name: String,
    pub pattern: regex_lite::Regex,
    pub edge_kind: String,
    /// Capture group index for the source symbol name.
    pub from_group: usize,
    /// Capture group index for the target symbol name.
    pub to_group: usize,
}

impl EdgePattern for RegexEdgePattern {
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(
        &self,
        _file_path: &str,
        content: &str,
        result: &mut ExtractionResult,
        symbol_index: &HashMap<String, SymbolId>,
    ) {
        for cap in self.pattern.captures_iter(content) {
            let from_name = cap.get(self.from_group).map(|m| m.as_str());
            let to_name = cap.get(self.to_group).map(|m| m.as_str());

            if let (Some(from_name), Some(to_name)) = (from_name, to_name) {
                if let Some(&from_id) = symbol_index.get(from_name) {
                    result.add_edge(from_id, to_name, &self.edge_kind);
                }
            }
        }
    }
}

/// Registry of edge patterns.
pub struct PluginRegistry {
    patterns: Vec<Box<dyn EdgePattern>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    pub fn register(&mut self, pattern: Box<dyn EdgePattern>) {
        self.patterns.push(pattern);
    }

    /// Apply all registered patterns to a file's extraction result.
    pub fn apply_all(
        &self,
        file_path: &str,
        content: &str,
        result: &mut ExtractionResult,
        symbol_index: &HashMap<String, SymbolId>,
    ) {
        for pattern in &self.patterns {
            pattern.apply(file_path, content, result, symbol_index);
        }
    }

    pub fn pattern_names(&self) -> Vec<&str> {
        self.patterns.iter().map(|p| p.name()).collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
