//! # Generic Tree-sitter Extractor
//!
//! Extracts symbols and raw facts from any language that has a tree-sitter
//! grammar plus two query files. Capture names understood:
//!
//! **Symbol query** (`<lang>.scm`)
//! - `@symbol.name` — name node of a definition (required)
//! - `@symbol.def` — the whole definition node (span, signature, docs)
//! - `@symbol.type` — declared type / return type
//! - `@symbol.container` — owning type when it is not a syntactic parent
//!   (e.g. a Go method receiver)
//! - `(#set! kind "...")` — override the inferred kind
//!
//! **Edge query** (`<lang>_edges.scm`)
//! - `@call.name` — called name; optional `@call.qualifier` (`Type::f`,
//!   `module::f`) and `@call.receiver` (`obj.f()`);
//!   `(#set! target "macro")` restricts targets, `(#set! edge "...")` sets the
//!   edge kind (default `calls`)
//! - `@import.path` with `@import.name` / `@import.alias` / `@import.default`,
//!   or `@import.module` (+ optional `@import.alias`) for whole-module imports
//! - `@binding.name` with `@binding.type`, `@binding.call` or
//!   `@binding.iter` (loop variable over a collection) — local variables,
//!   parameters, and `self.x` / `this.x` attributes whose type can be inferred

use crate::extract::{Extractor, LangProfile};
use crate::model::{Binding, ExtractionResult, Import, RawRef, Span, Symbol, SymbolId};
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;

/// Configuration for a tree-sitter based extractor.
pub struct TreeSitterConfig {
    /// Language identifier.
    pub lang_id: String,
    /// File extensions.
    pub extensions: Vec<String>,
    /// The tree-sitter language grammar.
    pub ts_language: tree_sitter::Language,
    /// Query source for symbol extraction.
    pub symbol_query: String,
    /// Query source for edge extraction (calls, imports, bindings).
    pub edge_query: String,
    /// Cross-file scoping rules.
    pub profile: LangProfile,
}

struct SymCaps {
    name: Option<u32>,
    def: Option<u32>,
    type_: Option<u32>,
    container: Option<u32>,
}

struct EdgeCaps {
    call_name: Option<u32>,
    call_qualifier: Option<u32>,
    call_receiver: Option<u32>,
    import_path: Option<u32>,
    import_name: Option<u32>,
    import_alias: Option<u32>,
    import_module: Option<u32>,
    import_default: Option<u32>,
    import_glob: Option<u32>,
    binding_name: Option<u32>,
    binding_type: Option<u32>,
    binding_call: Option<u32>,
    binding_iter: Option<u32>,
}

/// Compiled queries and their capture indexes.
struct Compiled {
    symbol_query: tree_sitter::Query,
    edge_query: tree_sitter::Query,
    sym_caps: SymCaps,
    edge_caps: EdgeCaps,
}

/// A generic extractor powered by tree-sitter + query files.
///
/// Queries are compiled on first use: compiling every language's queries
/// takes far longer than answering a query on an index that is up to date.
pub struct TreeSitterExtractor {
    config: TreeSitterConfig,
    compiled: std::sync::OnceLock<Result<Compiled, String>>,
}

impl TreeSitterExtractor {
    pub fn new(config: TreeSitterConfig) -> anyhow::Result<Self> {
        Ok(Self { config, compiled: std::sync::OnceLock::new() })
    }

    /// Compile now and report query errors (normally deferred to first use).
    pub fn validate(&self) -> anyhow::Result<()> {
        self.compiled().map(|_| ())
    }

