//! # Graph Store
//!
//! Holds per-file facts plus the resolved symbol graph (petgraph) with side
//! indexes for O(1) lookup by ID, file, and name. Persistence via bincode.

use crate::extract::LangProfile;
use crate::model::{Edge, FileFacts, Symbol, SymbolId};
use crate::resolve::{self, ResolveStats};
use ahash::AHashMap;
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Bump when the on-disk format changes; older indexes must be rebuilt.
pub const FORMAT_VERSION: u32 = 2;

/// Data carried by a graph edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeData {
    pub kind: String,
    /// 1 = guess, 2 = inferred, 3 = certain. See `model::confidence`.
    pub confidence: u8,
    /// 0-based line of the first occurrence.
    pub line: u32,
    /// Number of occurrences.
    pub count: u32,
}

#[derive(Serialize, Deserialize)]
struct SerializedStore {
    version: u32,
    facts: Vec<FileFacts>,
    edges: Vec<Edge>,
    profiles: HashMap<String, LangProfile>,
    stats: ResolveStats,
}

/// The in-memory graph with side indexes.
#[derive(Default)]
pub struct Store {
    pub graph: StableDiGraph<Symbol, EdgeData>,
    pub by_id: AHashMap<SymbolId, NodeIndex>,
    pub by_file: AHashMap<String, Vec<SymbolId>>,
    pub by_name_ci: AHashMap<String, Vec<SymbolId>>,
    /// Raw per-file facts, keyed by workspace-relative path.
    pub facts: BTreeMap<String, FileFacts>,
    pub profiles: HashMap<String, LangProfile>,
    pub stats: ResolveStats,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a store from per-file facts, resolving all references.
    pub fn from_facts(
        facts: BTreeMap<String, FileFacts>,
        profiles: HashMap<String, LangProfile>,
    ) -> Self {
        let mut s = Self {
            facts,
            profiles,
            ..Default::default()
        };
        s.rebuild();
        s
    }

    /// Rebuild the graph from `facts` (re-runs resolution).
    pub fn rebuild(&mut self) {
        self.graph = StableDiGraph::new();
        self.by_id.clear();
        self.by_file.clear();
        self.by_name_ci.clear();
        let syms: Vec<Symbol> = self.facts.values().flat_map(|f| f.symbols.iter().cloned()).collect();
        for s in syms {
            self.insert_symbol(s);
        }
        let (edges, stats) = resolve::resolve(&self.facts, &self.profiles);
        for e in edges {
            self.add_edge_raw(e);
        }
        self.stats = stats;
    }

    /// Insert a symbol into the graph. Returns the node index. Inserting an
    /// ID that already exists returns the existing node.
    pub fn insert_symbol(&mut self, sym: Symbol) -> NodeIndex {
        if let Some(&nx) = self.by_id.get(&sym.id) {
            return nx;
        }
        let id = sym.id;
        let file = sym.file.clone();
        let name_lower = sym.name.to_lowercase();

        let nx = self.graph.add_node(sym);
        self.by_id.insert(id, nx);
        self.by_file.entry(file).or_default().push(id);
        self.by_name_ci.entry(name_lower).or_default().push(id);
        nx
    }

    fn add_edge_raw(&mut self, edge: Edge) -> bool {
        let (Some(&a), Some(&b)) = (self.by_id.get(&edge.from), self.by_id.get(&edge.to)) else {
            return false;
        };
        self.graph.add_edge(
            a,
            b,
            EdgeData {
                kind: edge.kind,
                confidence: edge.confidence,
                line: edge.line,
                count: edge.count,
            },
        );
        true
    }

    /// Insert an edge, merging with an existing edge of the same kind between
    /// the same symbols. Both endpoints must already exist.
    pub fn insert_edge(&mut self, edge: Edge) -> bool {
        let (Some(&a), Some(&b)) = (self.by_id.get(&edge.from), self.by_id.get(&edge.to)) else {
            return false;
        };
        let existing = self
            .graph
            .edges_connecting(a, b)
            .find(|e| e.weight().kind == edge.kind)
            .map(|e| e.id());
        if let Some(eid) = existing {
            let w = &mut self.graph[eid];
            w.confidence = w.confidence.max(edge.confidence);
            w.line = w.line.min(edge.line);
            w.count += edge.count;
            return true;
        }
        self.add_edge_raw(edge)
    }

