//! # Insight
//!
//! Higher-level answers built on the graph, sized for an AI's context
//! window: what a change affects, what matters most in a repo, and the
//! smallest bundle of code relevant to a task.
//!
//! All text output uses workspace-relative paths and 1-based lines.

use crate::embed::{Embedder, VectorIndex};
use crate::model::{Symbol, SymbolId};
use crate::query::{display_name, location};
use crate::search::SearchIndex;
use crate::store::{EdgeData, Store};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;

/// Edge kinds through which a change propagates to the *source* of the edge.
const IMPACT_KINDS: &[&str] = &["calls", "references", "inherits", "implements", "overrides", "renders"];

/// Kinds worth showing in maps and context.
fn is_interesting(s: &Symbol) -> bool {
    matches!(
        s.kind.as_str(),
        "function" | "method" | "class" | "struct" | "interface" | "trait" | "enum" | "type"
            | "type_alias" | "macro" | "module" | "constant" | "static"
    )
}

/// Heuristic: does this symbol live in test code?
pub fn is_test(s: &Symbol) -> bool {
    let f = s.file.to_lowercase();
    let n = s.name.to_lowercase();
    f.contains("/test") || f.starts_with("test") || f.contains("_test.") || f.contains(".test.")
        || f.contains(".spec.") || f.contains("/spec/") || f.contains("__tests__")
        || n.starts_with("test_") || n.starts_with("test") && s.name.chars().nth(4).map(|c| c.is_uppercase()).unwrap_or(false)
        || s.container.as_deref().map(|c| c == "tests" || c.ends_with("::tests")).unwrap_or(false)
}

// ── Impact analysis ─────────────────────────────────────────────────────

/// One symbol affected by a change.
#[derive(Debug, Clone)]
pub struct ImpactEntry {
    pub id: SymbolId,
    pub depth: usize,
    /// How the change reaches it, e.g. `calls`, `inherits`, `calls (via Shape::area)`.
    pub via: String,
    /// The symbol it depends on (one step closer to the changed symbol).
    pub through: SymbolId,
}

/// Everything that (transitively) depends on `target`: callers, users of a
/// type, subclasses/implementors, and callers of the interface methods it
/// implements (they may dispatch to it).
pub fn impact(store: &Store, target: SymbolId, max_depth: usize, max_nodes: usize) -> Vec<ImpactEntry> {
    impact_with(store, target, max_depth, max_nodes, true)
}

/// [`impact`] with dynamic dispatch optional. Without it only static
/// dependencies count (no implementors, no callers of overridden methods).
pub fn impact_with(
    store: &Store,
    target: SymbolId,
    max_depth: usize,
    max_nodes: usize,
    dispatch: bool,
) -> Vec<ImpactEntry> {
    let mut seen: HashSet<SymbolId> = HashSet::from([target]);
    let mut out = Vec::new();
    let mut queue = VecDeque::from([(target, 0usize)]);

    while let Some((cur, d)) = queue.pop_front() {
        if d >= max_depth {
            continue;
        }
        let mut next: Vec<(SymbolId, String)> = store
            .edges_in(&cur)
            .into_iter()
            .filter(|(s, e)| IMPACT_KINDS.contains(&e.kind.as_str()) && s.kind != "file")
            .filter(|(_, e)| dispatch || e.kind != "overrides")
            .map(|(s, e)| (s.id, e.kind.clone()))
            .collect();
        // Dynamic dispatch: callers of the method this one overrides.
        for (sup, e) in store.edges_out(&cur).into_iter().filter(|_| dispatch) {
            if e.kind == "overrides" {
                for (caller, ce) in store.edges_in(&sup.id) {
                    if ce.kind == "calls" && caller.kind != "file" {
                        next.push((caller.id, format!("calls (via {})", display_name(sup))));
                    }
                }
            }
        }
        // Members of a changed type are affected through the type.
        if let Some(s) = store.get(&cur) {
            if s.is_type() && d == 0 {
                for m in store.children(&cur) {
                    for (caller, ce) in store.edges_in(&m.id) {
                        if ce.kind == "calls" && caller.kind != "file" && caller.parent != Some(cur) {
                            next.push((caller.id, format!("calls {}", display_name(m))));
                        }
                    }
                }
            }
        }
        for (id, via) in next {
            if seen.insert(id) {
                out.push(ImpactEntry {
                    id,
                    depth: d + 1,
                    via,
                    through: cur,
                });
                if out.len() >= max_nodes {
                    return out;
                }
                queue.push_back((id, d + 1));
            }
        }
    }
    out
}

