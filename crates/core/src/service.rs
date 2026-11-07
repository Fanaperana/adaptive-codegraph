//! # Workspace service
//!
//! One place that owns an indexed workspace and answers questions about it
//! as compact text. The CLI and the MCP server are thin layers over this.
//!
//! - Symbols can be named by plain name (`save`), qualified name
//!   (`Store::save`, `Store.save`), file-qualified name, or 32-hex ID.
//! - The index is created on first use and kept fresh: before answering,
//!   changed files are detected by mtime/size and re-indexed incrementally
//!   (throttled, and a no-op when nothing changed).

use crate::config::Config;
use crate::embed::{self, Embedder, VectorIndex};
use crate::extract::plugin::PluginRegistry;
use crate::extract::ExtractorRegistry;
use crate::index::{self, IndexState};
use crate::insight;
use crate::model::{Symbol, SymbolId};
use crate::query::{self, display_name, location};
use crate::search::SearchIndex;
use crate::store::Store;
use crate::text::{TextHit, TextIndex, TextQuery};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Approximate characters per token, for budget arguments.
pub const CHARS_PER_TOKEN: usize = 4;

pub struct Workspace {
    pub base: PathBuf,
    pub config: Config,
    registry: ExtractorRegistry,
    store: Option<Store>,
    search: Option<SearchIndex>,
    vectors: Option<VectorIndex>,
    embedder: Option<Box<dyn Embedder>>,
    text: Option<TextIndex>,
    last_check: Option<Instant>,
    /// Minimum time between freshness checks (default: check before every
    /// answer, so a query right after an edit sees the edit).
    pub refresh_interval: Duration,
    /// Disable automatic refresh (e.g. for benchmarks).
    pub auto_refresh: bool,
}

/// A symbol as shown in structured (JSON) output.
#[derive(Clone, Debug, Serialize)]
pub struct SymbolRef {
    pub name: String,
    pub kind: String,
    pub file: String,
    /// 1-based line range.
    pub line: u32,
    pub end_line: u32,
    pub id: String,
}

impl SymbolRef {
    pub fn of(s: &Symbol) -> Self {
        Self {
            name: qualified(s),
            kind: s.kind.clone(),
            file: s.file.clone(),
            line: s.span.start_line + 1,
            end_line: s.span.end_line + 1,
            id: s.id.to_hex(),
        }
    }
}

/// A text match with the symbol that contains it.
#[derive(Clone, Debug, Serialize)]
pub struct GrepHit {
    #[serde(flatten)]
    pub hit: TextHit,
    pub symbol: Option<SymbolRef>,
}

#[derive(Debug, Serialize)]
pub struct GrepResult {
    pub matches: Vec<GrepHit>,
    /// Files the regex ran on (after the trigram filter) and files indexed.
    pub files_scanned: usize,
    pub files_indexed: usize,
}