    fn compiled(&self) -> anyhow::Result<&Compiled> {
        self.compiled
            .get_or_init(|| compile(&self.config).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Compiled queries; `extract` has already checked they compile.
    fn cq(&self) -> &Compiled {
        self.compiled().expect("queries compiled before use")
    }
}

fn compile(config: &TreeSitterConfig) -> anyhow::Result<Compiled> {
    {
        let symbol_query = tree_sitter::Query::new(&config.ts_language, &config.symbol_query)
            .map_err(|e| anyhow::anyhow!("symbol query error for {}: {e}", config.lang_id))?;
        let edge_query = tree_sitter::Query::new(&config.ts_language, &config.edge_query)
            .map_err(|e| anyhow::anyhow!("edge query error for {}: {e}", config.lang_id))?;

        let s = |n: &str| symbol_query.capture_index_for_name(n);
        let sym_caps = SymCaps {
            name: s("symbol.name"),
            def: s("symbol.def"),
            type_: s("symbol.type"),
            container: s("symbol.container"),
        };
        let e = |n: &str| edge_query.capture_index_for_name(n);
        let edge_caps = EdgeCaps {
            call_name: e("call.name"),
            call_qualifier: e("call.qualifier"),
            call_receiver: e("call.receiver"),
            import_path: e("import.path"),
            import_name: e("import.name"),
            import_alias: e("import.alias"),
            import_module: e("import.module"),
            import_default: e("import.default"),
            import_glob: e("import.glob"),
            binding_name: e("binding.name"),
            binding_type: e("binding.type"),
            binding_call: e("binding.call"),
            binding_iter: e("binding.iter"),
        };

        Ok(Compiled {
            symbol_query,
            edge_query,
            sym_caps,
            edge_caps,
        })
    }
}

impl TreeSitterExtractor {

    /// Run the symbol query and build Symbol structs (without the file symbol).
    fn extract_symbols(&self, file: &str, tree: &tree_sitter::Tree, src: &[u8]) -> Vec<Symbol> {
        struct Cand<'t> {
            name: String,
            name_node: tree_sitter::Node<'t>,
            def: tree_sitter::Node<'t>,
            explicit_kind: Option<String>,
            type_hint: Option<String>,
            container: Option<String>,
        }

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&self.cq().symbol_query, tree.root_node(), src);
        // Keyed by name-node start byte: several patterns can match one symbol.
        let mut cands: HashMap<usize, Cand> = HashMap::new();

        while let Some(m) = matches.next() {
            let get = |idx: Option<u32>| {
                idx.and_then(|i| m.captures.iter().find(|c| c.index == i).map(|c| c.node))
            };
            let Some(name_node) = get(self.cq().sym_caps.name) else { continue };
            let name = text(name_node, src);
            if name.is_empty() {
                continue;
            }
            let def = get(self.cq().sym_caps.def).unwrap_or(name_node);
            let explicit_kind = property(&self.cq().symbol_query, m.pattern_index, "kind");
            let type_hint = get(self.cq().sym_caps.type_)
                .and_then(|n| clean_type(&text(n, src)))
                .or_else(|| arrow_return_type(def, src));
            let container = get(self.cq().sym_caps.container).map(|n| text(n, src));

            let cand = Cand {
                name,
                name_node,
                def,
                explicit_kind,
                type_hint,
                container,
            };
            match cands.get_mut(&name_node.start_byte()) {
                None => {
                    cands.insert(name_node.start_byte(), cand);
                }
                Some(old) => {
                    let better = (cand.explicit_kind.is_some() && old.explicit_kind.is_none())
                        || (cand.explicit_kind.is_some() == old.explicit_kind.is_some()
                            && cand.def.byte_range().len() > old.def.byte_range().len());
                    let keep_type = old.type_hint.clone();
                    let keep_container = old.container.clone();
                    if better {
                        *old = cand;
                    }
                    if old.type_hint.is_none() {
                        old.type_hint = keep_type;
                    }
                    if old.container.is_none() {
                        old.container = keep_container;
                    }
                }
            }
        }

        let mut cands: Vec<Cand> = cands.into_values().collect();
        cands.sort_by_key(|c| (c.def.start_byte(), std::cmp::Reverse(c.def.end_byte())));

        let mut seen: HashMap<SymbolId, usize> = HashMap::new();
        let mut symbols = Vec::with_capacity(cands.len());
        for c in cands {
            let (mut chain, scope_cat) = scope_chain(c.def, src);
            let mut kind = c.explicit_kind.clone().unwrap_or_else(|| kind_for(c.def, src));
            if let Some(owner) = &c.container {
                if let Some(owner) = normalize_type(owner) {
                    chain.push(owner);
                }
                if kind == "function" {
                    kind = "method".into();
                }
            } else if scope_cat == Some(ScopeCat::Type) {
                if kind == "function" {
                    kind = "method".into();
                } else if kind == "variable" {
                    kind = "field".into();
                }
            }

            let name = if kind == "impl" {
                normalize_type(&c.name).unwrap_or(c.name.clone())
            } else {
                c.name.clone()
            };
            let container = if chain.is_empty() {
                None
            } else {
                Some(chain.join("::"))
            };
            let mut fqname = match &container {
                Some(cn) => format!("{file}::{cn}::{name}"),
                None => format!("{file}::{name}"),
            };
            // Disambiguate same-named symbols in one scope (overloads, several
            // `impl` blocks for one type, ...).
            let mut id = SymbolId::new(&self.config.lang_id, &kind, &fqname, file);
            let n = seen.entry(id).or_insert(0);
            *n += 1;
            if *n > 1 {
                fqname = format!("{fqname}#{n}");
                id = SymbolId::new(&self.config.lang_id, &kind, &fqname, file);
            }

            let span = Span {
                start_byte: c.def.start_byte(),
                end_byte: c.def.end_byte(),
                start_line: c.def.start_position().row as u32,
                end_line: c.def.end_position().row as u32,
            };
            let mut sym = Symbol::new(&self.config.lang_id, &kind, &name, &fqname, file, span);
            sym.id = id;
            sym.signature = Some(extract_signature(c.def, src, &self.config.lang_id));
            sym.doc = extract_doc(c.def, src);
            sym.container = container;
            sym.type_hint = c.type_hint;
            let _ = c.name_node;
            symbols.push(sym);
        }
        symbols
    }