    /// Look up a symbol by ID.
    pub fn get(&self, id: &SymbolId) -> Option<&Symbol> {
        self.by_id.get(id).map(|&nx| &self.graph[nx])
    }

    /// Find symbols whose name contains `needle` (case-insensitive).
    pub fn find_by_name(&self, needle: &str) -> Vec<&Symbol> {
        let needle_lower = needle.to_lowercase();
        let mut results = Vec::new();
        for (name, ids) in &self.by_name_ci {
            if name.contains(&needle_lower) {
                for id in ids {
                    if let Some(sym) = self.get(id) {
                        results.push(sym);
                    }
                }
            }
        }
        // Exact matches first, then shorter names, then stable order.
        results.sort_by(|a, b| {
            let ea = a.name.eq_ignore_ascii_case(needle);
            let eb = b.name.eq_ignore_ascii_case(needle);
            eb.cmp(&ea)
                .then(a.name.len().cmp(&b.name.len()))
                .then(a.fqname.cmp(&b.fqname))
        });
        results
    }

    /// Find symbols whose name matches `needle` exactly (case-insensitive).
    pub fn find_by_name_exact(&self, needle: &str) -> Vec<&Symbol> {
        let needle_lower = needle.to_lowercase();
        if let Some(ids) = self.by_name_ci.get(&needle_lower) {
            ids.iter().filter_map(|id| self.get(id)).collect()
        } else {
            Vec::new()
        }
    }

    /// Find symbols by name, filtered by kind and/or language.
    pub fn find_filtered(
        &self,
        needle: &str,
        kind: Option<&str>,
        lang: Option<&str>,
    ) -> Vec<&Symbol> {
        self.find_by_name(needle)
            .into_iter()
            .filter(|s| kind.map_or(true, |k| s.kind == k))
            .filter(|s| lang.map_or(true, |l| s.lang == l))
            .collect()
    }

    /// All incoming edges of a symbol.
    pub fn edges_in(&self, id: &SymbolId) -> Vec<(&Symbol, &EdgeData)> {
        let Some(&nx) = self.by_id.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(nx, Direction::Incoming)
            .map(|e| (&self.graph[e.source()], e.weight()))
            .collect()
    }

    /// All outgoing edges of a symbol.
    pub fn edges_out(&self, id: &SymbolId) -> Vec<(&Symbol, &EdgeData)> {
        let Some(&nx) = self.by_id.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(nx, Direction::Outgoing)
            .map(|e| (&self.graph[e.target()], e.weight()))
            .collect()
    }

    /// Symbols that call the given symbol.
    pub fn callers(&self, id: &SymbolId) -> Vec<(&Symbol, &EdgeData)> {
        let mut v: Vec<_> = self.edges_in(id).into_iter().filter(|(_, e)| e.kind == "calls").collect();
        v.sort_by(|a, b| (&a.0.file, a.1.line).cmp(&(&b.0.file, b.1.line)));
        v
    }

    /// Symbols called by the given symbol.
    pub fn callees(&self, id: &SymbolId) -> Vec<(&Symbol, &EdgeData)> {
        let mut v: Vec<_> = self.edges_out(id).into_iter().filter(|(_, e)| e.kind == "calls").collect();
        v.sort_by_key(|(_, e)| e.line);
        v
    }

    /// Direct children (members) of a symbol.
    pub fn children(&self, id: &SymbolId) -> Vec<&Symbol> {
        let Some(sym) = self.get(id) else { return Vec::new() };
        let mut v: Vec<&Symbol> = self
            .by_file
            .get(&sym.file)
            .map(|ids| ids.iter().filter_map(|i| self.get(i)).filter(|s| s.parent == Some(*id)).collect())
            .unwrap_or_default();
        v.sort_by_key(|s| s.span.start_byte);
        v
    }

    /// Every edge in the graph.
    pub fn all_edges(&self) -> Vec<Edge> {
        self.graph
            .edge_references()
            .map(|e| Edge {
                from: self.graph[e.source()].id,
                to: self.graph[e.target()].id,
                kind: e.weight().kind.clone(),
                confidence: e.weight().confidence,
                line: e.weight().line,
                count: e.weight().count,
            })
            .collect()
    }

