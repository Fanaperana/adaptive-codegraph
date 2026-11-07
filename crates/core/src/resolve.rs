//! # Resolution
//!
//! Turns per-file raw facts ([`RawRef`], [`Import`], [`Binding`]) into graph
//! [`Edge`]s for the whole project. Resolution is cheap compared to parsing,
//! so it always runs over every file — which keeps incremental and full
//! indexing identical.
//!
//! The rules are language-agnostic; per-language differences come from the
//! `.scm` captures and the [`LangProfile`].
//!
//! | reference                  | strategy                                             |
//! |----------------------------|------------------------------------------------------|
//! | `f()`                      | enclosing scopes → imports → package → global/guess  |
//! | `Type::f()` / `mod::f()`   | imported/known type members → module top-level items |
//! | `x.f()`                    | infer type of `x` (self, bindings, fields, imports)  |
//! | `class A(B)` / `impl T for`| supertype; member lookups walk supertypes            |
//!
//! When the target cannot be pinned down the reference is left unresolved
//! (or resolved as a low-confidence guess) rather than linked to an
//! arbitrary same-named symbol.
//!
//! Besides the references found in source, resolution derives:
//! - `imports` edges (file → imported symbol / module file)
//! - `references` edges (symbol → project types in its signature / locals)
//! - `overrides` edges (method → the supertype method it overrides/implements)

use crate::extract::treesitter::{element_type, normalize_type};
use crate::extract::LangProfile;
use crate::model::{confidence, Binding, Edge, FileFacts, Import, RawRef, Symbol, SymbolId};
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Counters describing how references were resolved.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ResolveStats {
    pub refs: usize,
    /// Resolved references by confidence (index 1..=3).
    pub resolved: [usize; 4],
    /// References to code outside the project (std lib, packages).
    pub external: usize,
    /// Several plausible targets; left unlinked.
    pub ambiguous: usize,
    /// No plausible target found.
    pub unresolved: usize,
}

/// A call the resolver could not link to one target: its name matched
/// several symbols, or none it could prove. Kept so `callers` can list it
/// as a possible caller instead of dropping it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Unlinked {
    /// Innermost symbol containing the call.
    pub from: SymbolId,
    pub name: String,
    /// 0-based line.
    pub line: u32,
    /// `true` when several targets matched, `false` when none did.
    pub ambiguous: bool,
}

/// A package / crate / module root declared by a manifest
/// (`Cargo.toml`, `go.mod`, `package.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Package {
    /// Import name, e.g. `my_crate`, `example.com/app`, `@scope/ui`.
    pub name: String,
    /// Workspace-relative directory.
    pub dir: String,
}

/// Names that refer to the current instance / type.
const SELF_NAMES: &[&str] = &["self", "this", "Self", "cls"];

/// Method names too common to guess from the name alone.
const COMMON_METHODS: &[&str] = &[
    "new", "len", "get", "set", "push", "pop", "insert", "remove", "clear", "iter", "map",
    "filter", "find", "next", "clone", "to_string", "into", "from", "as_str", "unwrap", "expect",
    "ok", "err", "is_empty", "contains", "join", "split", "trim", "append", "extend", "keys",
    "values", "items", "update", "add", "delete", "has", "forEach", "then", "catch", "apply",
    "call", "bind", "toString", "valueOf", "write", "read", "close", "open", "run", "start",
    "stop", "send", "emit", "on", "off", "init", "reset", "flush", "size", "length", "format",
    "parse", "equals", "hashCode", "sort", "reverse", "slice", "splice", "indexOf", "includes",
    "some", "every", "reduce", "log", "error", "warn", "info", "debug", "print", "println",
    "default", "build", "load", "save", "dispose", "handle", "execute", "process", "render",
    "create", "lock", "borrow", "borrow_mut", "as_ref", "as_mut", "collect", "fmt", "eq",
    "cmp", "hash", "drop", "deref", "index", "Get", "Set", "Close", "String", "Error",
    "fire", "event", "register", "dispatch", "notify", "visit", "accept", "match", "test",
];

/// What an import binds.
#[derive(Clone, Debug)]
enum ImportTarget {
    /// A whole module (one file, or several for directory-packages).
    Module(Vec<usize>),
    Symbol(SymbolId),
    External,
    Unknown,
}

/// The inferred type of a receiver expression.
#[derive(Clone, Debug)]
enum RecvType {
    /// A named type (may or may not be defined in the project).
    Type(String),
    Module(Vec<usize>),
    External,
    Unknown,
}

/// Outcome of resolving one reference.
#[derive(Clone, Debug)]
enum Target {
    Sym(SymbolId, u8),
    External,
    Ambiguous,
    Unknown,
}

struct FileInfo<'a> {
    facts: &'a FileFacts,
    dir: String,
    /// Module path segments, e.g. `src/store/mod.rs` → [src, store].
    segs: Vec<String>,
    profile: LangProfile,
}