/// Compact text report of [`impact`].
pub fn format_impact(store: &Store, target: SymbolId, entries: &[ImpactEntry], max_depth: usize) -> String {
    let mut out = String::new();
    let Some(t) = store.get(&target) else { return out };
    let files: HashSet<&str> = entries.iter().filter_map(|e| store.get(&e.id)).map(|s| s.file.as_str()).collect();
    let tests: Vec<&Symbol> = entries.iter().filter_map(|e| store.get(&e.id)).filter(|s| is_test(s)).collect();
    let _ = writeln!(
        out,
        "Changing {} ({}) affects {} symbols in {} files ({} tests), up to {} hops.",
        display_name(t),
        location(t),
        entries.len(),
        files.len(),
        tests.len(),
        max_depth
    );
    let mut depth = 0;
    for e in entries.iter().filter(|e| store.get(&e.id).map(|s| !is_test(s)).unwrap_or(false)) {
        let Some(s) = store.get(&e.id) else { continue };
        if e.depth != depth {
            depth = e.depth;
            let _ = writeln!(out, "Depth {depth}:");
        }
        let through = store.get(&e.through).map(display_name).unwrap_or_default();
        let _ = writeln!(out, "  {} {} - {}  [{} {}]", s.kind, display_name(s), location(s), e.via, through);
    }
    if !tests.is_empty() {
        let _ = writeln!(out, "Tests to run:");
        let mut by_file: Vec<(&str, Vec<&str>)> = Vec::new();
        for s in &tests {
            match by_file.iter_mut().find(|(f, _)| *f == s.file) {
                Some((_, v)) => v.push(&s.name),
                None => by_file.push((&s.file, vec![&s.name])),
            }
        }
        for (f, names) in by_file {
            let _ = writeln!(out, "  {f}: {}", names.join(", "));
        }
    }
    out
}

// ── Importance ──────────────────────────────────────────────────────────

fn edge_weight(e: &EdgeData) -> f64 {
    let w = match e.kind.as_str() {
        "calls" | "renders" => 1.0,
        "inherits" | "implements" => 0.8,
        "references" => 0.5,
        "overrides" => 0.3,
        _ => 0.0, // imports: file level, skip
    };
    w * (e.confidence as f64 / 3.0)
}

/// PageRank over the dependency graph: a symbol is important when
/// important symbols depend on it.
pub fn pagerank(store: &Store) -> HashMap<SymbolId, f64> {
    let ids: Vec<SymbolId> = store.by_id.keys().copied().collect();
    let n = ids.len().max(1);
    let index: HashMap<SymbolId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut out_edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (i, id) in ids.iter().enumerate() {
        for (t, e) in store.edges_out(id) {
            let w = edge_weight(e);
            if w > 0.0 {
                if let Some(&j) = index.get(&t.id) {
                    out_edges[i].push((j, w));
                }
            }
        }
    }
    let totals: Vec<f64> = out_edges.iter().map(|v| v.iter().map(|x| x.1).sum()).collect();
    let damping = 0.85;
    let mut rank = vec![1.0 / n as f64; n];
    for _ in 0..30 {
        let dangling: f64 = (0..n).filter(|&i| totals[i] == 0.0).map(|i| rank[i]).sum();
        let base = (1.0 - damping) / n as f64 + damping * dangling / n as f64;
        let mut next = vec![base; n];
        for i in 0..n {
            if totals[i] == 0.0 {
                continue;
            }
            let share = damping * rank[i] / totals[i];
            for &(j, w) in &out_edges[i] {
                next[j] += share * w;
            }
        }
        rank = next;
    }
    ids.into_iter().zip(rank).collect()
}

