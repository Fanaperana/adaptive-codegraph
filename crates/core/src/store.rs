//! # In-Memory Graph Store
//!
//! Holds the symbol graph (petgraph) with side-table indexes for O(1) lookup
//! by ID, file, and name. Persistence via bincode.

use crate::model::{Edge, Symbol, SymbolId};
use ahash::AHashMap;
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::Direction;
use serde::{Deserialize, Serialize};

/// Serialization-friendly snapshot of the graph.
#[derive(Serialize, Deserialize)]
pub struct SerializedStore {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
}

/// The in-memory graph with side indexes.
pub struct Store {
    pub graph: StableDiGraph<Symbol, String>,
    pub by_id: AHashMap<SymbolId, NodeIndex>,
    pub by_file: AHashMap<String, Vec<SymbolId>>,
    pub by_name_ci: AHashMap<String, Vec<SymbolId>>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            graph: StableDiGraph::new(),
            by_id: AHashMap::new(),
            by_file: AHashMap::new(),
            by_name_ci: AHashMap::new(),
        }
    }

    /// Insert a symbol into the graph. Returns the node index.
    pub fn insert_symbol(&mut self, sym: Symbol) -> NodeIndex {
        let id = sym.id;
        let file = sym.file.clone();
        let name_lower = sym.name.to_lowercase();

        let nx = self.graph.add_node(sym);
        self.by_id.insert(id, nx);
        self.by_file.entry(file).or_default().push(id);
        self.by_name_ci.entry(name_lower).or_default().push(id);
        nx
    }

    /// Insert an edge. Both endpoints must already exist in the graph.
    pub fn insert_edge(&mut self, edge: Edge) -> bool {
        if let (Some(&from_nx), Some(&to_nx)) =
            (self.by_id.get(&edge.from), self.by_id.get(&edge.to))
        {
            self.graph.add_edge(from_nx, to_nx, edge.kind.clone());
            true
        } else {
            false
        }
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

    /// Get all symbols that call/reference the given symbol (incoming edges).
    pub fn callers(&self, id: &SymbolId) -> Vec<(&Symbol, &str)> {
        let Some(&nx) = self.by_id.get(id) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(nx, Direction::Incoming)
            .filter_map(|caller_nx| {
                let edge_ref = self
                    .graph
                    .edges_connecting(caller_nx, nx)
                    .next()?;
                Some((&self.graph[caller_nx], edge_ref.weight().as_str()))
            })
            .collect()
    }

    /// Get all symbols that the given symbol calls/references (outgoing edges).
    pub fn callees(&self, id: &SymbolId) -> Vec<(&Symbol, &str)> {
        let Some(&nx) = self.by_id.get(id) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(nx, Direction::Outgoing)
            .filter_map(|callee_nx| {
                let edge_ref = self
                    .graph
                    .edges_connecting(nx, callee_nx)
                    .next()?;
                Some((&self.graph[callee_nx], edge_ref.weight().as_str()))
            })
            .collect()
    }

    /// Remove all symbols from a given file (for incremental reindex).
    /// Returns the number of symbols removed.
    pub fn remove_file(&mut self, file: &str) -> usize {
        let mut removed = 0;
        if let Some(ids) = self.by_file.remove(file) {
            for id in &ids {
                if let Some(nx) = self.by_id.remove(id) {
                    // Get name before removing node
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

    /// Serialize to bytes for persistence.
    pub fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        let symbols: Vec<Symbol> = self.graph.node_weights().cloned().collect();
        let edges: Vec<Edge> = self
            .graph
            .edge_indices()
            .filter_map(|eix| {
                let (a, b) = self.graph.edge_endpoints(eix)?;
                Some(Edge {
                    from: self.graph[a].id,
                    to: self.graph[b].id,
                    kind: self.graph[eix].clone(),
                })
            })
            .collect();
        let ss = SerializedStore { symbols, edges };
        Ok(bincode::serialize(&ss)?)
    }

    /// Deserialize from bytes.
    pub fn deserialize(data: &[u8]) -> anyhow::Result<Self> {
        let ss: SerializedStore = bincode::deserialize(data)?;
        let mut store = Self::new();
        for sym in ss.symbols {
            store.insert_symbol(sym);
        }
        for edge in ss.edges {
            store.insert_edge(edge);
        }
        Ok(store)
    }

    /// Save to a file.
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let data = self.serialize()?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Load from a file.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        Self::deserialize(&data)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}
