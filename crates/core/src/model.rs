//! # Data Model
//!
//! Language-agnostic symbol and edge types. Unlike mie-codegraph which uses
//! fixed Rust enums for `SymbolKind` and `EdgeKind`, adaptive-codegraph uses
//! **string-based kinds** so new languages and edge types can be added via
//! config files without recompiling.

use serde::{Deserialize, Serialize};
use std::fmt;

// ── Symbol ID ───────────────────────────────────────────────────────────

/// A stable 128-bit content-addressed identifier for a symbol.
///
/// Computed as `BLAKE3(lang + ":" + kind + ":" + fqname + ":" + file)`
/// truncated to 16 bytes. Stable across edits that don't rename/move.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct SymbolId(pub [u8; 16]);

impl SymbolId {
    /// Create a new SymbolId from the identifying components.
    pub fn new(lang: &str, kind: &str, fqname: &str, file: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(lang.as_bytes());
        hasher.update(b":");
        hasher.update(kind.as_bytes());
        hasher.update(b":");
        hasher.update(fqname.as_bytes());
        hasher.update(b":");
        hasher.update(file.as_bytes());
        let hash = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash.as_bytes()[..16]);
        Self(bytes)
    }

    /// Display as 32-char hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Parse from a 32-char hex string.
    pub fn from_hex(s: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(s)?;
        anyhow::ensure!(bytes.len() == 16, "expected 32 hex chars");
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl fmt::Debug for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

// ── Inline hex helper ───────────────────────────────────────────────────

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn decode(s: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(s.len() % 2 == 0, "odd-length hex string");
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
            .collect()
    }
}

// ── Span ────────────────────────────────────────────────────────────────

/// Byte and line range of a symbol in its source file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
}

// ── Symbol ──────────────────────────────────────────────────────────────

/// A named entity extracted from source code.
///
/// `kind` and `lang` are free-form strings defined by language configs,
/// e.g. `kind = "function"`, `lang = "python"`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    /// Language identifier: "c", "python", "rust", "go", "javascript", etc.
    pub lang: String,
    /// Kind: "function", "class", "struct", "method", "module", "table", etc.
    pub kind: String,
    /// Short display name, e.g. `process_patient`.
    pub name: String,
    /// Fully-qualified name for disambiguation, e.g. `src/patient.py::Patient::process`.
    pub fqname: String,
    /// Workspace-relative file path.
    pub file: String,
    /// Location in source.
    pub span: Span,
    /// Optional signature, e.g. `def process(self, data: dict) -> bool`.
    pub signature: Option<String>,
    /// Optional doc comment / docstring.
    pub doc: Option<String>,
}

impl Symbol {
    /// Create a new symbol, computing its stable ID automatically.
    pub fn new(
        lang: impl Into<String>,
        kind: impl Into<String>,
        name: impl Into<String>,
        fqname: impl Into<String>,
        file: impl Into<String>,
        span: Span,
    ) -> Self {
        let lang = lang.into();
        let kind = kind.into();
        let name = name.into();
        let fqname = fqname.into();
        let file = file.into();
        let id = SymbolId::new(&lang, &kind, &fqname, &file);
        Self {
            id,
            lang,
            kind,
            name,
            fqname,
            file,
            span,
            signature: None,
            doc: None,
        }
    }
}

// ── Edge ────────────────────────────────────────────────────────────────

/// A directed relationship between two symbols.
///
/// `kind` is a free-form string: "calls", "imports", "inherits",
/// "implements", "renders", "reads_table", "defines", etc.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub from: SymbolId,
    pub to: SymbolId,
    /// Relationship type: "calls", "imports", "inherits", "implements", etc.
    pub kind: String,
}

/// An edge whose target hasn't been resolved to a SymbolId yet.
#[derive(Clone, Debug)]
pub struct UnresolvedEdge {
    pub from: SymbolId,
    pub to_name: String,
    pub kind: String,
}

// ── Extraction result ───────────────────────────────────────────────────

/// The output of parsing a single file: symbols found + edges discovered.
#[derive(Clone, Debug, Default)]
pub struct ExtractionResult {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub unresolved_edges: Vec<UnresolvedEdge>,
}

impl ExtractionResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_symbol(&mut self, sym: Symbol) {
        self.symbols.push(sym);
    }

    pub fn add_edge(&mut self, from: SymbolId, to_name: &str, kind: &str) {
        self.unresolved_edges.push(UnresolvedEdge {
            from,
            to_name: to_name.to_string(),
            kind: kind.to_string(),
        });
    }

    pub fn add_resolved_edge(&mut self, from: SymbolId, to: SymbolId, kind: &str) {
        self.edges.push(Edge {
            from,
            to,
            kind: kind.to_string(),
        });
    }

    pub fn merge(&mut self, other: ExtractionResult) {
        self.symbols.extend(other.symbols);
        self.edges.extend(other.edges);
        self.unresolved_edges.extend(other.unresolved_edges);
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_id_stable() {
        let a = SymbolId::new("python", "function", "src/main.py::main", "src/main.py");
        let b = SymbolId::new("python", "function", "src/main.py::main", "src/main.py");
        assert_eq!(a, b);
    }

    #[test]
    fn symbol_id_hex_roundtrip() {
        let id = SymbolId::new("rust", "function", "lib.rs::run", "src/lib.rs");
        let hex = id.to_hex();
        assert_eq!(hex.len(), 32);
        let back = SymbolId::from_hex(&hex).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn different_inputs_different_ids() {
        let a = SymbolId::new("c", "function", "main", "main.c");
        let b = SymbolId::new("c", "function", "main", "other.c");
        assert_ne!(a, b);
    }
}