/// A ranked overview of the repository: the most important symbols grouped
/// by file, with one-line signatures, within `max_chars`.
pub fn repo_map(store: &Store, max_chars: usize, path_prefix: Option<&str>) -> String {
    let ranks = pagerank(store);
    let mut syms: Vec<(&Symbol, f64)> = store
        .graph
        .node_weights()
        .filter(|s| is_interesting(s) && !is_test(s))
        .filter(|s| path_prefix.map(|p| s.file.starts_with(p)).unwrap_or(true))
        .map(|s| (s, ranks.get(&s.id).copied().unwrap_or(0.0)))
        .collect();
    syms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Pick symbols in rank order, then print them grouped by file.
    let mut chosen: Vec<&Symbol> = Vec::new();
    let mut used = 0;
    let mut files_seen: HashSet<&str> = HashSet::new();
    for (s, _) in &syms {
        let line = map_line(s);
        let cost = line.len() + if files_seen.contains(s.file.as_str()) { 0 } else { s.file.len() + 2 };
        if used + cost > max_chars {
            if used > max_chars * 9 / 10 {
                break;
            }
            continue;
        }
        used += cost;
        files_seen.insert(&s.file);
        chosen.push(s);
    }
    let mut by_file: Vec<(&str, Vec<&Symbol>, f64)> = Vec::new();
    for s in chosen {
        let r = ranks.get(&s.id).copied().unwrap_or(0.0);
        match by_file.iter_mut().find(|(f, _, _)| *f == s.file) {
            Some((_, v, score)) => {
                v.push(s);
                *score += r;
            }
            None => by_file.push((&s.file, vec![s], r)),
        }
    }
    by_file.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = String::new();
    for (f, mut v, _) in by_file {
        v.sort_by_key(|s| s.span.start_byte);
        let _ = writeln!(out, "{f}:");
        for s in v {
            out.push_str(&map_line(s));
        }
    }
    out
}

fn map_line(s: &Symbol) -> String {
    let indent = if s.owner().is_some() { "    " } else { "  " };
    let sig = s.signature.as_deref().unwrap_or(&s.name);
    let sig: String = sig.chars().take(140).collect();
    format!("{indent}{}: {sig}\n", s.line())
}

// ── Single-symbol views ─────────────────────────────────────────────────

/// Source of a symbol with 1-based line numbers. Long bodies keep the head
/// and tail and elide the middle.
pub fn source(store: &Store, base: &Path, id: SymbolId, max_lines: usize) -> anyhow::Result<String> {
    let s = store.get(&id).ok_or_else(|| anyhow::anyhow!("unknown symbol"))?;
    let text = std::fs::read_to_string(base.join(&s.file))?;
    let lines: Vec<&str> = text.lines().collect();
    let start = s.span.start_line as usize;
    let end = (s.span.end_line as usize).min(lines.len().saturating_sub(1));
    let mut out = format!("// {} {} - {}\n", s.kind, display_name(s), location(s));
    let total = end + 1 - start.min(end + 1);
    let width = (end + 1).to_string().len();
    let emit = |out: &mut String, i: usize| {
        let _ = writeln!(out, "{:>width$} | {}", i + 1, lines.get(i).unwrap_or(&""));
    };
    if total <= max_lines {
        for i in start..=end {
            emit(&mut out, i);
        }
    } else {
        let head = max_lines * 2 / 3;
        let tail = max_lines - head;
        for i in start..start + head {
            emit(&mut out, i);
        }
        let _ = writeln!(out, "{:>width$} | … {} lines omitted …", "", total - head - tail);
        for i in end + 1 - tail..=end {
            emit(&mut out, i);
        }
    }
    Ok(out)
}

/// The structure of a file: every symbol, nested, with line ranges.
pub fn outline(store: &Store, file: &str) -> String {
    let mut out = String::new();
    let Some(ids) = store.by_file.get(file) else {
        return format!("No symbols indexed for {file}.\n");
    };
    let mut syms: Vec<&Symbol> = ids.iter().filter_map(|i| store.get(i)).filter(|s| s.kind != "file").collect();
    syms.sort_by_key(|s| s.span.start_byte);
    let file_id = ids.iter().filter_map(|i| store.get(i)).find(|s| s.kind == "file").map(|s| s.id);
    let depth_of = |s: &Symbol| {
        let mut d = 0;
        let mut p = s.parent;
        while let Some(pid) = p {
            if Some(pid) == file_id {
                break;
            }
            d += 1;
            p = store.get(&pid).and_then(|x| x.parent);
        }
        d
    };
    let _ = writeln!(out, "{file}:");
    for s in syms {
        let sig = s.signature.as_deref().unwrap_or(&s.name);
        let sig: String = sig.chars().take(120).collect();
        let _ = writeln!(
            out,
            "{}{} {} [{}-{}]",
            "  ".repeat(depth_of(s) + 1),
            s.kind,
            sig,
            s.line(),
            s.span.end_line + 1
        );
    }
    out
}