/// One call site.
#[derive(Clone, Debug, Serialize)]
pub struct CallSite {
    pub caller: SymbolRef,
    /// 1-based line of the call.
    pub line: u32,
    pub count: u32,
    /// "certain", "inferred" or "guess" for linked calls; "ambiguous" or
    /// "unresolved" for possible callers.
    pub confidence: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CallersResult {
    pub target: SymbolRef,
    pub callers: Vec<CallSite>,
    /// Calls to the same name the resolver could not link to one target.
    pub possible: Vec<CallSite>,
}

/// How a symbol argument was resolved.
pub enum Lookup<'a> {
    Found(&'a Symbol),
    /// Several symbols match: the caller should pick one.
    Ambiguous(Vec<&'a Symbol>),
    NotFound,
}

impl Workspace {
    /// Open a workspace (does not build the index yet).
    pub fn open(base: &Path) -> anyhow::Result<Self> {
        let base = base.to_path_buf();
        let config = Config::load(&base)?;
        let registry = crate::lang::build_registry(&base)?;
        Ok(Self {
            base,
            config,
            registry,
            store: None,
            search: None,
            vectors: None,
            embedder: None,
            text: None,
            last_check: None,
            refresh_interval: Duration::ZERO,
            auto_refresh: true,
        })
    }

    pub fn index_dir(&self) -> PathBuf {
        self.base.join(&self.config.index_dir)
    }

    /// Load the saved index without building or refreshing anything.
    /// Returns whether an index is available.
    pub fn load_existing(&mut self) -> bool {
        self.load_saved();
        self.store.is_some()
    }

    /// Whether `base` looks like a project root (inside a git repository, or
    /// holding a common project manifest). Used to avoid indexing e.g. a home
    /// directory just because a session started there.
    pub fn is_project(&self) -> bool {
        const MARKERS: &[&str] = &[
            "Cargo.toml", "package.json", "go.mod", "pyproject.toml", "setup.py",
            "requirements.txt", "tsconfig.json", "Makefile", "CMakeLists.txt",
            ".adaptive-codegraph.toml",
        ];
        if MARKERS.iter().any(|m| self.base.join(m).exists()) {
            return true;
        }
        self.base.ancestors().any(|d| d.join(".git").exists())
    }

    /// Load the saved index if there is a usable one.
    fn load_saved(&mut self) {
        if self.store.is_some() {
            return;
        }
        if let Ok(s) = Store::load(&self.index_dir().join("graph.bin")) {
            self.store = Some(s);
            self.search = SearchIndex::open(&self.index_dir()).ok();
            self.vectors = VectorIndex::load(&self.index_dir().join("vectors.bin")).ok();
        }
    }

    /// Make sure an index exists and reflects the files on disk.
    /// Returns a short note when work was done.
    pub fn ensure_fresh(&mut self) -> anyhow::Result<Option<String>> {
        self.load_saved();
        if self.store.is_none() {
            // Another process (hook, MCP server, daemon) may be building it.
            let _lock = index::IndexLock::acquire(&self.index_dir())?;
            self.load_saved();
            if self.store.is_some() {
                self.last_check = Some(Instant::now());
                return Ok(None);
            }
            let t = Instant::now();
            let (store, search, vectors, _) =
                index::update_index(&self.config, &self.registry, &PluginRegistry::new(), None)?;
            let note = format!(
                "(built index: {} files, {} symbols, {} edges in {:.1}s)",
                store.file_count(),
                store.symbol_count(),
                store.edge_count(),
                t.elapsed().as_secs_f64()
            );
            self.store = Some(store);
            self.search = Some(search);
            self.vectors = Some(vectors);
            self.last_check = Some(Instant::now());
            return Ok(Some(note));
        }
        if !self.auto_refresh {
            return Ok(None);
        }
        if let Some(t) = self.last_check {
            if t.elapsed() < self.refresh_interval {
                return Ok(None);
            }
        }
        self.last_check = Some(Instant::now());
        if index::is_up_to_date(&self.config, &self.registry, self.store())? {
            return Ok(None);
        }
        self.refresh(false)
    }

    /// Re-index changed files (or everything with `full`).
    pub fn refresh(&mut self, full: bool) -> anyhow::Result<Option<String>> {
        let _lock = index::IndexLock::acquire(&self.index_dir())?;
        self.load_saved();
        let prev = if full { None } else { self.store.take() };
        // Drop the open search index before re-indexing writes to it.
        self.search = None;
        let (store, search, vectors, st) =
            index::update_index(&self.config, &self.registry, &PluginRegistry::new(), prev)?;
        self.store = Some(store);
        self.search = Some(search);
        self.vectors = Some(vectors);
        self.last_check = Some(Instant::now());
        if full || st.files_parsed > 0 || st.files_deleted > 0 {
            Ok(Some(format!(
                "(re-indexed {} changed and {} deleted files)",
                st.files_parsed, st.files_deleted
            )))
        } else {
            Ok(None)
        }
    }

    pub fn store(&self) -> &Store {
        self.store.as_ref().expect("call ensure_fresh first")
    }

    fn search_index(&self) -> anyhow::Result<&SearchIndex> {
        self.search.as_ref().ok_or_else(|| anyhow::anyhow!("search index not available"))
    }

    /// The embedding model (loaded once, only if vectors exist).
    fn embedder(&mut self) -> Option<&dyn Embedder> {
        let has_vectors = self.vectors.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
        if !has_vectors {
            return None;
        }
        if self.embedder.is_none() {
            self.embedder = Some(embed::create_embedder());
        }
        self.embedder.as_deref()
    }

    /// Resolve a symbol argument.
    pub fn lookup(&self, name: &str) -> Lookup<'_> {
        let store = self.store();
        let name = name.trim();
        if name.len() == 32 && name.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(id) = SymbolId::from_hex(name) {
                if let Some(s) = store.get(&id) {
                    return Lookup::Found(s);
                }
            }
        }
        let mut m = query::resolve_symbols(store, name);
        m.retain(|s| s.kind != "impl");
        if m.is_empty() {
            return Lookup::NotFound;
        }
        let exact: Vec<&Symbol> = m.iter().copied().filter(|s| s.name == name.rsplit([':', '.']).next().unwrap_or(name)).collect();
        let pool = if exact.is_empty() { m } else { exact };
        if pool.len() == 1 {
            return Lookup::Found(pool[0]);
        }
        // Prefer a unique non-test function/type when the name is plain.
        let primary: Vec<&Symbol> = pool
            .iter()
            .copied()
            .filter(|s| !insight::is_test(s) && !matches!(s.kind.as_str(), "field" | "variable" | "file" | "enum_variant"))
            .collect();
        if primary.len() == 1 {
            return Lookup::Found(primary[0]);
        }
        Lookup::Ambiguous(pool)
    }

