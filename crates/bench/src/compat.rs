//! Thin adapter over the core library so the benchmark logic stays the same
//! while the core API evolves.

use adaptive_codegraph_core::{
    config::Config,
    embed::{self, VectorIndex},
    extract::plugin::PluginRegistry,
    incremental, index, lang,
    model::SymbolId,
    query,
    search::SearchIndex,
    store::Store,
};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

pub struct Sym {
    pub id: SymbolId,
    pub name: String,
    pub kind: String,
    /// Workspace-relative, forward slashes.
    pub file: String,
    /// The file path exactly as stored in the graph.
    pub raw_file: String,
    pub start_line: u32,
    pub end_line: u32,
}

pub struct Edge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: String,
    pub conf: u8,
}

pub struct Graph {
    pub symbols: Vec<Sym>,
    pub edges: Vec<Edge>,
}

fn config(base: &Path, idx: &Path) -> Result<Config> {
    let mut c = Config::load(base)?;
    c.index_dir = idx.to_path_buf();
    Ok(c)
}

fn snapshot(base: &Path, store: &Store) -> Graph {
    let base_norm = crate::norm(&base.to_string_lossy());
    let symbols = store
        .graph
        .node_weights()
        .map(|s| {
            let raw = s.file.clone();
            let n = crate::norm(&raw);
            let file = n
                .strip_prefix(&format!("{base_norm}/"))
                .map(|x| x.to_string())
                .unwrap_or(n);
            Sym {
                id: s.id,
                name: s.name.clone(),
                kind: s.kind.clone(),
                file,
                raw_file: raw,
                start_line: s.span.start_line,
                end_line: s.span.end_line,
            }
        })
        .collect();
    let edges = store
        .graph
        .edge_indices()
        .filter_map(|e| {
            let (a, b) = store.graph.edge_endpoints(e)?;
            Some(Edge {
                from: store.graph[a].id,
                to: store.graph[b].id,
                kind: store.graph[e].kind.clone(),
                conf: store.graph[e].confidence,
            })
        })
        .collect();
    Graph { symbols, edges }
}

pub fn full_index(base: &Path, idx: &Path) -> Result<Graph> {
    let cfg = config(base, idx)?;
    let registry = lang::build_registry(base)?;
    let plugins = PluginRegistry::new();
    let (store, _, _) = index::full_index(&cfg, &registry, &plugins)?;
    Ok(snapshot(base, &store))
}

pub fn incremental_index(base: &Path, idx: &Path) -> Result<Graph> {
    let cfg = config(base, idx)?;
    let registry = lang::build_registry(base)?;
    let mut store = Store::load(&idx.join("graph.bin"))?;
    let search = SearchIndex::open(idx)?;
    let dim = embed::create_embedder().dim();
    let mut vectors =
        VectorIndex::load(&idx.join("vectors.bin")).unwrap_or_else(|_| VectorIndex::new(dim));
    incremental::incremental_reindex(&cfg, &registry, &mut store, &search, &mut vectors)?;
    Ok(snapshot(base, &store))
}

/// Symbols affected by changing `id` (transitive), as IDs.
pub fn impact_ids(idx: &Path, id: SymbolId, dispatch: bool) -> Result<Vec<SymbolId>> {
    let store = Store::load(&idx.join("graph.bin"))?;
    Ok(adaptive_codegraph_core::insight::impact_with(&store, id, 10, 10_000, dispatch)
        .into_iter()
        .map(|e| e.id)
        .collect())
}

/// Task context for a query, plus the size of the whole file holding the
/// expected symbol (what an agent would otherwise read).
pub fn context_text(ctx: &SearchCtx, base: &Path, q: &str, max_chars: usize) -> Result<String> {
    adaptive_codegraph_core::insight::context(
        &ctx.store,
        base,
        &ctx.search,
        ctx.vectors.as_ref(),
        Some(ctx.embedder.as_ref()),
        q,
        max_chars,
    )
}

/// Loaded search state for running many queries.
pub struct SearchCtx {
    store: Store,
    search: SearchIndex,
    vectors: Option<VectorIndex>,
    embedder: Box<dyn embed::Embedder>,
}

pub fn search_ctx(idx: &Path) -> Result<SearchCtx> {
    Ok(SearchCtx {
        store: Store::load(&idx.join("graph.bin"))?,
        search: SearchIndex::open(idx)?,
        vectors: VectorIndex::load(&idx.join("vectors.bin")).ok(),
        embedder: embed::create_embedder(),
    })
}

pub type Hits = Vec<(String, String)>;

/// Run a query with the legacy, lexical and hybrid search. Returns
/// (name, file) lists; hybrid is `None` without a semantic model.
pub fn search_all(ctx: &SearchCtx, q: &str, k: usize) -> Result<(Hits, Hits, Option<Hits>)> {
    let to = |v: Vec<(&adaptive_codegraph_core::model::Symbol, f32)>| -> Hits {
        v.into_iter().map(|(s, _)| (s.name.clone(), s.file.clone())).collect()
    };
    let legacy = ctx
        .search
        .search_legacy(q, k)?
        .into_iter()
        .map(|h| (h.name, h.file))
        .collect();
    let lexical = to(query::search(&ctx.store, &ctx.search, q, k)?);
    let hybrid = if ctx.embedder.is_semantic() && ctx.vectors.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
        Some(to(query::hybrid_search(
            &ctx.store,
            &ctx.search,
            ctx.vectors.as_ref(),
            Some(ctx.embedder.as_ref()),
            q,
            k,
        )?))
    } else {
        None
    };
    Ok((legacy, lexical, hybrid))
}

/// Average text size of a depth-1 neighborhood for the `n` most connected
/// symbols, plus average query time (load excluded).
pub fn neighborhood_cost(idx: &Path, n: usize) -> Result<(usize, f64)> {
    let store = Store::load(&idx.join("graph.bin"))?;
    let mut ids: Vec<(usize, SymbolId)> = store
        .by_id
        .iter()
        .map(|(id, &nx)| (store.graph.neighbors_undirected(nx).count(), *id))
        .collect();
    ids.sort_by(|a, b| b.0.cmp(&a.0).then(a.1 .0.cmp(&b.1 .0)));
    let ids: Vec<SymbolId> = ids.into_iter().take(n).map(|x| x.1).collect();
    if ids.is_empty() {
        return Ok((0, 0.0));
    }
    let mut bytes = 0;
    let t0 = Instant::now();
    for id in &ids {
        let nb = query::expand_neighborhood(&store, *id, 1, 50);
        bytes += query::format_neighborhood(&store, &nb).len();
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / ids.len() as f64;
    Ok((bytes / ids.len(), ms))
}