    /// Remove all symbols from a given file. Returns the number removed.
    pub fn remove_file(&mut self, file: &str) -> usize {
        self.facts.remove(file);
        let mut removed = 0;
        if let Some(ids) = self.by_file.remove(file) {
            for id in &ids {
                if let Some(nx) = self.by_id.remove(id) {
                    let name = self.graph[nx].name.to_lowercase();
                    self.graph.remove_node(nx);
                    removed += 1;
                    if let Some(v) = self.by_name_ci.get_mut(&name) {
                        v.retain(|x| x != id);
                    }
                }
            }
        }
        removed
    }

    pub fn symbol_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Total number of edges.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn file_count(&self) -> usize {
        self.facts.len()
    }

    /// Serialize to bytes for persistence.
    pub fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        let ss = SerializedStore {
            version: FORMAT_VERSION,
            facts: self.facts.values().cloned().collect(),
            edges: self.all_edges(),
            profiles: self.profiles.clone(),
            stats: self.stats.clone(),
        };
        Ok(bincode::serialize(&ss)?)
    }

    /// Deserialize from bytes (no re-resolution needed).
    pub fn deserialize(data: &[u8]) -> anyhow::Result<Self> {
        let version: u32 = bincode::deserialize(data.get(..4).unwrap_or(&[])).unwrap_or(0);
        anyhow::ensure!(
            version == FORMAT_VERSION,
            "index format v{version} is outdated (need v{FORMAT_VERSION}); rebuild with `adaptive-codegraph index`"
        );
        let ss: SerializedStore = bincode::deserialize(data)?;
        let mut store = Self {
            facts: ss.facts.into_iter().map(|f| (f.file.clone(), f)).collect(),
            profiles: ss.profiles,
            stats: ss.stats,
            ..Default::default()
        };
        let syms: Vec<Symbol> = store.facts.values().flat_map(|f| f.symbols.iter().cloned()).collect();
        for s in syms {
            store.insert_symbol(s);
        }
        for e in ss.edges {
            store.add_edge_raw(e);
        }
        Ok(store)
    }

    /// Save to a file.
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let data = self.serialize()?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load from a file.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        Self::deserialize(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, Span, Symbol};

    fn make_sym(lang: &str, kind: &str, name: &str, file: &str) -> Symbol {
        Symbol::new(
            lang,
            kind,
            name,
            &format!("{}::{}", file, name),
            file,
            Span::default(),
        )
    }

    #[test]
    fn insert_and_get() {
        let mut store = Store::new();
        let sym = make_sym("rust", "function", "main", "src/main.rs");
        let id = sym.id;
        store.insert_symbol(sym);
        assert_eq!(store.symbol_count(), 1);
        let got = store.get(&id).unwrap();
        assert_eq!(got.name, "main");
    }

    #[test]
    fn duplicate_ids_are_inserted_once() {
        let mut store = Store::new();
        store.insert_symbol(make_sym("rust", "function", "main", "src/main.rs"));
        store.insert_symbol(make_sym("rust", "function", "main", "src/main.rs"));
        assert_eq!(store.symbol_count(), 1);
    }

    #[test]
    fn find_by_name_case_insensitive() {
        let mut store = Store::new();
        store.insert_symbol(make_sym("rust", "function", "MyFunc", "lib.rs"));
        let results = store.find_by_name("myfunc");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "MyFunc");
    }

    #[test]
    fn find_by_name_exact() {
        let mut store = Store::new();
        store.insert_symbol(make_sym("python", "function", "process", "a.py"));
        store.insert_symbol(make_sym("python", "function", "process_data", "b.py"));
        let exact = store.find_by_name_exact("process");
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].name, "process");
    }

    #[test]
    fn find_filtered_by_kind_and_lang() {
        let mut store = Store::new();
        store.insert_symbol(make_sym("rust", "function", "run", "lib.rs"));
        store.insert_symbol(make_sym("rust", "struct", "Config", "lib.rs"));
        store.insert_symbol(make_sym("python", "function", "run", "main.py"));

        let fns = store.find_filtered("run", Some("function"), None);
        assert_eq!(fns.len(), 2);

        let rust_fns = store.find_filtered("run", Some("function"), Some("rust"));
        assert_eq!(rust_fns.len(), 1);
        assert_eq!(rust_fns[0].lang, "rust");
    }

    #[test]
    fn insert_and_query_edges() {
        let mut store = Store::new();
        let a = make_sym("rust", "function", "caller", "lib.rs");
        let b = make_sym("rust", "function", "callee", "lib.rs");
        let a_id = a.id;
        let b_id = b.id;
        store.insert_symbol(a);
        store.insert_symbol(b);

        assert!(store.insert_edge(Edge::new(a_id, b_id, "calls")));
        assert!(store.insert_edge(Edge::new(a_id, b_id, "calls")));
        assert_eq!(store.edge_count(), 1, "same edge is merged");

        let callees = store.callees(&a_id);
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].0.name, "callee");
        assert_eq!(callees[0].1.kind, "calls");
        assert_eq!(callees[0].1.count, 2);

        let callers = store.callers(&b_id);
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].0.name, "caller");
    }

    #[test]
    fn insert_edge_missing_endpoint_returns_false() {
        let mut store = Store::new();
        let a = make_sym("rust", "function", "a", "lib.rs");
        let a_id = a.id;
        store.insert_symbol(a);
        let fake_id = SymbolId::new("rust", "function", "nonexistent", "x.rs");
        assert!(!store.insert_edge(Edge::new(a_id, fake_id, "calls")));
    }

    #[test]
    fn remove_file() {
        let mut store = Store::new();
        store.insert_symbol(make_sym("rust", "function", "a", "lib.rs"));
        store.insert_symbol(make_sym("rust", "function", "b", "lib.rs"));
        store.insert_symbol(make_sym("rust", "function", "c", "main.rs"));
        assert_eq!(store.symbol_count(), 3);

        let removed = store.remove_file("lib.rs");
        assert_eq!(removed, 2);
        assert_eq!(store.symbol_count(), 1);
        assert!(store.find_by_name("a").is_empty());
        assert_eq!(store.find_by_name("c").len(), 1);
    }

    fn facts_store() -> (Store, SymbolId, SymbolId) {
        let a = make_sym("rust", "function", "alpha", "lib.rs");
        let b = make_sym("rust", "function", "beta", "lib.rs");
        let (a_id, b_id) = (a.id, b.id);
        let mut facts = BTreeMap::new();
        facts.insert(
            "lib.rs".to_string(),
            FileFacts {
                file: "lib.rs".into(),
                lang: "rust".into(),
                symbols: vec![a, b],
                ..Default::default()
            },
        );
        let mut store = Store::from_facts(facts, HashMap::new());
        store.insert_edge(Edge::new(a_id, b_id, "calls"));
        (store, a_id, b_id)
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let (store, a_id, _) = facts_store();
        let bytes = store.serialize().unwrap();
        let restored = Store::deserialize(&bytes).unwrap();
        assert_eq!(restored.symbol_count(), 2);
        assert_eq!(restored.edge_count(), 1);
        assert_eq!(restored.get(&a_id).unwrap().name, "alpha");
        assert_eq!(restored.file_count(), 1);
    }

    #[test]
    fn rejects_old_format() {
        assert!(Store::deserialize(&[1, 0, 0, 0, 9, 9]).is_err());
    }

    #[test]
    fn save_load_file() {
        let (store, _, _) = facts_store();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("graph.bin");
        store.save(&path).unwrap();

        let loaded = Store::load(&path).unwrap();
        assert_eq!(loaded.symbol_count(), 2);
        assert_eq!(loaded.find_by_name("alpha").len(), 1);
    }

    #[test]
    fn by_file_index() {
        let mut store = Store::new();
        store.insert_symbol(make_sym("c", "function", "init", "core.c"));
        store.insert_symbol(make_sym("c", "function", "run", "core.c"));
        store.insert_symbol(make_sym("c", "function", "main", "main.c"));

        let core_ids = store.by_file.get("core.c").unwrap();
        assert_eq!(core_ids.len(), 2);
    }
}