    /// Resolve or explain why not, as text.
    fn resolve_or_message(&self, name: &str) -> Result<SymbolId, String> {
        match self.lookup(name) {
            Lookup::Found(s) => Ok(s.id),
            Lookup::NotFound => Err(format!(
                "No symbol named '{name}'. Try `search` with a description, or a qualified name like Type::method."
            )),
            Lookup::Ambiguous(v) => {
                let mut out = format!("'{name}' matches {} symbols; use a qualified name or ID:\n", v.len());
                for s in v.iter().take(15) {
                    let _ = writeln!(out, "  {} {} - {}  [{}]", s.kind, qualified(s), location(s), s.id);
                }
                Err(out)
            }
        }
    }

    // ── answers ────────────────────────────────────────────────────────

    pub fn search_text(&mut self, q: &str, limit: usize, kind: Option<&str>, path: Option<&str>) -> anyhow::Result<String> {
        let fetch = if kind.is_some() || path.is_some() { limit * 5 } else { limit };
        // Split the borrow: embedder needs &mut self.
        let _ = self.embedder();
        let store = self.store.as_ref().expect("fresh");
        let search = self.search_index()?;
        let hits = query::hybrid_search(store, search, self.vectors.as_ref(), self.embedder.as_deref(), q, fetch.max(limit))?;
        let mut out = String::new();
        let mut n = 0;
        for (s, _) in hits {
            if kind.map(|k| s.kind != k).unwrap_or(false) || path.map(|p| !s.file.starts_with(p)).unwrap_or(false) {
                continue;
            }
            let sig = s.signature.as_deref().unwrap_or("");
            let sig: String = sig.chars().take(110).collect();
            let _ = writeln!(out, "{} {} - {}  {}", s.kind, qualified(s), location(s), sig);
            n += 1;
            if n >= limit {
                break;
            }
        }
        if n == 0 {
            out.push_str("No results.\n");
        }
        Ok(out)
    }

    pub fn explain_text(&self, name: &str) -> String {
        match self.resolve_or_message(name) {
            Ok(id) => insight::explain(self.store(), id, 12),
            Err(m) => m,
        }
    }

    pub fn source_text(&self, name: &str, max_lines: usize) -> String {
        match self.resolve_or_message(name) {
            Ok(id) => insight::source(self.store(), &self.base, id, max_lines).unwrap_or_else(|e| format!("error: {e}")),
            Err(m) => m,
        }
    }

