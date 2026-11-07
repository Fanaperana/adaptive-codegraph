//! # Query Helpers
//!
//! Symbol lookup by (qualified) name, BFS neighborhoods, and compact text
//! formatting for the MCP and CLI layers. Lines are shown 1-based.

use crate::model::{Symbol, SymbolId};
use crate::store::{EdgeData, Store};
use std::collections::{HashSet, VecDeque};

/// BFS neighborhood around a symbol.
pub struct Neighborhood {
    pub center: SymbolId,
    pub nodes: Vec<SymbolId>,
    pub edges: Vec<(SymbolId, SymbolId, EdgeData)>,
}

/// Expand BFS from a symbol, returning up to `cap` nodes within `depth` hops.
/// Only edges between returned nodes are included.
pub fn expand_neighborhood(store: &Store, center: SymbolId, depth: usize, cap: usize) -> Neighborhood {
    let mut visited: Vec<SymbolId> = vec![center];
    let mut seen: HashSet<SymbolId> = HashSet::from([center]);
    let mut queue = VecDeque::from([(center, 0usize)]);

    'bfs: while let Some((current, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }
        let mut next: Vec<(&Symbol, &EdgeData)> = store.edges_out(&current);
        next.extend(store.edges_in(&current));
        // Most certain, most used relationships first.
        next.sort_by(|a, b| {
            b.1.confidence
                .cmp(&a.1.confidence)
                .then(b.1.count.cmp(&a.1.count))
                .then(a.0.fqname.cmp(&b.0.fqname))
        });
        for (sym, _) in next {
            if sym.kind == "file" && sym.id != center {
                continue; // file nodes are hubs, not useful context
            }
            if seen.insert(sym.id) {
                if visited.len() >= cap {
                    break 'bfs;
                }
                visited.push(sym.id);
                queue.push_back((sym.id, d + 1));
            }
        }
    }

    let mut edges = Vec::new();
    for id in &visited {
        for (t, e) in store.edges_out(id) {
            if seen.contains(&t.id) && visited.contains(&t.id) {
                edges.push((*id, t.id, e.clone()));
            }
        }
    }

    Neighborhood {
        center,
        nodes: visited,
        edges,
    }
}

/// Display name with its owning scope, e.g. `Store::save`.
pub fn display_name(sym: &Symbol) -> String {
    match sym.owner() {
        Some(o) if sym.kind != "file" => format!("{o}::{}", sym.name),
        _ => sym.name.clone(),
    }
}

/// `file:line` (1-based).
pub fn location(sym: &Symbol) -> String {
    format!("{}:{}", sym.file, sym.line())
}

/// Find symbols matching a name. Accepts plain names (`save`), qualified
/// names (`Store::save`, `Store.save`) and file-qualified names
/// (`src/store.rs::Store::save`). Best matches first.
pub fn resolve_symbols<'a>(store: &'a Store, query: &str) -> Vec<&'a Symbol> {
    let q = query.trim().replace('.', "::").replace("::::", "::");
    let q = q.trim_matches(':');
    let (scope, name) = match q.rsplit_once("::") {
        Some((s, n)) => (Some(s), n),
        None => (None, q),
    };
    // Accept "file.rs::name" forms: the '.' replacement broke the extension.
    let raw_suffix = query.trim();

    let mut matches: Vec<&Symbol> = store
        .find_by_name_exact(name)
        .into_iter()
        .filter(|s| match scope {
            None => true,
            Some(sc) => {
                let full = format!("{}::{}", s.container.as_deref().unwrap_or(""), s.name);
                full.ends_with(&format!("{sc}::{name}"))
                    || s.fqname.ends_with(raw_suffix)
                    || s.fqname.replace('.', "::").ends_with(q)
            }
        })
        .collect();
    if matches.is_empty() && scope.is_none() {
        matches = store.find_by_name(name);
    }

    let rank = |s: &Symbol| -> (bool, u8, std::cmp::Reverse<usize>) {
        let kind_rank = match s.kind.as_str() {
            "function" | "method" => 0,
            "class" | "struct" | "interface" | "trait" | "enum" | "type" | "type_alias" => 1,
            "file" | "impl" => 3,
            _ => 2,
        };
        let exact = s.name == name;
        (!exact, kind_rank, std::cmp::Reverse(store.edges_in(&s.id).len()))
    };
    matches.sort_by_key(|s| rank(s));
    matches
}

/// Ranked search: BM25 candidates reranked by symbol kind and by how much
/// the rest of the code depends on the symbol.
pub fn search<'a>(
    store: &'a Store,
    index: &crate::search::SearchIndex,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<(&'a Symbol, f32)>> {
    let hits = index.search(query, (limit * 5).max(50))?;
    let mut out: Vec<(&Symbol, f32)> = hits
        .into_iter()
        .filter_map(|h| {
            let s = store.get(&h.id)?;
            let kind_weight = match s.kind.as_str() {
                "function" | "method" => 1.0,
                "class" | "struct" | "interface" | "trait" | "enum" | "type" | "type_alias" => 0.95,
                "module" | "macro" => 0.85,
                "field" | "variable" | "constant" | "static" => 0.7,
                "impl" | "enum_variant" => 0.6,
                _ => 0.5,
            };
            let used_by = store
                .edges_in(&s.id)
                .iter()
                .filter(|(_, e)| e.kind != "imports")
                .count() as f32;
            Some((s, h.score * kind_weight * (1.0 + 0.1 * (1.0 + used_by).ln())))
        })
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    Ok(out)
}

