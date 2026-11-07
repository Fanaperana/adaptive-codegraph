//! # Data Model
//!
//! Language-agnostic symbol and edge types. Kinds are **strings** so new
//! languages and edge types can be added via config files without recompiling.
//!
//! Indexing happens in two stages:
//! 1. **Extraction** (per file, parallel): a [`FileFacts`] holding the file's
//!    symbols plus *raw* facts — call sites ([`RawRef`]), imports
//!    ([`Import`]) and local variable types ([`Binding`]).
//! 2. **Resolution** (whole project): raw facts are turned into [`Edge`]s
//!    using scopes, imports and type hints (see `resolve.rs`).

use serde::{Deserialize, Serialize};
use std::fmt;

// ── Symbol ID ───────────────────────────────────────────────────────────

/// A stable 128-bit content-addressed identifier for a symbol.
///
/// Computed as `BLAKE3(lang + ":" + kind + ":" + fqname + ":" + file)`
/// truncated to 16 bytes. Stable across edits that don't rename/move.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
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

/// Byte and line range of a symbol in its source file. Lines are 0-based.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
}

impl Span {
    pub fn contains(&self, byte: usize) -> bool {
        self.start_byte <= byte && byte < self.end_byte
    }

    pub fn len(&self) -> usize {
        self.end_byte.saturating_sub(self.start_byte)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ── Symbol ──────────────────────────────────────────────────────────────

/// Symbol kinds that can be the target of a call.
pub const CALLABLE_KINDS: &[&str] = &["function", "method", "macro", "class", "struct"];

/// Symbol kinds that define a type (and can own members).
pub const TYPE_KINDS: &[&str] = &[
    "class", "struct", "interface", "trait", "enum", "union", "type", "type_alias", "impl",
];

/// A named entity extracted from source code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    /// Language identifier: "c", "python", "rust", "go", "javascript", etc.
    pub lang: String,
    /// Kind: "function", "method", "class", "struct", "field", "file", ...
    pub kind: String,
    /// Short display name, e.g. `process_patient`.
    pub name: String,
    /// Fully-qualified name, e.g. `src/patient.py::Patient::process`.
    pub fqname: String,
    /// Workspace-relative file path with forward slashes.
    pub file: String,
    /// Location in source.
    pub span: Span,
    /// Optional signature, e.g. `def process(self, data: dict) -> bool`.
    pub signature: Option<String>,
    /// Optional doc comment / docstring.
    pub doc: Option<String>,
    /// Enclosing scope path inside the file, e.g. `Store` or `Outer::Inner`.
    pub container: Option<String>,
    /// Declared type as written (e.g. `Vec<Box<dyn Shape>>`): return type for
    /// functions, field/variable type otherwise.
    pub type_hint: Option<String>,
    /// Innermost enclosing symbol (the file symbol for top-level items).
    pub parent: Option<SymbolId>,
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
            container: None,
            type_hint: None,
            parent: None,
        }
    }

    /// 1-based start line, for display.
    pub fn line(&self) -> u32 {
        self.span.start_line + 1
    }

    pub fn is_callable(&self) -> bool {
        CALLABLE_KINDS.contains(&self.kind.as_str())
    }

    pub fn is_type(&self) -> bool {
        TYPE_KINDS.contains(&self.kind.as_str())
    }

    /// The last segment of the container path (the owning type/module name).
    pub fn owner(&self) -> Option<&str> {
        self.container.as_deref().map(|c| c.rsplit("::").next().unwrap_or(c))
    }
}

// ── Edge ────────────────────────────────────────────────────────────────

/// How sure the resolver is about an edge.
pub mod confidence {
    /// Name-only guess (unique project-wide name, receiver type unknown).
    pub const GUESS: u8 = 1;
    /// Inferred (e.g. same-package lookup, ambiguous name narrowed by file).
    pub const INFERRED: u8 = 2;
    /// Resolved through scope, import, `self`, or a known receiver type.
    pub const CERTAIN: u8 = 3;
}