    /// Direct callers of a symbol, plus calls to the same name that could
    /// not be linked (possible callers). `Err` holds a message for the user.
    pub fn callers_data(&self, name: &str) -> Result<CallersResult, String> {
        let id = self.resolve_or_message(name)?;
        let store = self.store();
        let s = store.get(&id).unwrap();
        let mut callers: Vec<CallSite> = store
            .edges_in(&id)
            .into_iter()
            .filter(|(x, e)| matches!(e.kind.as_str(), "calls" | "renders") && x.kind != "file")
            .map(|(x, e)| CallSite {
                caller: SymbolRef::of(x),
                line: e.line + 1,
                count: e.count,
                confidence: match e.confidence {
                    3 => "certain",
                    2 => "inferred",
                    _ => "guess",
                },
            })
            .collect();
        callers.sort_by(|a, b| (&a.caller.file, a.line).cmp(&(&b.caller.file, b.line)));
        let linked: std::collections::HashSet<String> = callers.iter().map(|c| c.caller.id.clone()).collect();
        let mut possible: Vec<CallSite> = store
            .unlinked
            .iter()
            .filter(|u| u.name == s.name)
            .filter_map(|u| store.get(&u.from).map(|x| (u, x)))
            .filter(|(_, x)| x.kind != "file" && !linked.contains(&x.id.to_hex()))
            .map(|(u, x)| CallSite {
                caller: SymbolRef::of(x),
                line: u.line + 1,
                count: 1,
                confidence: if u.ambiguous { "ambiguous" } else { "unresolved" },
            })
            .collect();
        possible.sort_by(|a, b| (&a.caller.file, a.line).cmp(&(&b.caller.file, b.line)));
        possible.dedup_by(|a, b| a.caller.id == b.caller.id && a.line == b.line);
        Ok(CallersResult { target: SymbolRef::of(s), callers, possible })
    }

    pub fn callers_text(&self, name: &str, depth: usize) -> String {
        if depth <= 1 {
            return match self.callers_data(name) {
                Ok(r) => format_callers(&r),
                Err(m) => m,
            };
        }
        let id = match self.resolve_or_message(name) {
            Ok(id) => id,
            Err(m) => return m,
        };
        let store = self.store();
        let entries = insight::impact_with(store, id, depth, 200, false)
            .into_iter()
            .collect::<Vec<_>>();
        insight::format_impact(store, id, &entries, depth)
    }

    /// Literal or regex search over every text file (code, config, docs),
    /// each match tagged with the symbol that contains it. The text index is
    /// refreshed first, so results match the files on disk.
    pub fn grep(&mut self, q: &TextQuery) -> anyhow::Result<GrepResult> {
        let index_dir = self.index_dir();
        let text = self.text.get_or_insert_with(|| TextIndex::load(&index_dir));
        text.refresh(&self.config, &self.base, &index_dir)?;
        let r = text.search(q)?;
        let store = self.store.as_ref();
        let matches = r
            .hits
            .into_iter()
            .map(|hit| {
                let symbol = store.and_then(|s| enclosing(s, &hit.file, hit.line - 1)).map(SymbolRef::of);
                GrepHit { hit, symbol }
            })
            .collect();
        Ok(GrepResult { matches, files_scanned: r.candidates, files_indexed: r.files })
    }

    /// Read changed files into the text index now (normally done lazily).
    pub fn refresh_text(&mut self) -> anyhow::Result<usize> {
        let index_dir = self.index_dir();
        let text = self.text.get_or_insert_with(|| TextIndex::load(&index_dir));
        text.refresh(&self.config, &self.base, &index_dir)
    }

    pub fn callees_text(&self, name: &str) -> String {
        let id = match self.resolve_or_message(name) {
            Ok(id) => id,
            Err(m) => return m,
        };
        let store = self.store();
        let s = store.get(&id).unwrap();
        let v: Vec<_> = store
            .callees(&id)
            .into_iter()
            .chain(store.edges_out(&id).into_iter().filter(|(_, e)| e.kind == "renders"))
            .collect();
        if v.is_empty() {
            return format!("{} ({}) calls nothing in the project.\n", display_name(s), location(s));
        }
        let mut out = format!("{} calls ({}):\n", display_name(s), location(s));
        for (x, e) in v {
            let q = if e.confidence < 3 { "  (inferred)" } else { "" };
            let _ = writeln!(out, "  line {}: {} - {}{q}", e.line + 1, qualified(x), location(x));
        }
        out
    }

    pub fn impact_text(&self, name: &str, depth: usize) -> String {
        match self.resolve_or_message(name) {
            Ok(id) => {
                let entries = insight::impact(self.store(), id, depth, 300);
                insight::format_impact(self.store(), id, &entries, depth)
            }
            Err(m) => m,
        }
    }