/// One compact card with everything known about a symbol.
pub fn explain(store: &Store, id: SymbolId, max_items: usize) -> String {
    let mut out = String::new();
    let Some(s) = store.get(&id) else { return out };
    let _ = writeln!(out, "{} {} - {}-{}", s.kind, display_name(s), location(s), s.span.end_line + 1);
    if let Some(sig) = &s.signature {
        let _ = writeln!(out, "  signature: {sig}");
    }
    if let Some(doc) = &s.doc {
        let d: String = doc.lines().take(6).collect::<Vec<_>>().join("\n    ");
        let _ = writeln!(out, "  doc: {d}");
    }
    if let Some(p) = s.parent.and_then(|p| store.get(&p)).filter(|p| p.kind != "file") {
        let _ = writeln!(out, "  inside: {} {}", p.kind, display_name(p));
    }

    let list = |out: &mut String, label: &str, mut items: Vec<(&Symbol, &EdgeData)>| {
        if items.is_empty() {
            return;
        }
        items.sort_by(|a, b| b.1.confidence.cmp(&a.1.confidence).then(a.0.fqname.cmp(&b.0.fqname)));
        let n = items.len();
        let shown: Vec<String> = items
            .iter()
            .take(max_items)
            .map(|(x, e)| {
                let q = if e.confidence < 3 { "?" } else { "" };
                format!("{}{q} ({})", display_name(x), location(x))
            })
            .collect();
        let more = if n > max_items { format!(" … +{} more", n - max_items) } else { String::new() };
        let _ = writeln!(out, "  {label} ({n}): {}{more}", shown.join(", "));
    };
    let ins = store.edges_in(&id);
    let outs = store.edges_out(&id);
    list(&mut out, "inherits from", [by(&outs, "inherits"), by(&outs, "implements")].concat());
    list(&mut out, "overrides", by(&outs, "overrides"));
    list(&mut out, "subtypes", [by(&ins, "inherits"), by(&ins, "implements")].concat());
    list(&mut out, "overridden by", by(&ins, "overrides"));
    let members = store.children(&id);
    if !members.is_empty() && s.kind != "file" {
        let names: Vec<String> = members.iter().take(max_items * 2).map(|m| format!("{} {}", m.kind, m.name)).collect();
        let more = if members.len() > max_items * 2 { " …".to_string() } else { String::new() };
        let _ = writeln!(out, "  members ({}): {}{more}", members.len(), names.join(", "));
    }
    list(&mut out, "called by", by(&ins, "calls"));
    list(&mut out, "calls", by(&outs, "calls"));
    list(&mut out, "used by", [by(&ins, "references"), by(&ins, "renders")].concat());
    list(&mut out, "uses types", by(&outs, "references"));
    out
}

/// Edges of one kind, excluding file nodes.
fn by<'a>(v: &[(&'a Symbol, &'a EdgeData)], k: &str) -> Vec<(&'a Symbol, &'a EdgeData)> {
    v.iter().filter(|(x, e)| e.kind == k && x.kind != "file").copied().collect()
}

/// Shortest dependency path from `from` to `to` (following calls,
/// references, inherits/implements), as display names.
pub fn call_path(store: &Store, from: SymbolId, to: SymbolId, max_depth: usize) -> Option<Vec<SymbolId>> {
    let mut prev: HashMap<SymbolId, SymbolId> = HashMap::new();
    let mut queue = VecDeque::from([(from, 0usize)]);
    let mut seen = HashSet::from([from]);
    while let Some((cur, d)) = queue.pop_front() {
        if cur == to {
            let mut path = vec![to];
            let mut c = to;
            while let Some(p) = prev.get(&c) {
                path.push(*p);
                c = *p;
            }
            path.reverse();
            return Some(path);
        }
        if d >= max_depth {
            continue;
        }
        for (t, e) in store.edges_out(&cur) {
            if IMPACT_KINDS.contains(&e.kind.as_str()) && seen.insert(t.id) {
                prev.insert(t.id, cur);
                queue.push_back((t.id, d + 1));
            }
        }
    }
    None
}