/// A directed relationship between two symbols.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: SymbolId,
    pub to: SymbolId,
    /// Relationship type: "calls", "imports", "inherits", "implements", etc.
    pub kind: String,
    /// See [`confidence`].
    pub confidence: u8,
    /// 0-based line of the first occurrence.
    pub line: u32,
    /// Number of occurrences merged into this edge.
    pub count: u32,
}

impl Edge {
    pub fn new(from: SymbolId, to: SymbolId, kind: &str) -> Self {
        Self {
            from,
            to,
            kind: kind.to_string(),
            confidence: confidence::CERTAIN,
            line: 0,
            count: 1,
        }
    }
}

// ── Raw facts (pre-resolution) ──────────────────────────────────────────

/// A reference (call, instantiation, ...) found in source, not yet resolved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawRef {
    /// Innermost symbol containing the reference.
    pub from: SymbolId,
    /// Referenced name, e.g. `save`.
    pub name: String,
    /// Edge kind to create, e.g. "calls".
    pub kind: String,
    /// Path qualifier, e.g. `Store` in `Store::new()` or `util` in `util::f()`.
    pub qualifier: Option<String>,
    /// Receiver expression, e.g. `self.items` in `self.items.push()`.
    pub receiver: Option<String>,
    /// Restrict targets to this symbol kind (e.g. "macro").
    pub target_kind: Option<String>,
    /// 0-based line.
    pub line: u32,
}

/// An import / use statement binding a local name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Import {
    /// The name bound in this file (alias, imported item, or module alias).
    pub local: String,
    /// Module path as written, e.g. `crate::util`, `./store`, `.util`.
    pub path: String,
    /// Imported item; `None` when the whole module is bound to `local`.
    pub item: Option<String>,
    /// 0-based line.
    pub line: u32,
}

/// A local variable or parameter whose type can be inferred.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// Innermost symbol containing the binding (its scope).
    pub scope: SymbolId,
    pub name: String,
    /// Declared or constructed type, e.g. `Store`.
    pub type_name: Option<String>,
    /// Callee expression whose return type gives the type, e.g. `Store::new`.
    pub call: Option<String>,
    /// Iterated expression: the type is its element type (`for x in xs`).
    pub iter: Option<String>,
}

/// Everything extracted from one file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileFacts {
    pub file: String,
    pub lang: String,
    /// Modification time (ns since epoch) and size, for change detection.
    pub mtime: u128,
    pub size: u64,
    pub symbols: Vec<Symbol>,
    pub refs: Vec<RawRef>,
    pub imports: Vec<Import>,
    pub bindings: Vec<Binding>,
}

// ── Extraction result ───────────────────────────────────────────────────

/// The output of parsing a single file.
#[derive(Clone, Debug, Default)]
pub struct ExtractionResult {
    pub symbols: Vec<Symbol>,
    /// Edges already resolved to concrete symbols (e.g. by plugins).
    pub edges: Vec<Edge>,
    pub refs: Vec<RawRef>,
    pub imports: Vec<Import>,
    pub bindings: Vec<Binding>,
}

impl ExtractionResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_symbol(&mut self, sym: Symbol) {
        self.symbols.push(sym);
    }

    /// Add an unresolved reference from `from` to a symbol called `to_name`.
    pub fn add_edge(&mut self, from: SymbolId, to_name: &str, kind: &str) {
        self.refs.push(RawRef {
            from,
            name: to_name.to_string(),
            kind: kind.to_string(),
            qualifier: None,
            receiver: None,
            target_kind: None,
            line: 0,
        });
    }

    pub fn add_resolved_edge(&mut self, from: SymbolId, to: SymbolId, kind: &str) {
        self.edges.push(Edge::new(from, to, kind));
    }

    pub fn merge(&mut self, other: ExtractionResult) {
        self.symbols.extend(other.symbols);
        self.edges.extend(other.edges);
        self.refs.extend(other.refs);
        self.imports.extend(other.imports);
        self.bindings.extend(other.bindings);
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

    #[test]
    fn owner_is_last_container_segment() {
        let mut s = Symbol::new("rust", "method", "new", "a.rs::m::Store::new", "a.rs", Span::default());
        s.container = Some("m::Store".into());
        assert_eq!(s.owner(), Some("Store"));
    }
}