    pub fn context_text(&mut self, q: &str, budget_tokens: usize) -> anyhow::Result<String> {
        let _ = self.embedder();
        let store = self.store.as_ref().expect("fresh");
        insight::context(
            store,
            &self.base,
            self.search_index()?,
            self.vectors.as_ref(),
            self.embedder.as_deref(),
            q,
            budget_tokens * CHARS_PER_TOKEN,
        )
    }

    pub fn repo_map_text(&self, budget_tokens: usize, path: Option<&str>) -> String {
        insight::repo_map(self.store(), budget_tokens * CHARS_PER_TOKEN, path)
    }

    pub fn outline_text(&self, file: &str) -> String {
        let f = file.trim().replace('\\', "/");
        let f = f.trim_start_matches("./");
        let store = self.store();
        if store.by_file.contains_key(f) {
            return insight::outline(store, f);
        }
        // Accept a path suffix.
        let matches: Vec<&String> = store.by_file.keys().filter(|k| k.ends_with(f)).collect();
        match matches.as_slice() {
            [one] => insight::outline(store, one),
            [] => format!("No indexed file matches '{file}'.\n"),
            many => {
                let mut out = format!("'{file}' matches several files:\n");
                for m in many.iter().take(20) {
                    let _ = writeln!(out, "  {m}");
                }
                out
            }
        }
    }

    pub fn path_text(&self, from: &str, to: &str) -> String {
        let (a, b) = match (self.resolve_or_message(from), self.resolve_or_message(to)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(m), _) | (_, Err(m)) => return m,
        };
        let store = self.store();
        match insight::call_path(store, a, b, 12) {
            Some(p) => {
                let mut out = String::from("Path:\n");
                for id in p {
                    if let Some(s) = store.get(&id) {
                        let _ = writeln!(out, "  {} - {}", qualified(s), location(s));
                    }
                }
                out
            }
            None => format!("No dependency path from {from} to {to} (within 12 hops).\n"),
        }
    }

    pub fn dead_code_text(&self, path: Option<&str>, limit: usize) -> String {
        let v = insight::dead_code(self.store(), path);
        let mut out = format!(
            "{} functions/methods with no callers or references in the project (entry points, exports and callbacks can look unused):\n",
            v.len()
        );
        for s in v.iter().take(limit) {
            let _ = writeln!(out, "  {} {} - {}", s.kind, qualified(s), location(s));
        }
        if v.len() > limit {
            let _ = writeln!(out, "  … +{} more", v.len() - limit);
        }
        out
    }

    pub fn neighborhood_text(&self, name: &str, depth: usize, cap: usize) -> String {
        match self.resolve_or_message(name) {
            Ok(id) => {
                let n = query::expand_neighborhood(self.store(), id, depth, cap);
                query::format_neighborhood(self.store(), &n)
            }
            Err(m) => m,
        }
    }

    pub fn status_text(&self) -> String {
        let mut out = String::new();
        let state = IndexState::load(&self.index_dir().join("state.json")).ok();
        let Some(store) = self.store.as_ref() else {
            return "No index yet.\n".into();
        };
        let _ = writeln!(out, "Workspace: {}", self.base.display().to_string().trim_start_matches(r"\\?\"));
        let _ = writeln!(out, "Files: {}  Symbols: {}  Edges: {}", store.file_count(), store.symbol_count(), store.edge_count());
        let mut langs: std::collections::BTreeMap<&str, usize> = Default::default();
        for f in store.facts.values() {
            *langs.entry(&f.lang).or_default() += 1;
        }
        let _ = writeln!(out, "Languages: {}", langs.iter().map(|(l, n)| format!("{l} ({n})")).collect::<Vec<_>>().join(", "));
        let st = &store.stats;
        let resolved: usize = st.resolved.iter().sum();
        let _ = writeln!(
            out,
            "References: {} - resolved {} (certain {}, inferred {}, guess {}), external {}, ambiguous {}, unresolved {}",
            st.refs, resolved, st.resolved[3], st.resolved[2], st.resolved[1], st.external, st.ambiguous, st.unresolved
        );
        let sem = self.vectors.as_ref().map(|v| v.len()).unwrap_or(0);
        let _ = writeln!(
            out,
            "Semantic vectors: {}",
            if sem > 0 { format!("{sem}") } else { "none (lexical search only)".into() }
        );
        if let Some(s) = state {
            if let Some(h) = s.git_head {
                let _ = writeln!(out, "Git HEAD at index: {h}");
            }
        }
        out
    }
}

/// The innermost non-file symbol in `file` whose lines contain `line` (0-based).
fn enclosing<'a>(store: &'a Store, file: &str, line: u32) -> Option<&'a Symbol> {
    store
        .by_file
        .get(file)?
        .iter()
        .filter_map(|id| store.get(id))
        .filter(|s| s.kind != "file" && s.span.start_line <= line && line <= s.span.end_line)
        .min_by_key(|s| s.span.end_line - s.span.start_line)
}