/// Hybrid search: lexical ([`search`]) and embedding similarity fused with
/// reciprocal-rank fusion. Falls back to lexical search when no semantic
/// model / vectors are available.
pub fn hybrid_search<'a>(
    store: &'a Store,
    index: &crate::search::SearchIndex,
    vectors: Option<&crate::embed::VectorIndex>,
    embedder: Option<&dyn crate::embed::Embedder>,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<(&'a Symbol, f32)>> {
    const POOL: usize = 50;
    const K: f32 = 60.0;
    let lexical = search(store, index, query, POOL)?;
    let (Some(vectors), Some(embedder)) = (vectors, embedder) else {
        return Ok(lexical.into_iter().take(limit).collect());
    };
    if !embedder.is_semantic() || vectors.is_empty() {
        return Ok(lexical.into_iter().take(limit).collect());
    }
    let qv = embedder.embed_one(query)?;
    let semantic = vectors.search(&qv, POOL);

    let mut fused: std::collections::HashMap<SymbolId, f32> = std::collections::HashMap::new();
    for (rank, (s, _)) in lexical.iter().enumerate() {
        *fused.entry(s.id).or_default() += 1.0 / (K + rank as f32 + 1.0);
    }
    for (rank, (id, _)) in semantic.iter().enumerate() {
        *fused.entry(*id).or_default() += 1.0 / (K + rank as f32 + 1.0);
    }
    let mut out: Vec<(&Symbol, f32)> =
        fused.into_iter().filter_map(|(id, s)| store.get(&id).map(|sym| (sym, s))).collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    Ok(out)
}

/// Best single match for a name (see [`resolve_symbols`]).
pub fn resolve_symbol(store: &Store, name: &str) -> Option<SymbolId> {
    resolve_symbols(store, name).first().map(|s| s.id)
}

fn conf_mark(c: u8) -> &'static str {
    match c {
        3 => "",
        2 => " (inferred)",
        _ => " (guess)",
    }
}

/// Format a neighborhood as compact text.
pub fn format_neighborhood(store: &Store, n: &Neighborhood) -> String {
    let mut out = String::new();
    if let Some(c) = store.get(&n.center) {
        out.push_str(&format!(
            "Neighborhood of {} ({}) - {} nodes, {} edges\n",
            display_name(c),
            location(c),
            n.nodes.len(),
            n.edges.len()
        ));
    }
    out.push_str("Nodes:\n");
    for id in &n.nodes {
        if let Some(s) = store.get(id) {
            let marker = if *id == n.center { "  [center]" } else { "" };
            out.push_str(&format!(
                "  {} {} - {}-{}{}\n",
                s.kind,
                display_name(s),
                location(s),
                s.span.end_line + 1,
                marker
            ));
        }
    }
    out.push_str("Edges:\n");
    for (from, to, e) in &n.edges {
        let f = store.get(from).map(display_name).unwrap_or_else(|| "?".into());
        let t = store.get(to).map(display_name).unwrap_or_else(|| "?".into());
        let times = if e.count > 1 { format!(" x{}", e.count) } else { String::new() };
        out.push_str(&format!(
            "  {f} -{}-> {t}  (line {}{times}){}\n",
            e.kind,
            e.line + 1,
            conf_mark(e.confidence)
        ));
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
        let (a_id, b_id, c_id, d_id) = (a.id, b.id, c.id, d.id);
        store.insert_symbol(a);
        store.insert_symbol(b);
        store.insert_symbol(c);
        store.insert_symbol(d);
        // main -> process -> helper, main -> util
        store.insert_edge(Edge::new(a_id, b_id, "calls"));
        store.insert_edge(Edge::new(b_id, c_id, "calls"));
        store.insert_edge(Edge::new(a_id, d_id, "calls"));
        store
    }

    #[test]
    fn resolve_symbol_exact_match() {
        let store = build_test_store();
        let id = resolve_symbol(&store, "main").unwrap();
        assert_eq!(store.get(&id).unwrap().name, "main");
    }

    #[test]
    fn resolve_symbol_qualified() {
        let mut store = Store::new();
        let mut a = make_sym("save", "a.rs");
        a.container = Some("Store".into());
        let mut b = make_sym("save", "b.rs");
        b.container = Some("Cache".into());
        b.fqname = "b.rs::Cache::save".into();
        b.id = crate::model::SymbolId::new("rust", "function", &b.fqname, "b.rs");
        store.insert_symbol(a);
        store.insert_symbol(b);
        let r = resolve_symbols(&store, "Cache::save");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].file, "b.rs");
        let r = resolve_symbols(&store, "Cache.save");
        assert_eq!(r.len(), 1);
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
        assert_eq!(nbr.nodes.len(), 3);
        assert_eq!(nbr.edges.len(), 2);
    }

    #[test]
    fn expand_neighborhood_depth_2() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        let nbr = expand_neighborhood(&store, main_id, 2, 50);
        assert_eq!(nbr.nodes.len(), 4);
    }

    #[test]
    fn expand_neighborhood_respects_cap() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        let nbr = expand_neighborhood(&store, main_id, 10, 2);
        assert_eq!(nbr.nodes.len(), 2);
        assert!(nbr.edges.len() <= 1);
    }

    #[test]
    fn format_neighborhood_produces_output() {
        let store = build_test_store();
        let main_id = resolve_symbol(&store, "main").unwrap();
        let nbr = expand_neighborhood(&store, main_id, 1, 50);
        let text = format_neighborhood(&store, &nbr);
        assert!(text.contains("Neighborhood of main"));
        assert!(text.contains("Nodes:"));
        assert!(text.contains("main -calls-> process"));
    }
}