    /// Run the edge query: collect raw references, imports and bindings.
    fn extract_facts(
        &self,
        tree: &tree_sitter::Tree,
        src: &[u8],
        index: &SpanIndex,
        result: &mut ExtractionResult,
    ) {
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&self.cq().edge_query, tree.root_node(), src);
        let c = &self.cq().edge_caps;
        // One reference per name node: when several patterns match the same
        // call, keep the most specific (with a qualifier or receiver).
        let mut ref_at: HashMap<usize, usize> = HashMap::new();

        while let Some(m) = matches.next() {
            let all = |idx: Option<u32>| -> Vec<tree_sitter::Node> {
                match idx {
                    Some(i) => m.captures.iter().filter(|c| c.index == i).map(|c| c.node).collect(),
                    None => Vec::new(),
                }
            };
            let one = |idx: Option<u32>| all(idx).into_iter().next();

            // Calls / references
            let names = all(c.call_name);
            if !names.is_empty() {
                let qualifier = one(c.call_qualifier).map(|n| text(n, src));
                let receiver = one(c.call_receiver).map(|n| text(n, src));
                let target_kind = property(&self.cq().edge_query, m.pattern_index, "target");
                let kind = property(&self.cq().edge_query, m.pattern_index, "edge")
                    .unwrap_or_else(|| "calls".to_string());
                for n in names {
                    let name = text(n, src);
                    if name.is_empty() {
                        continue;
                    }
                    let r = RawRef {
                        from: index.innermost(&result.symbols, n.start_byte()),
                        name,
                        kind: kind.clone(),
                        qualifier: qualifier.clone(),
                        receiver: receiver.clone(),
                        target_kind: target_kind.clone(),
                        line: n.start_position().row as u32,
                    };
                    match ref_at.get(&n.start_byte()) {
                        Some(&i) => {
                            let old = &result.refs[i];
                            if old.qualifier.is_none() && old.receiver.is_none() {
                                result.refs[i] = r;
                            }
                        }
                        None => {
                            ref_at.insert(n.start_byte(), result.refs.len());
                            result.refs.push(r);
                        }
                    }
                }
                continue;
            }

            // Imports
            let path = one(c.import_path).map(|n| strip_quotes(&text(n, src)));
            let alias = one(c.import_alias).map(|n| text(n, src));
            let items = all(c.import_name);
            let default = one(c.import_default);
            let module = one(c.import_module);
            if let Some(g) = one(c.import_glob) {
                // `use a::*`, `from a import *`, `export * from "./a"`
                result.imports.push(Import {
                    local: "*".into(),
                    path: strip_quotes(&text(g, src)),
                    item: None,
                    line: g.start_position().row as u32,
                });
                continue;
            }
            if path.is_some() || module.is_some() {
                let line = m.captures[0].node.start_position().row as u32;
                if let Some(module) = module {
                    let p = strip_quotes(&text(module, src));
                    let local = alias.clone().unwrap_or_else(|| last_segment(&p).to_string());
                    result.imports.push(Import {
                        local,
                        path: p,
                        item: None,
                        line,
                    });
                } else if let Some(path) = path {
                    if let Some(d) = default {
                        let local = text(d, src);
                        result.imports.push(Import {
                            local: local.clone(),
                            path: path.clone(),
                            item: Some(local),
                            line,
                        });
                    }
                    if items.is_empty() && default.is_none() {
                        // Whole-module import, bound to the alias or last segment.
                        let local = alias.clone().unwrap_or_else(|| last_segment(&path).to_string());
                        result.imports.push(Import {
                            local,
                            path: path.clone(),
                            item: None,
                            line,
                        });
                    }
                    for it in &items {
                        let item = text(*it, src);
                        let local = if items.len() == 1 {
                            alias.clone().unwrap_or_else(|| item.clone())
                        } else {
                            item.clone()
                        };
                        result.imports.push(Import {
                            local,
                            path: path.clone(),
                            item: Some(item),
                            line,
                        });
                    }
                }
                continue;
            }

            // Bindings
            if let Some(bn) = one(c.binding_name) {
                let type_name = one(c.binding_type).and_then(|n| clean_type(&text(n, src)));
                let call = one(c.binding_call).map(|n| text(n, src));
                let iter = one(c.binding_iter).map(|n| text(n, src));
                if type_name.is_none() && call.is_none() && iter.is_none() {
                    continue;
                }
                result.bindings.push(Binding {
                    scope: index.innermost(&result.symbols, bn.start_byte()),
                    name: text(bn, src),
                    type_name,
                    call,
                    iter,
                });
            }
        }
    }
}