fn format_callers(r: &CallersResult) -> String {
    let t = &r.target;
    let mut out = String::new();
    if r.callers.is_empty() {
        let _ = writeln!(out, "Nothing in the project calls {} ({}:{}).", t.name, t.file, t.line);
    } else {
        let _ = writeln!(out, "{} callers of {} ({}:{}):", r.callers.len(), t.name, t.file, t.line);
        for c in &r.callers {
            let q = if c.confidence != "certain" { "  (inferred)" } else { "" };
            let n = if c.count > 1 { format!(" ×{}", c.count) } else { String::new() };
            let _ = writeln!(out, "  {} - {}:{}{n}{q}", c.caller.name, c.caller.file, c.line);
        }
    }
    if !r.possible.is_empty() {
        const SHOW: usize = 8;
        let _ = writeln!(
            out,
            "Possible callers ({}): calls to `{}` that could not be linked to one target:",
            r.possible.len(),
            t.name.rsplit("::").next().unwrap_or(&t.name)
        );
        for c in r.possible.iter().take(SHOW) {
            let _ = writeln!(out, "  {} - {}:{}  ({})", c.caller.name, c.caller.file, c.line, c.confidence);
        }
        if r.possible.len() > SHOW {
            let _ = writeln!(out, "  … +{} more", r.possible.len() - SHOW);
        }
    }
    out
}

/// Grep matches grouped by file, then by containing symbol. `limit` caps
/// the lines shown (0 = all).
pub fn format_grep(r: &GrepResult, limit: usize) -> String {
    let mut out = String::new();
    if r.matches.is_empty() {
        let _ = writeln!(out, "No matches ({} files searched).", r.files_indexed);
        return out;
    }
    let files: std::collections::BTreeSet<&str> = r.matches.iter().map(|m| m.hit.file.as_str()).collect();
    let _ = writeln!(
        out,
        "{} matching lines in {} files ({} of {} files scanned):",
        r.matches.len(),
        files.len(),
        r.files_scanned,
        r.files_indexed
    );
    let shown = if limit == 0 { r.matches.len() } else { limit.min(r.matches.len()) };
    let mut file = "";
    let mut sym: Option<&str> = None;
    for m in &r.matches[..shown] {
        if m.hit.file != file {
            file = &m.hit.file;
            sym = None;
            let _ = writeln!(out, "{file}");
        }
        let this = m.symbol.as_ref().map(|s| s.id.as_str()).unwrap_or("");
        if sym != Some(this) {
            sym = Some(this);
            match &m.symbol {
                Some(s) => {
                    let _ = writeln!(out, "  {} ({}, {}-{})", s.name, s.kind, s.line, s.end_line);
                }
                None => {
                    let _ = writeln!(out, "  (top level)");
                }
            }
        }
        let _ = writeln!(out, "    {}: {}", m.hit.line, m.hit.text.trim());
    }
    if shown < r.matches.len() {
        let _ = writeln!(out, "… +{} more lines (raise the limit or narrow with a path)", r.matches.len() - shown);
    }
    out
}

/// One `file:line:column:text` row per match, like ripgrep's `--vimgrep`.
pub fn format_grep_flat(r: &GrepResult) -> String {
    let mut out = String::new();
    for m in &r.matches {
        let _ = writeln!(out, "{}:{}:{}:{}", m.hit.file, m.hit.line, m.hit.column, m.hit.text);
    }
    out
}

/// `Owner::name` (or just `name`).
pub fn qualified(s: &Symbol) -> String {
    display_name(s)
}