/// Functions and methods nothing in the project uses (candidates only:
/// entry points, exported APIs and framework callbacks look unused too).
pub fn dead_code<'a>(store: &'a Store, path_prefix: Option<&str>) -> Vec<&'a Symbol> {
    let mut v: Vec<&Symbol> = store
        .graph
        .node_weights()
        .filter(|s| matches!(s.kind.as_str(), "function" | "method"))
        .filter(|s| path_prefix.map(|p| s.file.starts_with(p)).unwrap_or(true))
        .filter(|s| !is_test(s) && !matches!(s.name.as_str(), "main" | "new" | "default" | "init" | "__init__" | "constructor"))
        .filter(|s| {
            store
                .edges_in(&s.id)
                .iter()
                .all(|(x, e)| !(IMPACT_KINDS.contains(&e.kind.as_str()) || e.kind == "imports") || x.id == s.id)
        })
        .filter(|s| store.edges_out(&s.id).iter().all(|(_, e)| e.kind != "overrides"))
        .collect();
    v.sort_by(|a, b| (&a.file, a.span.start_line).cmp(&(&b.file, b.span.start_line)));
    v
}

// ── Task context ────────────────────────────────────────────────────────

/// Resolve a query to seed symbols: an exact (qualified) name if it is one,
/// otherwise the best search hits.
pub fn seeds<'a>(
    store: &'a Store,
    search: &SearchIndex,
    vectors: Option<&VectorIndex>,
    embedder: Option<&dyn Embedder>,
    query: &str,
    n: usize,
) -> anyhow::Result<Vec<&'a Symbol>> {
    let looks_like_name = !query.trim().contains(' ');
    if looks_like_name {
        let exact: Vec<&Symbol> = crate::query::resolve_symbols(store, query)
            .into_iter()
            .filter(|s| s.kind != "file")
            .take(n)
            .collect();
        if !exact.is_empty() {
            return Ok(exact);
        }
    }
    Ok(crate::query::hybrid_search(store, search, vectors, embedder, query, n)?
        .into_iter()
        .map(|(s, _)| s)
        .collect())
}

/// Pack the code most relevant to `query` into about `max_chars`:
/// full source of the best-matching symbols, then signatures of what they
/// call and what calls them.
#[allow(clippy::too_many_arguments)]
pub fn context(
    store: &Store,
    base: &Path,
    search: &SearchIndex,
    vectors: Option<&VectorIndex>,
    embedder: Option<&dyn Embedder>,
    query: &str,
    max_chars: usize,
) -> anyhow::Result<String> {
    let seeds = seeds(store, search, vectors, embedder, query, 5)?;
    let mut out = String::new();
    if seeds.is_empty() {
        return Ok(format!("No code found for: {query}\n"));
    }
    let ranks = pagerank_cached(store);
    let mut included: HashSet<SymbolId> = HashSet::new();

    // 1. Source of the seeds, best first; shorter cut for later seeds.
    let mut src_budget = max_chars * 7 / 10;
    for (i, s) in seeds.iter().enumerate() {
        let max_lines = if i == 0 { 80 } else { 40 };
        let text = source(store, base, s.id, max_lines).unwrap_or_default();
        if text.len() > src_budget && i > 0 {
            continue;
        }
        let text: String = text.chars().take(src_budget).collect();
        src_budget = src_budget.saturating_sub(text.len());
        out.push_str(&text);
        out.push('\n');
        included.insert(s.id);
    }

    // 2. Related symbols as one-line signatures, most important first.
    let mut related: Vec<(f64, String, SymbolId)> = Vec::new();
    for s in &seeds {
        for (t, e) in store.edges_out(&s.id) {
            if t.kind != "file" && e.kind != "imports" {
                let r = ranks.get(&t.id).copied().unwrap_or(0.0) + e.confidence as f64;
                related.push((r, format!("{} → {} {}", display_name(s), e.kind, display_name(t)), t.id));
            }
        }
        for (t, e) in store.edges_in(&s.id) {
            if t.kind != "file" && e.kind != "imports" {
                let r = ranks.get(&t.id).copied().unwrap_or(0.0) + e.confidence as f64 - 0.5;
                related.push((r, format!("{} ← {} by {}", display_name(s), e.kind, display_name(t)), t.id));
            }
        }
    }
    related.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut rel_text = String::new();
    for (_, label, id) in related {
        if included.contains(&id) || out.len() + rel_text.len() > max_chars {
            continue;
        }
        let Some(t) = store.get(&id) else { continue };
        let sig = t.signature.as_deref().unwrap_or(&t.name);
        let sig: String = sig.chars().take(120).collect();
        let line = format!("  {label} - {} - {sig}\n", location(t));
        if out.len() + rel_text.len() + line.len() > max_chars {
            continue;
        }
        rel_text.push_str(&line);
        included.insert(id);
    }
    if !rel_text.is_empty() {
        out.push_str("Related:\n");
        out.push_str(&rel_text);
    }
    Ok(out)
}