pub struct Resolver<'a> {
    files: Vec<FileInfo<'a>>,
    syms: AHashMap<SymbolId, &'a Symbol>,
    sym_file: AHashMap<SymbolId, usize>,
    /// (owner type name, member name) → members.
    members: AHashMap<(&'a str, &'a str), Vec<SymbolId>>,
    /// (parent, name) → children.
    children: AHashMap<(SymbolId, &'a str), Vec<SymbolId>>,
    /// Type name → type symbols (incl. impl blocks).
    types: AHashMap<&'a str, Vec<SymbolId>>,
    /// Last module segment → files.
    module_files: AHashMap<String, Vec<usize>>,
    /// Full module path (`src/store`) → files.
    modpath_files: AHashMap<String, Vec<usize>>,
    /// Directory → files.
    dir_files: AHashMap<String, Vec<usize>>,
    /// Top-level name → top-level symbols (project-wide).
    top_level: AHashMap<&'a str, Vec<SymbolId>>,
    /// Method name → methods (project-wide).
    methods: AHashMap<&'a str, Vec<SymbolId>>,
    /// (scope, name) → binding.
    bindings: AHashMap<(SymbolId, &'a str), &'a Binding>,
    /// (owner type, attribute) → `self.x = ...` / `this.x = ...` binding.
    attrs: AHashMap<(String, String), &'a Binding>,
    /// Type name → direct supertype names.
    supers: AHashMap<String, Vec<String>>,
    /// file → local name → import target.
    imports: Vec<AHashMap<&'a str, ImportTarget>>,
    /// file → glob-imported modules.
    globs: Vec<Vec<Vec<usize>>>,
    packages: Vec<Package>,
}

const MAX_DEPTH: u8 = 4;

impl<'a> Resolver<'a> {
    pub fn new(
        facts: &'a BTreeMap<String, FileFacts>,
        profiles: &HashMap<String, LangProfile>,
        packages: &[Package],
    ) -> Self {
        let mut r = Resolver {
            files: Vec::with_capacity(facts.len()),
            syms: AHashMap::new(),
            sym_file: AHashMap::new(),
            members: AHashMap::new(),
            children: AHashMap::new(),
            types: AHashMap::new(),
            module_files: AHashMap::new(),
            modpath_files: AHashMap::new(),
            dir_files: AHashMap::new(),
            top_level: AHashMap::new(),
            methods: AHashMap::new(),
            bindings: AHashMap::new(),
            attrs: AHashMap::new(),
            supers: AHashMap::new(),
            imports: Vec::new(),
            globs: Vec::new(),
            packages: {
                let mut p = packages.to_vec();
                p.sort_by(|a, b| b.name.len().cmp(&a.name.len()));
                p
            },
        };

        for (fi, f) in facts.values().enumerate() {
            let profile = profiles.get(&f.lang).cloned().unwrap_or_default();
            let (dir, segs) = module_segments(&f.file, &profile);
            r.dir_files.entry(dir.clone()).or_default().push(fi);
            if let Some(last) = segs.last() {
                r.module_files.entry(last.clone()).or_default().push(fi);
            }
            r.modpath_files.entry(segs.join("/")).or_default().push(fi);
            r.files.push(FileInfo {
                facts: f,
                dir,
                segs,
                profile,
            });

            let file_sym = f.symbols.first().map(|s| s.id);
            for s in &f.symbols {
                r.syms.insert(s.id, s);
                r.sym_file.insert(s.id, fi);
                if let Some(p) = s.parent {
                    r.children.entry((p, s.name.as_str())).or_default().push(s.id);
                    if Some(p) == file_sym && s.kind != "file" {
                        r.top_level.entry(s.name.as_str()).or_default().push(s.id);
                    }
                }
                if let Some(owner) = s.owner() {
                    r.members.entry((owner, s.name.as_str())).or_default().push(s.id);
                }
                if s.is_type() {
                    r.types.entry(s.name.as_str()).or_default().push(s.id);
                }
                if s.kind == "method" {
                    r.methods.entry(s.name.as_str()).or_default().push(s.id);
                }
            }
            for b in &f.bindings {
                r.bindings.insert((b.scope, b.name.as_str()), b);
            }
        }

        // `self.x = ...` / `this.x = ...` become attributes of the enclosing type.
        let mut attrs = AHashMap::new();
        for f in facts.values() {
            for b in &f.bindings {
                let Some((obj, attr)) = b.name.split_once('.') else { continue };
                if SELF_NAMES.contains(&obj) && is_ident(attr) {
                    if let Some(owner) = r.enclosing_type(b.scope) {
                        attrs.entry((owner.to_string(), attr.to_string())).or_insert(b);
                    }
                }
            }
        }
        r.attrs = attrs;

        // Imports (needed by everything below).
        let mut imports = Vec::with_capacity(r.files.len());
        let mut globs = Vec::with_capacity(r.files.len());
        for fi in 0..r.files.len() {
            let mut map = AHashMap::new();
            let mut g = Vec::new();
            for imp in &r.files[fi].facts.imports {
                if imp.local == "*" {
                    let files = r.resolve_module(fi, &imp.path);
                    if !files.is_empty() {
                        g.push(files);
                    }
                    continue;
                }
                let t = r.resolve_import(fi, imp, 0);
                map.insert(imp.local.as_str(), t);
            }
            imports.push(map);
            globs.push(g);
        }
        r.imports = imports;
        r.globs = globs;

        // Supertypes, from `inherits` / `implements` references.
        let mut supers: AHashMap<String, Vec<String>> = AHashMap::new();
        for (fi, info) in r.files.iter().enumerate() {
            for rf in &info.facts.refs {
                if !is_super_kind(&rf.kind) {
                    continue;
                }
                let Some(from) = r.sym(rf.from) else { continue };
                let child = from.name.clone();
                let parent = match r.resolve_type_ref(fi, rf) {
                    Target::Sym(id, _) => r.sym(id).map(|s| s.name.clone()),
                    _ => None,
                }
                .unwrap_or_else(|| rf.name.clone());
                if parent != child {
                    let v = supers.entry(child).or_default();
                    if !v.contains(&parent) {
                        v.push(parent);
                    }
                }
            }
        }
        r.supers = supers;
        r
    }

    /// Resolve every raw reference and import into edges.
    pub fn resolve_all(&self) -> (Vec<Edge>, ResolveStats, Vec<Unlinked>) {
        type Out = (Vec<(SymbolId, SymbolId, String, u8, u32)>, ResolveStats, Vec<Unlinked>);
        let per_file: Vec<Out> = (0..self.files.len())
            .into_par_iter()
            .map(|fi| self.resolve_file(fi))
            .collect();

        let mut stats = ResolveStats::default();
        let mut merged: AHashMap<(SymbolId, SymbolId, String), Edge> = AHashMap::new();
        let mut unlinked = Vec::new();
        for (edges, s, u) in per_file {
            unlinked.extend(u);
            stats.refs += s.refs;
            for i in 0..4 {
                stats.resolved[i] += s.resolved[i];
            }
            stats.external += s.external;
            stats.ambiguous += s.ambiguous;
            stats.unresolved += s.unresolved;
            for (from, to, kind, conf, line) in edges {
                if from == to && kind != "calls" {
                    continue;
                }
                merged
                    .entry((from, to, kind.clone()))
                    .and_modify(|e| {
                        e.confidence = e.confidence.max(conf);
                        e.line = e.line.min(line);
                        e.count += 1;
                    })
                    .or_insert(Edge {
                        from,
                        to,
                        kind,
                        confidence: conf,
                        line,
                        count: 1,
                    });
            }
        }
        let mut edges: Vec<Edge> = merged.into_values().collect();
        edges.sort_by(|a, b| (a.from, a.to, &a.kind).cmp(&(b.from, b.to, &b.kind)));
        (edges, stats, unlinked)
    }

    fn resolve_file(&self, fi: usize) -> (Vec<(SymbolId, SymbolId, String, u8, u32)>, ResolveStats, Vec<Unlinked>) {
        let info = &self.files[fi];
        let mut out = Vec::new();
        let mut unlinked = Vec::new();
        let mut stats = ResolveStats::default();

        // Import edges: file → imported symbol / module file.
        if let Some(file_sym) = info.facts.symbols.first() {
            for imp in &info.facts.imports {
                match self.imports[fi].get(imp.local.as_str()) {
                    Some(ImportTarget::Symbol(id)) => {
                        out.push((file_sym.id, *id, "imports".into(), 3, imp.line))
                    }
                    Some(ImportTarget::Module(files)) => {
                        for &f in files {
                            if let Some(t) = self.files[f].facts.symbols.first() {
                                out.push((file_sym.id, t.id, "imports".into(), 3, imp.line));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // References found in source.
        for r in &info.facts.refs {
            stats.refs += 1;
            let t = if is_super_kind(&r.kind) || r.kind == "references" {
                self.resolve_type_ref(fi, r)
            } else {
                self.resolve_ref(fi, r)
            };
            match t {
                Target::Sym(to, conf) => {
                    stats.resolved[conf as usize] += 1;
                    out.push((r.from, to, r.kind.clone(), conf, r.line));
                }
                Target::External => stats.external += 1,
                Target::Ambiguous | Target::Unknown => {
                    let ambiguous = matches!(t, Target::Ambiguous);
                    if ambiguous {
                        stats.ambiguous += 1;
                    } else {
                        stats.unresolved += 1;
                    }
                    if matches!(r.kind.as_str(), "calls" | "renders") {
                        unlinked.push(Unlinked { from: r.from, name: r.name.clone(), line: r.line, ambiguous });
                    }
                }
            }
        }

        // Derived: type references from declared types and typed locals.
        for s in &info.facts.symbols {
            if let Some(t) = &s.type_hint {
                for name in type_names_in(t) {
                    if let Target::Sym(to, conf) = self.resolve_type_name(fi, s.id, &name) {
                        out.push((s.id, to, "references".into(), conf, s.span.start_line));
                    }
                }
            }
            // Derived: method overrides / implements a supertype method.
            if s.kind == "method" {
                if let Some(owner) = s.owner() {
                    if let Some(target) = self.find_in_supers(owner, &s.name) {
                        out.push((s.id, target, "overrides".into(), 3, s.span.start_line));
                    }
                }
            }
        }
        for b in &info.facts.bindings {
            if let Some(t) = &b.type_name {
                let line = self.sym(b.scope).map(|s| s.span.start_line).unwrap_or(0);
                for name in type_names_in(t) {
                    if let Target::Sym(to, conf) = self.resolve_type_name(fi, b.scope, &name) {
                        out.push((b.scope, to, "references".into(), conf, line));
                    }
                }
            }
        }
        (out, stats, unlinked)
    }

    fn resolve_ref(&self, fi: usize, r: &RawRef) -> Target {
        let tk = r.target_kind.as_deref();
        if let Some(q) = &r.qualifier {
            self.resolve_qualified(fi, r.from, q, &r.name, tk, 0)
        } else if let Some(recv) = &r.receiver {
            self.resolve_method(fi, r.from, recv, &r.name, 0)
        } else {
            self.resolve_plain(fi, r.from, &r.name, tk, 0)
        }
    }

    // ── helpers ────────────────────────────────────────────────────────

    fn sym(&self, id: SymbolId) -> Option<&'a Symbol> {
        self.syms.get(&id).copied()
    }

    fn ancestors(&self, id: SymbolId) -> impl Iterator<Item = &'a Symbol> + '_ {
        let mut cur = self.sym(id);
        std::iter::from_fn(move || {
            let s = cur?;
            cur = s.parent.and_then(|p| self.sym(p));
            Some(s)
        })
    }

    /// The type that `self`/`this` refers to inside `id`.
    fn enclosing_type(&self, id: SymbolId) -> Option<&'a str> {
        for s in self.ancestors(id) {
            if s.kind == "method" || s.kind == "field" {
                if let Some(o) = s.owner() {
                    return Some(o);
                }
            }
            if s.is_type() {
                return Some(s.name.as_str());
            }
        }
        None
    }

    fn binding(&self, scope: SymbolId, name: &str) -> Option<&'a Binding> {
        for s in self.ancestors(scope) {
            if let Some(b) = self.bindings.get(&(s.id, name)) {
                return Some(b);
            }
        }
        None
    }

    fn acceptable(&self, s: &Symbol, target_kind: Option<&str>) -> bool {
        match target_kind {
            Some(k) => s.kind == k,
            None => matches!(s.kind.as_str(), "function" | "class" | "struct"),
        }
    }

    fn file_of(&self, id: &SymbolId) -> Option<usize> {
        self.sym_file.get(id).copied()
    }

    /// Top-level symbols named `name` in the given files.
    fn top_level_in(&self, files: &[usize], name: &str) -> Vec<SymbolId> {
        let Some(v) = self.top_level.get(name) else { return Vec::new() };
        v.iter()
            .copied()
            .filter(|id| self.file_of(id).map(|f| files.contains(&f)).unwrap_or(false))
            .collect()
    }

    /// Direct members of type `owner` called `name` that can be called.
    fn callable_members(&self, owner: &str, name: &str) -> Vec<SymbolId> {
        let Some(v) = self.members.get(&(owner, name)) else { return Vec::new() };
        v.iter()
            .copied()
            .filter(|id| {
                self.sym(*id)
                    .map(|s| matches!(s.kind.as_str(), "method" | "function" | "class" | "struct"))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Supertypes of `owner`, nearest first (breadth-first, cycle-safe).
    fn supertypes(&self, owner: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut queue: std::collections::VecDeque<String> =
            self.supers.get(owner).cloned().unwrap_or_default().into();
        while let Some(t) = queue.pop_front() {
            if t == owner || out.contains(&t) || out.len() > 32 {
                continue;
            }
            if let Some(next) = self.supers.get(&t) {
                queue.extend(next.iter().cloned());
            }
            out.push(t);
        }
        out
    }

    /// A callable member `name` in one of `owner`'s supertypes.
    fn find_in_supers(&self, owner: &str, name: &str) -> Option<SymbolId> {
        for t in self.supertypes(owner) {
            if let Some(&id) = self.callable_members(&t, name).first() {
                return Some(id);
            }
        }
        None
    }

    /// Pick from candidates: prefer the caller's file, then its directory.
    fn pick(&self, fi: usize, cands: &[SymbolId], conf: u8) -> Target {
        match cands.len() {
            0 => Target::Unknown,
            1 => Target::Sym(cands[0], conf),
            _ => {
                let same_file: Vec<_> = cands.iter().filter(|id| self.file_of(id) == Some(fi)).collect();
                if same_file.len() == 1 {
                    return Target::Sym(*same_file[0], conf.min(confidence::INFERRED));
                }
                let dir = &self.files[fi].dir;
                let same_dir: Vec<_> = cands
                    .iter()
                    .filter(|id| self.file_of(id).map(|f| &self.files[f].dir == dir).unwrap_or(false))
                    .collect();
                if same_dir.len() == 1 {
                    return Target::Sym(*same_dir[0], conf.min(confidence::INFERRED));
                }
                Target::Ambiguous
            }
        }
    }

    // ── plain calls: `f()` ─────────────────────────────────────────────

    fn resolve_plain(&self, fi: usize, from: SymbolId, name: &str, tk: Option<&str>, depth: u8) -> Target {
        // 1. Enclosing scopes (nested functions, same-file top level).
        for anc in self.ancestors(from) {
            if anc.is_type() {
                continue; // members need an explicit receiver
            }
            if let Some(kids) = self.children.get(&(anc.id, name)) {
                if let Some(k) = kids.iter().find(|k| self.sym(**k).map(|s| self.acceptable(s, tk)).unwrap_or(false)) {
                    return Target::Sym(*k, confidence::CERTAIN);
                }
            }
        }

        // 2. Imports, then glob imports.
        match self.imports[fi].get(name) {
            Some(ImportTarget::Symbol(id)) => {
                if let Some(s) = self.sym(*id) {
                    if self.acceptable(s, tk) || s.kind == "method" {
                        return Target::Sym(*id, confidence::CERTAIN);
                    }
                }
                return Target::Unknown;
            }
            Some(ImportTarget::External) => return Target::External,
            Some(ImportTarget::Module(_)) => return Target::Unknown,
            Some(ImportTarget::Unknown) | None => {}
        }
        for files in &self.globs[fi] {
            let c: Vec<SymbolId> = self
                .top_level_in(files, name)
                .into_iter()
                .filter(|id| self.sym(*id).map(|s| self.acceptable(s, tk)).unwrap_or(false))
                .collect();
            if !c.is_empty() {
                return self.pick(fi, &c, confidence::CERTAIN);
            }
        }

        let profile = &self.files[fi].profile;
        let all: Vec<SymbolId> = self
            .top_level
            .get(name)
            .map(|v| v.iter().copied().filter(|id| self.sym(*id).map(|s| self.acceptable(s, tk)).unwrap_or(false)).collect())
            .unwrap_or_default();

        // 3. Same package (directory-scoped languages such as Go).
        if profile.module_mode == "directory" {
            let dir = &self.files[fi].dir;
            let in_pkg: Vec<SymbolId> = all
                .iter()
                .copied()
                .filter(|id| self.file_of(id).map(|f| &self.files[f].dir == dir).unwrap_or(false))
                .collect();
            if !in_pkg.is_empty() {
                return self.pick(fi, &in_pkg, confidence::CERTAIN);
            }
        }

        // 4. Global-scope languages (C): any top-level definition.
        if profile.scope == "global" {
            return self.pick(fi, &all, confidence::INFERRED);
        }

        // 5. Last resort: a unique project-wide name is a guess.
        let _ = depth;
        match self.pick(fi, &all, confidence::GUESS) {
            Target::Sym(id, _) => Target::Sym(id, confidence::GUESS),
            other => other,
        }
    }

    // ── qualified calls: `Type::f()`, `module::f()` ────────────────────

    fn resolve_qualified(
        &self,
        fi: usize,
        from: SymbolId,
        qualifier: &str,
        name: &str,
        tk: Option<&str>,
        depth: u8,
    ) -> Target {
        let q = strip_generics(qualifier.trim().trim_start_matches("::"));
        let segs: Vec<&str> = q.split("::").filter(|s| !s.is_empty()).collect();
        let Some(&last) = segs.last() else {
            return self.resolve_plain(fi, from, name, tk, depth);
        };

        match last {
            "Self" => {
                return match self.enclosing_type(from) {
                    Some(t) => self.member_of(fi, t, name),
                    None => Target::Unknown,
                }
            }
            "self" | "crate" | "super" if segs.len() == 1 => {
                return self.resolve_plain(fi, from, name, tk, depth);
            }
            _ => {}
        }

        if segs.len() == 1 {
            match self.imports[fi].get(last) {
                Some(ImportTarget::Module(files)) => {
                    let c = self.top_level_in(files, name);
                    return if c.is_empty() { Target::Unknown } else { self.pick(fi, &c, confidence::CERTAIN) };
                }
                Some(ImportTarget::Symbol(id)) => {
                    if let Some(s) = self.sym(*id) {
                        if s.is_type() {
                            return self.member_of(fi, &s.name, name);
                        }
                    }
                    return Target::Unknown;
                }
                Some(ImportTarget::External) => return Target::External,
                _ => {}
            }
        }

        if self.types.contains_key(last) {
            return self.member_of(fi, last, name);
        }

        let files = self.resolve_module(fi, &segs.join("::"));
        if !files.is_empty() {
            let c = self.top_level_in(&files, name);
            return if c.is_empty() { Target::Unknown } else { self.pick(fi, &c, confidence::CERTAIN) };
        }
        Target::External
    }

    /// Callable member `name` of a named type (or of its supertypes).
    fn member_of(&self, fi: usize, owner: &str, name: &str) -> Target {
        let c = self.callable_members(owner, name);
        if !c.is_empty() {
            // Several hits usually means the same type split over impl blocks.
            return match self.pick(fi, &c, confidence::CERTAIN) {
                Target::Ambiguous => Target::Sym(c[0], confidence::INFERRED),
                t => t,
            };
        }
        if let Some(id) = self.find_in_supers(owner, name) {
            return Target::Sym(id, confidence::CERTAIN);
        }
        if self.types.contains_key(owner) {
            Target::Unknown // project type, member not found (external base, macro, ...)
        } else {
            Target::External
        }
    }

    // ── method calls: `x.f()` ──────────────────────────────────────────

    fn resolve_method(&self, fi: usize, from: SymbolId, recv: &str, name: &str, depth: u8) -> Target {
        match self.type_of(fi, from, recv, depth) {
            RecvType::Type(t) => self.member_of(fi, &t, name),
            RecvType::Module(files) => {
                let c: Vec<SymbolId> = self
                    .top_level_in(&files, name)
                    .into_iter()
                    .filter(|id| self.sym(*id).map(|s| self.acceptable(s, None)).unwrap_or(false))
                    .collect();
                if c.is_empty() {
                    Target::Unknown
                } else {
                    self.pick(fi, &c, confidence::CERTAIN)
                }
            }
            RecvType::External => Target::External,
            RecvType::Unknown => {
                if COMMON_METHODS.contains(&name) {
                    return Target::Ambiguous;
                }
                match self.methods.get(name).map(|v| v.as_slice()) {
                    None | Some([]) => Target::Unknown,
                    Some([only]) => Target::Sym(*only, confidence::GUESS),
                    Some(_) => Target::Ambiguous,
                }
            }
        }
    }

    /// Infer the type of a receiver expression.
    fn type_of(&self, fi: usize, from: SymbolId, expr: &str, depth: u8) -> RecvType {
        if depth > MAX_DEPTH {
            return RecvType::Unknown;
        }
        let e = expr.trim().trim_start_matches(['&', '*']).trim();
        let e = e.strip_prefix("mut ").unwrap_or(e).trim();
        let e = e.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(e).trim();

        if SELF_NAMES.contains(&e) {
            return match self.enclosing_type(from) {
                Some(t) => RecvType::Type(t.to_string()),
                None => RecvType::Unknown,
            };
        }
        // `super()` (Python), `super` (JS/TS/Java): the first supertype.
        if e == "super" || e.starts_with("super(") {
            return match self.enclosing_type(from).map(|t| self.supertypes(t)) {
                Some(s) if !s.is_empty() => RecvType::Type(s[0].clone()),
                _ => RecvType::Unknown,
            };
        }

        if is_ident(e) {
            if let Some(b) = self.binding(from, e) {
                return self.type_of_binding(fi, b, depth + 1);
            }
            match self.imports[fi].get(e) {
                Some(ImportTarget::Module(files)) => return RecvType::Module(files.clone()),
                Some(ImportTarget::Symbol(id)) => {
                    return match self.sym(*id) {
                        Some(s) if s.is_type() => RecvType::Type(s.name.clone()),
                        _ => RecvType::Unknown,
                    }
                }
                Some(ImportTarget::External) => return RecvType::External,
                _ => {}
            }
            if self.types.contains_key(e) {
                return RecvType::Type(e.to_string());
            }
            if let Some(files) = self.module_files.get(e) {
                if self.files[fi].profile.module_mode == "directory" || self.files[fi].facts.lang == "rust" {
                    return RecvType::Module(files.clone());
                }
            }
            return RecvType::Unknown;
        }

        // `a.b` (field access): resolve `a`, then the declared type of field `b`.
        if let Some((head, field)) = e.rsplit_once('.') {
            if is_ident(field) && !head.contains(['(', '[']) || head.starts_with("super(") {
                return match self.type_of(fi, from, head, depth + 1) {
                    RecvType::Type(t) => self.field_type(fi, &t, field, depth + 1),
                    RecvType::Module(files) => {
                        let c = self.top_level_in(&files, field);
                        match c.first().and_then(|id| self.sym(*id)) {
                            Some(s) if s.is_type() => RecvType::Type(s.name.clone()),
                            _ => RecvType::Unknown,
                        }
                    }
                    RecvType::External => RecvType::External,
                    RecvType::Unknown => RecvType::Unknown,
                };
            }
        }
        RecvType::Unknown
    }

    /// Declared (raw) type of field `field` on `owner` or its supertypes.
    fn field_raw_type(&self, owner: &str, field: &str) -> Option<(String, Option<&'a Binding>)> {
        let mut chain = vec![owner.to_string()];
        chain.extend(self.supertypes(owner));
        for t in &chain {
            if let Some(c) = self.members.get(&(t.as_str(), field)) {
                for id in c {
                    let Some(s) = self.sym(*id) else { continue };
                    if let Some(h) = &s.type_hint {
                        let h = if h == "Self" { t.clone() } else { h.clone() };
                        return Some((h, None));
                    }
                    // Field initializer: `private x = make()`.
                    if let Some(b) = self.bindings.get(&(s.id, s.name.as_str())) {
                        return Some((String::new(), Some(*b)));
                    }
                }
            }
            if let Some(b) = self.attrs.get(&(t.clone(), field.to_string())) {
                return Some((String::new(), Some(*b)));
            }
        }
        None
    }

    fn field_type(&self, fi: usize, owner: &str, field: &str, depth: u8) -> RecvType {
        match self.field_raw_type(owner, field) {
            Some((_, Some(b))) => self.type_of_binding(fi, b, depth + 1),
            Some((raw, None)) => match normalize_type(&raw) {
                Some(t) => RecvType::Type(t),
                None => RecvType::Unknown,
            },
            None if self.types.contains_key(owner) => RecvType::Unknown,
            None => RecvType::External,
        }
    }

    /// Raw declared type of an expression, used for loop element types.
    fn raw_type_of(&self, fi: usize, from: SymbolId, expr: &str, depth: u8) -> Option<String> {
        if depth > MAX_DEPTH {
            return None;
        }
        let e = expr.trim().trim_start_matches(['&', '*']).trim();
        let e = e.strip_prefix("mut ").unwrap_or(e).trim();
        let e = e.split(".iter()").next().unwrap_or(e);
        let e = e.split(".values()").next().unwrap_or(e).trim();
        if is_ident(e) {
            let b = self.binding(from, e)?;
            if let Some(t) = &b.type_name {
                return Some(t.clone());
            }
            if let Some(call) = &b.call {
                if let Target::Sym(id, _) = self.resolve_call_expr(fi, b.scope, call, depth + 1) {
                    return self.sym(id)?.type_hint.clone();
                }
            }
            return None;
        }
        let (head, field) = e.rsplit_once('.')?;
        if !is_ident(field) {
            return None;
        }
        let RecvType::Type(t) = self.type_of(fi, from, head, depth + 1) else { return None };
        match self.field_raw_type(&t, field)? {
            (raw, None) => Some(raw),
            (_, Some(b)) => b.type_name.clone(),
        }
    }

    fn type_of_binding(&self, fi: usize, b: &Binding, depth: u8) -> RecvType {
        if depth > MAX_DEPTH {
            return RecvType::Unknown;
        }
        if let Some(t) = b.type_name.as_deref().and_then(normalize_type) {
            if t == "Self" {
                return match self.enclosing_type(b.scope) {
                    Some(t) => RecvType::Type(t.to_string()),
                    None => RecvType::Unknown,
                };
            }
            return RecvType::Type(t);
        }
        if let Some(iter) = &b.iter {
            return match self.raw_type_of(fi, b.scope, iter, depth + 1).and_then(|t| element_type(&t)) {
                Some(t) => RecvType::Type(t),
                None => RecvType::Unknown,
            };
        }
        if let Some(call) = &b.call {
            let target = self.resolve_call_expr(fi, b.scope, call, depth + 1);
            return match target {
                Target::Sym(id, _) => {
                    let Some(s) = self.sym(id) else { return RecvType::Unknown };
                    if s.is_type() {
                        return RecvType::Type(s.name.clone());
                    }
                    match s.type_hint.as_deref().and_then(normalize_type).as_deref() {
                        Some("Self") => match s.owner() {
                            Some(o) => RecvType::Type(o.to_string()),
                            None => RecvType::Unknown,
                        },
                        Some(t) => RecvType::Type(t.to_string()),
                        None => RecvType::Unknown,
                    }
                }
                Target::External => RecvType::External,
                _ => RecvType::Unknown,
            };
        }
        RecvType::Unknown
    }

    /// Resolve a callee expression such as `Store::new`, `cache.NewCache` or `make`.
    fn resolve_call_expr(&self, fi: usize, scope: SymbolId, call: &str, depth: u8) -> Target {
        if depth > MAX_DEPTH {
            return Target::Unknown;
        }
        let call = strip_generics(call.trim());
        if let Some((q, n)) = call.rsplit_once("::") {
            return self.resolve_qualified(fi, scope, q, n, None, depth);
        }
        if let Some((recv, n)) = call.rsplit_once('.') {
            return self.resolve_method(fi, scope, recv, n, depth);
        }
        if is_ident(&call) {
            return self.resolve_plain(fi, scope, &call, None, depth);
        }
        Target::Unknown
    }

    // ── type references ────────────────────────────────────────────────

    /// Resolve a type name as seen from `from` (scopes, imports, package,
    /// then a unique project-wide type).
    fn resolve_type_name(&self, fi: usize, from: SymbolId, name: &str) -> Target {
        let is_target = |id: &SymbolId| self.sym(*id).map(|s| s.is_type() && s.kind != "impl").unwrap_or(false);
        for anc in self.ancestors(from) {
            if let Some(kids) = self.children.get(&(anc.id, name)) {
                if let Some(k) = kids.iter().find(|k| is_target(k)) {
                    return Target::Sym(*k, confidence::CERTAIN);
                }
            }
        }
        match self.imports[fi].get(name) {
            Some(ImportTarget::Symbol(id)) if is_target(id) => return Target::Sym(*id, confidence::CERTAIN),
            Some(ImportTarget::External) => return Target::External,
            _ => {}
        }
        for files in &self.globs[fi] {
            let c: Vec<SymbolId> = self.top_level_in(files, name).into_iter().filter(|id| is_target(id)).collect();
            if !c.is_empty() {
                return self.pick(fi, &c, confidence::CERTAIN);
            }
        }
        let Some(all) = self.types.get(name) else { return Target::External };
        let all: Vec<SymbolId> = all.iter().copied().filter(|id| is_target(id)).collect();
        // Types are often reachable through re-exports or glob imports we
        // can't see, so a unique project type is a fair inference.
        match self.pick(fi, &all, confidence::INFERRED) {
            Target::Sym(id, c) => Target::Sym(id, c.min(confidence::INFERRED)),
            t => t,
        }
    }

    /// Resolve an `inherits` / `implements` / `references` reference.
    fn resolve_type_ref(&self, fi: usize, r: &RawRef) -> Target {
        let q = r.qualifier.as_deref().or(r.receiver.as_deref());
        if let Some(q) = q {
            let q = strip_generics(q);
            let last = q.rsplit(|c| c == ':' || c == '.').next().unwrap_or(&q);
            let files = match self.imports[fi].get(last) {
                Some(ImportTarget::Module(files)) => files.clone(),
                Some(ImportTarget::External) => return Target::External,
                _ => self.resolve_module(fi, &q),
            };
            if !files.is_empty() {
                let c: Vec<SymbolId> = self
                    .top_level_in(&files, &r.name)
                    .into_iter()
                    .filter(|id| self.sym(*id).map(|s| s.is_type()).unwrap_or(false))
                    .collect();
                return if c.is_empty() { Target::Unknown } else { self.pick(fi, &c, confidence::CERTAIN) };
            }
        }
        self.resolve_type_name(fi, r.from, &r.name)
    }

    // ── imports & modules ──────────────────────────────────────────────

    fn resolve_import(&self, fi: usize, imp: &Import, depth: u8) -> ImportTarget {
        let files = self.resolve_module(fi, &imp.path);
        let Some(item) = &imp.item else {
            return if files.is_empty() { ImportTarget::External } else { ImportTarget::Module(files) };
        };
        if !files.is_empty() {
            if let Some(t) = self.find_exported(&files, item, depth) {
                return t;
            }
        }
        // `from . import other`, `use crate::util;`: the item is a module.
        let sub = join_module_path(&imp.path, item, &self.files[fi].facts.lang);
        let sub_files = self.resolve_module(fi, &sub);
        if !sub_files.is_empty() {
            return ImportTarget::Module(sub_files);
        }
        if files.is_empty() {
            ImportTarget::External
        } else {
            ImportTarget::Unknown
        }
    }

    /// Find `item` exported by `files`: a top-level symbol, or a re-export
    /// (`pub use`, `export { x } from`, `from .x import y` in `__init__`).
    fn find_exported(&self, files: &[usize], item: &str, depth: u8) -> Option<ImportTarget> {
        let c = self.top_level_in(files, item);
        if let Some(id) = c.first() {
            return Some(ImportTarget::Symbol(*id));
        }
        if depth >= 3 {
            return None;
        }
        for &f in files {
            for imp in &self.files[f].facts.imports {
                if imp.local == item {
                    match self.resolve_import(f, imp, depth + 1) {
                        t @ (ImportTarget::Symbol(_) | ImportTarget::Module(_)) => return Some(t),
                        _ => {}
                    }
                } else if imp.local == "*" {
                    let g = self.resolve_module(f, &imp.path);
                    if !g.is_empty() && g != files {
                        if let Some(t) = self.find_exported(&g, item, depth + 1) {
                            return Some(t);
                        }
                    }
                }
            }
        }
        None
    }

    /// Files implementing the module `path` as seen from file `fi`.
    fn resolve_module(&self, fi: usize, path: &str) -> Vec<usize> {
        let from = &self.files[fi];
        let p = path.trim();
        if p.is_empty() {
            return Vec::new();
        }

        // Relative paths: Python `.x` / `..x`, JS/TS `./x` / `../x`.
        let target = if from.facts.lang == "python" && p.starts_with('.') {
            let dots = p.chars().take_while(|&c| c == '.').count();
            let mut dir = from.dir.clone();
            for _ in 1..dots {
                dir = parent_dir(&dir);
            }
            let rest = p[dots..].replace('.', "/");
            Some(join_path(&dir, &rest))
        } else if p.starts_with("./") || p.starts_with("../") || p == "." || p == ".." {
            Some(normalize_path(&join_path(&from.dir, p)))
        } else {
            None
        };
        if let Some(target) = target {
            return self.modpath_files.get(&strip_ext(&target)).cloned().unwrap_or_default();
        }

        // Declared packages: `my_crate::store`, `example.com/app/util`, `@scope/ui/x`.
        let p = strip_ext(p);
        for pkg in &self.packages {
            let rest = if p == pkg.name {
                Some("")
            } else {
                p.strip_prefix(&pkg.name)
                    .and_then(|r| r.strip_prefix("::").or_else(|| r.strip_prefix('/')).or_else(|| r.strip_prefix('.')))
            };
            let Some(rest) = rest else { continue };
            let rest_segs: Vec<&str> = rest.split(|c| c == ':' || c == '/' || c == '.').filter(|s| !s.is_empty()).collect();
            let roots = [pkg.dir.clone(), join_path(&pkg.dir, "src"), join_path(&pkg.dir, "lib")];
            for root in &roots {
                let target = join_path(root, &rest_segs.join("/"));
                if let Some(v) = self.modpath_files.get(&target) {
                    return v.clone();
                }
            }
        }

        // Otherwise match module segments by suffix.
        let segs: Vec<&str> = p
            .split(|c| c == ':' || c == '.' || c == '/')
            .filter(|s| !s.is_empty() && !matches!(*s, "crate" | "self" | "super" | "@" | "~"))
            .collect();
        let Some(last) = segs.last() else { return Vec::new() };
        let Some(cands) = self.module_files.get(*last) else { return Vec::new() };

        let mut best = 0;
        let mut out = Vec::new();
        for &f in cands {
            let fs = &self.files[f].segs;
            let common = fs.iter().rev().zip(segs.iter().rev()).take_while(|(a, b)| a == *b).count();
            // Require the whole shorter path to match, so `std::collections`
            // does not resolve to a project file named `collections`.
            if common < segs.len().min(fs.len()) {
                continue;
            }
            match common.cmp(&best) {
                std::cmp::Ordering::Greater => {
                    best = common;
                    out = vec![f];
                }
                std::cmp::Ordering::Equal => out.push(f),
                _ => {}
            }
        }
        out
    }
}

/// Resolve all references for a set of files.
pub fn resolve(
    facts: &BTreeMap<String, FileFacts>,
    profiles: &HashMap<String, LangProfile>,
    packages: &[Package],
) -> (Vec<Edge>, ResolveStats, Vec<Unlinked>) {
    Resolver::new(facts, profiles, packages).resolve_all()
}

fn is_super_kind(kind: &str) -> bool {
    kind == "inherits" || kind == "implements"
}

/// Project-relevant type names inside a type expression:
/// `HashMap<String, Vec<User>>` → [HashMap, String, Vec, User].
fn type_names_in(t: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in t.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$' || c == ':' || c == '.')) {
        let tok = tok.trim_matches(|c| c == ':' || c == '.');
        let name = tok.rsplit(|c| c == ':' || c == '.').next().unwrap_or(tok);
        if name.is_empty()
            || !name.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
            || matches!(name, "mut" | "dyn" | "impl" | "const" | "readonly" | "Self" | "self" | "typeof" | "keyof" | "in" | "extends")
        {
            continue;
        }
        if !out.iter().any(|x| x == name) {
            out.push(name.to_string());
        }
    }
    out
}

// ── path helpers ───────────────────────────────────────────────────────

const MODULE_INDEX_NAMES: &[&str] = &["mod", "index", "__init__", "lib"];

/// Directory of a file and its module segments.
fn module_segments(file: &str, profile: &LangProfile) -> (String, Vec<String>) {
    let dir = parent_dir(file);
    let mut segs: Vec<String> = dir.split('/').filter(|s| !s.is_empty()).map(String::from).collect();
    if profile.module_mode != "directory" {
        let name = file.rsplit('/').next().unwrap_or(file);
        let stem = name.split('.').next().unwrap_or(name).to_string();
        if !MODULE_INDEX_NAMES.contains(&stem.as_str()) {
            segs.push(stem);
        }
    }
    (dir, segs)
}

fn parent_dir(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((d, _)) => d.to_string(),
        None => String::new(),
    }
}

fn join_path(dir: &str, rest: &str) -> String {
    match (dir.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (_, true) => dir.to_string(),
        _ => format!("{dir}/{rest}"),
    }
}

fn normalize_path(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            x => out.push(x),
        }
    }
    out.join("/")
}

fn strip_ext(p: &str) -> String {
    const EXTS: &[&str] = &[
        ".d.ts", ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".py", ".rs", ".go", ".c", ".h",
    ];
    let mut s = p.to_string();
    for e in EXTS {
        if let Some(x) = s.strip_suffix(e) {
            s = x.to_string();
            break;
        }
    }
    // `pkg/index` and `pkg/__init__` are the package itself.
    for idx in MODULE_INDEX_NAMES {
        if let Some(x) = s.strip_suffix(&format!("/{idx}")) {
            return x.to_string();
        }
        if s == *idx {
            return String::new();
        }
    }
    s
}

fn join_module_path(path: &str, item: &str, lang: &str) -> String {
    match lang {
        "python" => {
            if path.ends_with('.') {
                format!("{path}{item}")
            } else {
                format!("{path}.{item}")
            }
        }
        "rust" => format!("{path}::{item}"),
        _ => format!("{}/{item}", path.trim_end_matches('/')),
    }
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !s.chars().next().unwrap().is_ascii_digit()
}

/// `Vec::<u8>::new` → `Vec::new`, `foo<T>` → `foo`.
fn strip_generics(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("::::", "::").trim_end_matches("::").to_string()
}

#[allow(dead_code)]
fn unique(ids: Vec<SymbolId>) -> Vec<SymbolId> {
    let mut seen = AHashSet::new();
    ids.into_iter().filter(|i| seen.insert(*i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_segments_file_mode() {
        let p = LangProfile::default();
        assert_eq!(module_segments("src/store.rs", &p).1, vec!["src", "store"]);
        assert_eq!(module_segments("src/store/mod.rs", &p).1, vec!["src", "store"]);
        assert_eq!(module_segments("app/__init__.py", &p).1, vec!["app"]);
    }

    #[test]
    fn module_segments_directory_mode() {
        let p = LangProfile {
            module_mode: "directory".into(),
            ..Default::default()
        };
        assert_eq!(module_segments("store/store.go", &p).1, vec!["store"]);
    }

    #[test]
    fn path_helpers() {
        assert_eq!(normalize_path("src/a/../b/./c"), "src/b/c");
        assert_eq!(strip_ext("src/util.ts"), "src/util");
        assert_eq!(strip_ext("src/pkg/index.ts"), "src/pkg");
        assert_eq!(strip_generics("Vec::<u8>::new"), "Vec::new");
        assert_eq!(join_module_path(".", "other", "python"), ".other");
        assert_eq!(join_module_path("crate", "util", "rust"), "crate::util");
    }

    #[test]
    fn type_names_in_generic() {
        assert_eq!(
            type_names_in("HashMap<String, Vec<crate::User>>"),
            vec!["HashMap", "String", "Vec", "User"]
        );
        assert_eq!(type_names_in("&mut Self"), Vec::<String>::new());
    }
}