impl Extractor for TreeSitterExtractor {
    fn language(&self) -> &str {
        &self.config.lang_id
    }

    fn extensions(&self) -> &[&str] {
        // Extension matching is done by ExtractorRegistry.
        &[]
    }

    fn profile(&self) -> LangProfile {
        self.config.profile.clone()
    }

    fn extract(&self, path: &Path, content: &[u8]) -> anyhow::Result<ExtractionResult> {
        self.compiled()?;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.config.ts_language)?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter parse failed for {}", path.display()))?;

        let file = path.to_string_lossy().replace('\\', "/");
        let mut result = ExtractionResult::new();

        // The file itself is a symbol: it owns top-level code and imports.
        let file_name = file.rsplit('/').next().unwrap_or(&file).to_string();
        let lines = content.iter().filter(|&&b| b == b'\n').count() as u32;
        let file_sym = Symbol::new(
            &self.config.lang_id,
            "file",
            file_name,
            file.clone(),
            file.clone(),
            Span {
                start_byte: 0,
                end_byte: content.len() + 1,
                start_line: 0,
                end_line: lines,
            },
        );
        result.symbols.push(file_sym);
        result.symbols.extend(self.extract_symbols(&file, &tree, content));

        let index = SpanIndex::new(&result.symbols);
        index.assign_parents(&mut result.symbols);
        self.extract_facts(&tree, content, &index, &mut result);

        Ok(result)
    }
}

// ── Span index: innermost enclosing symbol lookups ─────────────────────

/// Symbols sorted by start (ties: larger first), for innermost lookups.
/// Symbol spans are properly nested, so the innermost symbol containing a
/// position is the containing one with the largest start.
struct SpanIndex {
    order: Vec<usize>,
}

impl SpanIndex {
    fn new(symbols: &[Symbol]) -> Self {
        let mut order: Vec<usize> = (0..symbols.len()).collect();
        order.sort_by_key(|&i| {
            (
                symbols[i].span.start_byte,
                std::cmp::Reverse(symbols[i].span.end_byte),
            )
        });
        Self { order }
    }

    /// Innermost symbol containing `byte` (falls back to the file symbol,
    /// which is always `symbols[0]`).
    fn innermost(&self, symbols: &[Symbol], byte: usize) -> SymbolId {
        let upper = self
            .order
            .partition_point(|&i| symbols[i].span.start_byte <= byte);
        for &i in self.order[..upper].iter().rev() {
            if symbols[i].span.contains(byte) {
                return symbols[i].id;
            }
        }
        symbols[0].id
    }