/// PageRank is O(edges × 30); cache it per store generation.
fn pagerank_cached(store: &Store) -> HashMap<SymbolId, f64> {
    // Stores are rebuilt rather than mutated in normal use, so a cache keyed
    // by size is good enough to avoid recomputing within one session.
    use std::sync::Mutex;
    static CACHE: Mutex<Option<((usize, usize), HashMap<SymbolId, f64>)>> = Mutex::new(None);
    let key = (store.symbol_count(), store.edge_count());
    if let Ok(guard) = CACHE.lock() {
        if let Some((k, v)) = guard.as_ref() {
            if *k == key {
                return v.clone();
            }
        }
    }
    let r = pagerank(store);
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((key, r.clone()));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, Span};

    fn sym(name: &str, file: &str, line: u32) -> Symbol {
        Symbol::new(
            "rust",
            "function",
            name,
            format!("{file}::{name}"),
            file,
            Span {
                start_byte: line as usize * 10,
                end_byte: line as usize * 10 + 5,
                start_line: line,
                end_line: line,
            },
        )
    }

    fn chain() -> (Store, Vec<SymbolId>) {
        // a -> b -> c, test_x -> a
        let mut st = Store::new();
        let v = vec![sym("a", "src/a.rs", 1), sym("b", "src/b.rs", 1), sym("c", "src/c.rs", 1), sym("test_x", "tests/t.rs", 1)];
        let ids: Vec<SymbolId> = v.iter().map(|s| s.id).collect();
        for s in v {
            st.insert_symbol(s);
        }
        st.insert_edge(Edge::new(ids[0], ids[1], "calls"));
        st.insert_edge(Edge::new(ids[1], ids[2], "calls"));
        st.insert_edge(Edge::new(ids[3], ids[0], "calls"));
        (st, ids)
    }

    #[test]
    fn impact_is_transitive_and_flags_tests() {
        let (st, ids) = chain();
        let imp = impact(&st, ids[2], 5, 100);
        let got: Vec<SymbolId> = imp.iter().map(|e| e.id).collect();
        assert_eq!(got, vec![ids[1], ids[0], ids[3]]);
        assert_eq!(imp[2].depth, 3);
        let text = format_impact(&st, ids[2], &imp, 5);
        assert!(text.contains("affects 3 symbols"));
        assert!(text.contains("Tests to run"));
    }

    #[test]
    fn pagerank_prefers_depended_on() {
        let (st, ids) = chain();
        let r = pagerank(&st);
        assert!(r[&ids[2]] > r[&ids[3]]);
    }

    #[test]
    fn path_and_dead_code() {
        let (st, ids) = chain();
        assert_eq!(call_path(&st, ids[3], ids[2], 5), Some(vec![ids[3], ids[0], ids[1], ids[2]]));
        assert!(call_path(&st, ids[2], ids[0], 5).is_none());
        let dead: Vec<&str> = dead_code(&st, None).iter().map(|s| s.name.as_str()).collect();
        assert!(dead.is_empty(), "a is called by a test, b and c are called: {dead:?}");
    }

    #[test]
    fn repo_map_respects_budget() {
        let (st, _) = chain();
        let m = repo_map(&st, 40, None);
        assert!(m.len() <= 60, "{m}");
        assert!(m.contains("src/c.rs"), "most depended-on file first: {m}");
    }
}
