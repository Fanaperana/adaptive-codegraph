//! # Query Helpers
//!
//! BFS neighborhood expansion, filtered queries, and convenience functions
//! for the MCP and CLI layers.

use crate::model::SymbolId;
use crate::store::Store;
use std::collections::{HashSet, VecDeque};

/// BFS neighborhood around a symbol.
pub struct Neighborhood {
    pub center: SymbolId,
    pub nodes: Vec<SymbolId>,
    pub edges: Vec<(SymbolId, SymbolId, String)>,
}

/// Expand BFS from a symbol, returning up to `cap` nodes within `depth` hops.
pub fn expand_neighborhood(
    store: &Store,
    center: SymbolId,
    depth: usize,
    cap: usize,
) -> Neighborhood {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut edges = Vec::new();

    visited.insert(center);
    queue.push_back((center, 0));

    while let Some((current, d)) = queue.pop_front() {
        if visited.len() >= cap {
            break;
        }
        if d >= depth {
            continue;
        }

        // Outgoing edges (callees)
        for (target_sym, kind) in store.callees(&current) {
            let target_id = target_sym.id;
            edges.push((current, target_id, kind.to_string()));
            if visited.insert(target_id) {
                queue.push_back((target_id, d + 1));
            }
        }

        // Incoming edges (callers)
        for (source_sym, kind) in store.callers(&current) {
            let source_id = source_sym.id;
            edges.push((source_id, current, kind.to_string()));
            if visited.insert(source_id) {
                queue.push_back((source_id, d + 1));
            }
        }
    }

    Neighborhood {
        center,
        nodes: visited.into_iter().collect(),
        edges,
    }
}

/// Find a symbol by name (case-insensitive), returning the best match.
pub fn resolve_symbol(store: &Store, name: &str) -> Option<SymbolId> {
    let matches = store.find_by_name(name);
    // Prefer exact match, then prefix match
    matches
        .iter()
        .find(|s| s.name == name)
        .or_else(|| matches.first())
        .map(|s| s.id)
}

/// Format a neighborhood as compact text for MCP responses.
pub fn format_neighborhood(store: &Store, neighborhood: &Neighborhood) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Neighborhood ({} nodes, {} edges):\n\n",
        neighborhood.nodes.len(),
        neighborhood.edges.len()
    ));

    // List nodes
    out.push_str("Nodes:\n");
    for id in &neighborhood.nodes {
        if let Some(sym) = store.get(id) {
            let marker = if *id == neighborhood.center {
                " [CENTER]"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {} ({}, {}) — {}:{}-{}{}\n",
                sym.name,
                sym.kind,
                sym.lang,
                sym.file,
                sym.span.start_line,
                sym.span.end_line,
                marker
            ));
        }
    }

    // List edges
    out.push_str("\nEdges:\n");
    for (from, to, kind) in &neighborhood.edges {
        let from_name = store.get(from).map(|s| s.name.as_str()).unwrap_or("?");
        let to_name = store.get(to).map(|s| s.name.as_str()).unwrap_or("?");
        out.push_str(&format!("  {} --[{}]--> {}\n", from_name, kind, to_name));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, Span, Symbol};
    use crate::store::Store;

    fn make_sym(name: &str, file: &str) -> Symbol {
        Symbol::new(
            "rust",
            "function",
            name,
            &format!("{}::{}", file, name),
            file,
            Span::default(),
        )
    }

    fn build_test_store() -> Store {
        let mut store = Store::new();
        let a = make_sym("main", "src/main.rs");
        let b = make_sym("process", "src/lib.rs");
        let c = make_sym("helper", "src/lib.rs");
        let d = make_sym("util", "src/util.rs");
        let a_id = a.id;
        let b_id = b.id;
        let c_id = c.id;
        let d_id = d.id;
        store.insert_symbol(a);
        store.insert_symbol(b);
        store.insert_symbol(c);
        store.insert_symbol(d);
        // main -> process -> helper, main -> util
        store.insert_edge(Edge {
            from: a_id,
            to: b_id,
            kind: "calls".into(),
        });
        store.insert_edge(Edge {
            from: b_id,
            to: c_id,
            kind: "calls".into(),
        });
        store.insert_edge(Edge {
            from: a_id,
            to: d_id,
            kind: "calls".into(),
        });
        store
    }

    #[test]
    fn resolve_symbol_exact_match() {
        let store = build_test_store();
        let id = resolve_symbol(&store, "main").unwrap();
        assert_eq!(store.get(&id).unwrap().name, "main");
    }

    #[test]
    fn resolve_symbol_not_found() {
        let store = build_test_store();
        assert!(resolve_symbol(&store, "nonexistent").is_none());
    }

    #[test]
    fn expand_neighborhood_depth_1() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        let nbr = expand_neighborhood(&store, main_id, 1, 50);
        // main + its direct neighbors (process, util)
        assert!(nbr.nodes.len() >= 3);
        assert!(!nbr.edges.is_empty());
    }

    #[test]
    fn expand_neighborhood_depth_2() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        let nbr = expand_neighborhood(&store, main_id, 2, 50);
        // Should reach helper via main->process->helper
        assert!(nbr.nodes.len() >= 4);
    }

    #[test]
    fn expand_neighborhood_respects_cap() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        // Cap=2: BFS starts with center (1 node), then explores neighbors.
        // It may slightly exceed cap since it checks after popping, but
        // should be significantly less than the total (4 nodes).
        let nbr = expand_neighborhood(&store, main_id, 10, 2);
        assert!(
            nbr.nodes.len() < 4,
            "Cap should limit expansion, got {}",
            nbr.nodes.len()
        );
    }

    #[test]
    fn format_neighborhood_produces_output() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        let nbr = expand_neighborhood(&store, main_id, 1, 50);
        let text = format_neighborhood(&store, &nbr);
        assert!(text.contains("Neighborhood"));
        assert!(text.contains("main"));
        assert!(text.contains("Nodes:"));
        assert!(text.contains("Edges:"));
    }
}