    /// Set `parent` of every symbol to its innermost strict container.
    fn assign_parents(&self, symbols: &mut [Symbol]) {
        let mut stack: Vec<usize> = Vec::new();
        let mut parents = vec![None; symbols.len()];
        for &i in &self.order {
            let s = &symbols[i].span;
            while let Some(&top) = stack.last() {
                let t = &symbols[top].span;
                if t.start_byte <= s.start_byte && s.end_byte <= t.end_byte {
                    break;
                }
                stack.pop();
            }
            parents[i] = stack.last().map(|&p| symbols[p].id);
            stack.push(i);
        }
        for (i, p) in parents.into_iter().enumerate() {
            symbols[i].parent = p;
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn text(node: tree_sitter::Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn property(q: &tree_sitter::Query, pattern: usize, key: &str) -> Option<String> {
    q.property_settings(pattern)
        .iter()
        .find(|p| &*p.key == key)
        .and_then(|p| p.value.as_ref().map(|v| v.to_string()))
}

fn strip_quotes(s: &str) -> String {
    s.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '<' || c == '>').to_string()
}

/// Last segment of a module path: `crate::util` → `util`, `./a/b` → `b`.
pub fn last_segment(path: &str) -> &str {
    path.rsplit(|c| c == ':' || c == '.' || c == '/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ScopeCat {
    Type,
    Module,
    Function,
}

fn scope_category(kind: &str) -> Option<ScopeCat> {
    match kind {
        "class_definition" | "class_declaration" | "abstract_class_declaration" | "class"
        | "struct_item" | "impl_item" | "trait_item" | "interface_declaration" | "enum_item"
        | "enum_declaration" | "struct_specifier" | "union_specifier" | "enum_specifier"
        | "type_spec" | "union_item" => Some(ScopeCat::Type),
        "mod_item" | "module" | "internal_module" | "namespace_definition" => Some(ScopeCat::Module),
        "function_definition" | "function_declaration" | "function_item" | "method_definition"
        | "method_declaration" | "generator_function_declaration" => Some(ScopeCat::Function),
        _ => None,
    }
}

/// Names of enclosing scopes (outermost first) and the category of the
/// innermost one.
fn scope_chain(def: tree_sitter::Node, src: &[u8]) -> (Vec<String>, Option<ScopeCat>) {
    let mut chain = Vec::new();
    let mut innermost = None;
    let mut cur = def.parent();
    while let Some(p) = cur {
        if let Some(cat) = scope_category(p.kind()) {
            let name = if p.kind() == "impl_item" {
                p.child_by_field_name("type").and_then(|n| normalize_type(&text(n, src)))
            } else {
                p.child_by_field_name("name").map(|n| text(n, src))
            };
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                chain.push(name);
                if innermost.is_none() {
                    innermost = Some(cat);
                }
            }
        }
        cur = p.parent();
    }
    chain.reverse();
    (chain, innermost)
}

/// Infer the kind of a definition node, looking through wrappers such as
/// decorators, `export`, and `const f = () => ...`.
fn kind_for(def: tree_sitter::Node, _src: &[u8]) -> String {
    let mut node = def;
    match node.kind() {
        "decorated_definition" => {
            if let Some(d) = node.child_by_field_name("definition") {
                node = d;
            }
        }
        "export_statement" => {
            if let Some(d) = node.child_by_field_name("declaration") {
                node = d;
            }
        }
        _ => {}
    }
    if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
        if let Some(d) = node.named_child(0) {
            node = d;
        }
    }
    if node.kind() == "variable_declarator" {
        if let Some(v) = node.child_by_field_name("value") {
            if matches!(
                v.kind(),
                "arrow_function" | "function_expression" | "function" | "generator_function"
            ) {
                return "function".into();
            }
        }
    }
    if node.kind() == "type_spec" {
        return match node.child_by_field_name("type").map(|t| t.kind()) {
            Some("struct_type") => "struct".into(),
            Some("interface_type") => "interface".into(),
            _ => "type".into(),
        };
    }
    infer_kind(node.kind())
}

/// For `const f = (x): T => ...`, the return type of the arrow function.
fn arrow_return_type(def: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let decl = if def.kind() == "variable_declarator" {
        def
    } else {
        def.named_child(0).filter(|n| n.kind() == "variable_declarator")?
    };
    let v = decl.child_by_field_name("value")?;
    let rt = v.child_by_field_name("return_type")?;
    clean_type(&text(rt, src))
}

/// A type annotation as written, minus `:` / `->` and extra whitespace.
pub fn clean_type(t: &str) -> Option<String> {
    let t = t.trim().trim_start_matches(':').trim().trim_start_matches("->").trim();
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() || t.len() > 160 {
        None
    } else {
        Some(t)
    }
}

/// Collection types whose element type is what a loop variable gets.
const COLLECTIONS: &[&str] = &[
    "Vec", "VecDeque", "HashSet", "BTreeSet", "LinkedList", "BinaryHeap", "Set", "Array",
    "ReadonlyArray", "List", "list", "Iterable", "Iterator", "IntoIterator", "Sequence",
    "Collection", "set", "tuple", "Tuple", "IntoIter", "Iter", "frozenset", "Generator",
    "AsyncIterable", "AsyncIterator",
];

/// Element type of a collection type: `Vec<Box<dyn Shape>>` → `Shape`,
/// `Listener[]` → `Listener`, `&[Circle]` → `Circle`, `list[User]` → `User`.
pub fn element_type(t: &str) -> Option<String> {
    let mut s = t.trim();
    loop {
        let before = s;
        for p in ["&", "mut ", "readonly "] {
            s = s.strip_prefix(p).unwrap_or(s).trim_start();
        }
        if s.starts_with('\'') {
            s = s.split_once(' ').map(|x| x.1).unwrap_or("");
        }
        if s == before {
            break;
        }
    }
    if let Some(inner) = s.strip_suffix("[]") {
        return normalize_type(inner);
    }
    if let Some(inner) = s.strip_prefix("[]") {
        return normalize_type(inner);
    }
    if let Some(inner) = s.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        // Rust slices / arrays: `[T]`, `[T; N]`.
        return normalize_type(inner.split(';').next().unwrap_or(inner));
    }
    let open = s.find(['<', '['])?;
    let base = s[..open].rsplit(|c| c == ':' || c == '.').next().unwrap_or("");
    if !COLLECTIONS.contains(&base) {
        return None;
    }
    let inner = &s[open + 1..s.rfind(['>', ']'])?];
    normalize_type(inner)
}

/// Infer a kind string from the tree-sitter node type.
fn infer_kind(node_kind: &str) -> String {
    match node_kind {
        "function_definition"
        | "function_declaration"
        | "function_item"
        | "function_signature_item"
        | "method_definition"
        | "method_declaration"
        | "method_signature"
        | "method_elem"
        | "generator_function_declaration"
        | "abstract_method_signature" => "function".to_string(),
        "class_definition" | "class_declaration" | "abstract_class_declaration" => {
            "class".to_string()
        }
        "struct_item" | "struct_specifier" => "struct".to_string(),
        "enum_item" | "enum_specifier" | "enum_declaration" => "enum".to_string(),
        "enum_variant" | "enumerator" | "enum_assignment" => "enum_variant".to_string(),
        "type_alias_declaration" | "type_item" | "type_definition" => "type_alias".to_string(),
        "interface_declaration" => "interface".to_string(),
        "impl_item" => "impl".to_string(),
        "trait_item" => "trait".to_string(),
        "module" | "mod_item" | "internal_module" => "module".to_string(),
        "field_declaration" | "public_field_definition" | "field_definition"
        | "property_signature" => "field".to_string(),
        "macro_definition" | "preproc_def" | "preproc_function_def" => "macro".to_string(),
        "const_item" | "const_spec" | "const_declaration" => "constant".to_string(),
        "static_item" => "static".to_string(),
        "union_specifier" | "union_item" => "union".to_string(),
        "type_spec" | "type_declaration" => "type".to_string(),
        "var_declaration" | "var_spec" => "variable".to_string(),
        "decorated_definition" => "decorated".to_string(),
        "lexical_declaration" | "variable_declarator" | "variable_declaration" => {
            "variable".to_string()
        }
        "assignment" | "expression_statement" | "declaration" | "init_declarator" => {
            "variable".to_string()
        }
        _ => "definition".to_string(),
    }
}

/// Reduce a type expression to the bare type name used for member lookup:
/// `&mut Store` → `Store`, `*store.Store` → `Store`, `Option<Store>` →
/// `Store`, `Promise<Cache> | null` → `Cache`, `Vec<u8>` → `Vec`.
pub fn normalize_type(t: &str) -> Option<String> {
    let mut s = t.trim();
    s = s.trim_start_matches(':').trim();
    s = s.trim_start_matches("->").trim();
    s = s.trim_matches(|c| c == '"' || c == '\'');
    loop {
        let before = s;
        for p in ["&", "*", "mut ", "dyn ", "impl ", "const ", "readonly ", "typeof ", "[]"] {
            s = s.strip_prefix(p).unwrap_or(s).trim_start();
        }
        if s.starts_with('\'') {
            // lifetime: `'a Store`
            s = s.split_once(' ').map(|x| x.1).unwrap_or("");
        }
        if s == before {
            break;
        }
    }
    let base_end = s
        .find(|c: char| matches!(c, '<' | '[' | '(' | '|' | ' ' | ',' | '{' | '?' | '&' | ';' | '='))
        .unwrap_or(s.len());
    let base = &s[..base_end];
    let base = base.rsplit(|c| c == ':' || c == '.').next().unwrap_or(base);

    // Look through common wrappers to the wrapped type.
    const WRAPPERS: &[&str] = &[
        "Option", "Optional", "Box", "Rc", "Arc", "RefCell", "Cell", "Mutex", "RwLock", "Promise",
        "Readonly", "Partial", "Ref", "Weak", "Pin",
    ];
    if WRAPPERS.contains(&base) {
        let rest = &s[base_end..];
        if rest.starts_with('<') || rest.starts_with('[') {
            // Contents up to the matching close bracket.
            let mut depth = 0;
            let mut end = rest.len();
            for (i, c) in rest.char_indices() {
                match c {
                    '<' | '[' => depth += 1,
                    '>' | ']' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(t) = normalize_type(&rest[1..end]) {
                return Some(t);
            }
        }
    }

    let valid = !base.is_empty()
        && base.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !base.chars().next().unwrap().is_ascii_digit();
    valid.then(|| base.to_string())
}

/// Extract a one-line signature: the definition text up to its body.
fn extract_signature(node: tree_sitter::Node, source: &[u8], lang: &str) -> String {
    let text = node.utf8_text(source).unwrap_or("");
    let mut depth = 0i32;
    let mut end_idx = text.len();
    let bytes = text.as_bytes();

    for (i, ch) in text.char_indices() {
        match ch {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' => depth -= 1,
            '>' if i > 0 && bytes[i - 1] != b'-' && bytes[i - 1] != b'=' => depth -= 1,
            '{' if depth <= 0 => {
                end_idx = i;
                break;
            }
            '=' if depth <= 0 && bytes.get(i + 1) == Some(&b'>') => {
                end_idx = i + 2;
                break;
            }
            ':' if depth <= 0 && lang == "python" => {
                let after = text.get(i + 1..).unwrap_or("");
                if after.starts_with('\n') || after.starts_with(' ') || after.is_empty() {
                    end_idx = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    let sig: String = text[..end_idx].split_whitespace().collect::<Vec<_>>().join(" ");
    if sig.len() > 300 {
        let mut cut = 300;
        while !sig.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &sig[..cut])
    } else {
        sig
    }
}

/// Doc comments from preceding comment siblings (skipping attributes /
/// decorators), or a Python docstring.
fn extract_doc(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut sibling = node.prev_sibling();
    while let Some(sib) = sibling {
        let kind = sib.kind();
        if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
            if let Ok(text) = sib.utf8_text(source) {
                lines.push(text.to_string());
            }
        } else if !matches!(kind, "attribute_item" | "decorator" | "inner_attribute_item") {
            break;
        }
        sibling = sib.prev_sibling();
    }
    lines.reverse();

    if lines.is_empty() {
        // Python docstring: first statement of the body is a string.
        let def = if node.kind() == "decorated_definition" {
            node.child_by_field_name("definition").unwrap_or(node)
        } else {
            node
        };
        if let Some(body) = def.child_by_field_name("body") {
            if let Some(first) = body.named_child(0) {
                if first.kind() == "expression_statement" {
                    if let Some(s) = first.named_child(0).filter(|s| s.kind() == "string") {
                        let t = s.utf8_text(source).unwrap_or("");
                        let t = t.trim_matches(|c| c == '"' || c == '\'').trim();
                        if !t.is_empty() {
                            return Some(t.to_string());
                        }
                    }
                }
            }
        }
        return None;
    }

    let cleaned: Vec<String> = lines
        .iter()
        .flat_map(|l| l.lines().map(|x| x.to_string()).collect::<Vec<_>>())
        .map(|l| {
            l.trim()
                .trim_start_matches("///")
                .trim_start_matches("//!")
                .trim_start_matches("//")
                .trim_start_matches("/**")
                .trim_start_matches("/*")
                .trim_end_matches("*/")
                .trim_start_matches('*')
                .trim_start_matches('#')
                .trim()
                .to_string()
        })
        .collect();

    let doc = cleaned.join("\n").trim().to_string();
    if doc.is_empty() {
        None
    } else {
        Some(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_kind_functions() {
        assert_eq!(infer_kind("function_definition"), "function");
        assert_eq!(infer_kind("function_declaration"), "function");
        assert_eq!(infer_kind("function_item"), "function");
        assert_eq!(infer_kind("method_definition"), "function");
        assert_eq!(infer_kind("method_declaration"), "function");
        assert_eq!(infer_kind("generator_function_declaration"), "function");
        assert_eq!(infer_kind("abstract_method_signature"), "function");
    }

    #[test]
    fn infer_kind_classes_structs() {
        assert_eq!(infer_kind("class_definition"), "class");
        assert_eq!(infer_kind("class_declaration"), "class");
        assert_eq!(infer_kind("struct_item"), "struct");
        assert_eq!(infer_kind("struct_specifier"), "struct");
    }

    #[test]
    fn infer_kind_enums() {
        assert_eq!(infer_kind("enum_item"), "enum");
        assert_eq!(infer_kind("enum_specifier"), "enum");
        assert_eq!(infer_kind("enum_declaration"), "enum");
        assert_eq!(infer_kind("enum_variant"), "enum_variant");
        assert_eq!(infer_kind("enumerator"), "enum_variant");
        assert_eq!(infer_kind("enum_assignment"), "enum_variant");
    }

    #[test]
    fn infer_kind_types() {
        assert_eq!(infer_kind("type_alias_declaration"), "type_alias");
        assert_eq!(infer_kind("type_item"), "type_alias");
        assert_eq!(infer_kind("type_definition"), "type_alias");
        assert_eq!(infer_kind("interface_declaration"), "interface");
    }

    #[test]
    fn infer_kind_rust_specific() {
        assert_eq!(infer_kind("impl_item"), "impl");
        assert_eq!(infer_kind("trait_item"), "trait");
        assert_eq!(infer_kind("mod_item"), "module");
        assert_eq!(infer_kind("macro_definition"), "macro");
        assert_eq!(infer_kind("const_item"), "constant");
        assert_eq!(infer_kind("static_item"), "static");
    }

    #[test]
    fn infer_kind_c_specific() {
        assert_eq!(infer_kind("preproc_def"), "macro");
        assert_eq!(infer_kind("preproc_function_def"), "macro");
        assert_eq!(infer_kind("union_specifier"), "union");
    }

    #[test]
    fn infer_kind_fields_and_variables() {
        assert_eq!(infer_kind("field_declaration"), "field");
        assert_eq!(infer_kind("public_field_definition"), "field");
        assert_eq!(infer_kind("field_definition"), "field");
        assert_eq!(infer_kind("lexical_declaration"), "variable");
        assert_eq!(infer_kind("variable_declarator"), "variable");
        assert_eq!(infer_kind("var_declaration"), "variable");
        assert_eq!(infer_kind("var_spec"), "variable");
    }

    #[test]
    fn infer_kind_unknown_defaults_to_definition() {
        assert_eq!(infer_kind("some_unknown_node"), "definition");
    }

    #[test]
    fn normalize_type_cases() {
        assert_eq!(normalize_type("&mut Store").as_deref(), Some("Store"));
        assert_eq!(normalize_type("*store.Store").as_deref(), Some("Store"));
        assert_eq!(normalize_type("Option<Store>").as_deref(), Some("Store"));
        assert_eq!(normalize_type(": Promise<Cache> | null").as_deref(), Some("Cache"));
        assert_eq!(normalize_type("Vec<u8>").as_deref(), Some("Vec"));
        assert_eq!(normalize_type("crate::a::B").as_deref(), Some("B"));
        assert_eq!(normalize_type("Optional[Store]").as_deref(), Some("Store"));
        assert_eq!(normalize_type("'Cache'").as_deref(), Some("Cache"));
        assert_eq!(normalize_type("&'a str").as_deref(), Some("str"));
        assert_eq!(normalize_type("-> Self").as_deref(), Some("Self"));
        assert_eq!(normalize_type("(a, b)"), None);
    }

    #[test]
    fn element_type_cases() {
        assert_eq!(element_type("Vec<Box<dyn Shape>>").as_deref(), Some("Shape"));
        assert_eq!(element_type("Listener[]").as_deref(), Some("Listener"));
        assert_eq!(element_type("&[Circle]").as_deref(), Some("Circle"));
        assert_eq!(element_type("list[User]").as_deref(), Some("User"));
        assert_eq!(element_type("[]*store.Store").as_deref(), Some("Store"));
        assert_eq!(element_type("HashMap<String, User>"), None);
        assert_eq!(element_type("Store"), None);
    }

    #[test]
    fn last_segment_cases() {
        assert_eq!(last_segment("crate::util"), "util");
        assert_eq!(last_segment("./a/b"), "b");
        assert_eq!(last_segment(".util"), "util");
        assert_eq!(last_segment("example.com/app/store"), "store");
    }

    #[test]
    fn extract_signature_simple() {
        let src = b"fn main() {\n    println!(\"hello\");\n}";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        let root = tree.root_node();
        let func = root.child(0).unwrap();
        let sig = extract_signature(func, src, "rust");
        assert!(sig.contains("fn main()"));
        assert!(!sig.contains("println"));
    }

    #[test]
    fn extract_signature_keeps_generics_and_return_type() {
        let src = b"pub fn get<T: Clone>(x: Vec<T>) -> Option<T> where T: Copy { None }";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        let func = tree.root_node().child(0).unwrap();
        let sig = extract_signature(func, src, "rust");
        assert_eq!(sig, "pub fn get<T: Clone>(x: Vec<T>) -> Option<T> where T: Copy");
    }
}
