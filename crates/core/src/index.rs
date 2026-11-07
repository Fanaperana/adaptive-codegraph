//! # Indexing Pipeline
//!
//! Walks the workspace, re-parses files that changed (by mtime/size), resolves
//! the whole project, and updates the search + vector indexes.
//!
//! Full and incremental indexing share one code path: a full index is an
//! update with no previous state. Because resolution always covers every
//! file, an incremental update produces exactly the same graph as a full one.

use crate::config::Config;
use crate::embed::{self, VectorIndex};
use crate::extract::plugin::PluginRegistry;
use crate::extract::ExtractorRegistry;
use crate::model::{FileFacts, SymbolId};
use crate::search::SearchIndex;
use crate::store::{Store, FORMAT_VERSION};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Index state saved between runs.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct IndexState {
    pub git_head: Option<String>,
    pub indexed_at: Option<u64>,
    pub file_count: usize,
    #[serde(default)]
    pub format_version: u32,
}

impl IndexState {
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}

/// What an index update did.
#[derive(Debug, Default, Clone)]
pub struct UpdateStats {
    pub files_total: usize,
    pub files_parsed: usize,
    pub files_deleted: usize,
    pub parse_failures: usize,
    pub symbols_added: usize,
    pub symbols_removed: usize,
}

/// Full index build.
pub fn full_index(
    config: &Config,
    registry: &ExtractorRegistry,
    plugins: &PluginRegistry,
) -> anyhow::Result<(Store, SearchIndex, VectorIndex)> {
    let (store, search, vectors, _) = update_index(config, registry, plugins, None)?;
    Ok((store, search, vectors))
}

/// A file found on disk.
struct DiskFile {
    abs: PathBuf,
    rel: String,
    mtime: u128,
    size: u64,
}

/// Bring the index up to date. With `prev = None` everything is rebuilt.
pub fn update_index(
    config: &Config,
    registry: &ExtractorRegistry,
    plugins: &PluginRegistry,
    prev: Option<Store>,
) -> anyhow::Result<(Store, SearchIndex, VectorIndex, UpdateStats)> {
    let base = config.base.as_deref().unwrap_or(Path::new("."));
    let index_dir = base.join(&config.index_dir);
    std::fs::create_dir_all(&index_dir)?;

    let full = prev.is_none();
    let mut facts: BTreeMap<String, FileFacts> = prev.map(|s| s.facts).unwrap_or_default();
    let disk = collect_files(config, registry, base)?;
    let mut stats = UpdateStats {
        files_total: disk.len(),
        ..Default::default()
    };

    // Work out what changed.
    let on_disk: HashSet<&str> = disk.iter().map(|f| f.rel.as_str()).collect();
    let deleted: Vec<String> = facts
        .keys()
        .filter(|k| !on_disk.contains(k.as_str()))
        .cloned()
        .collect();
    let changed: Vec<&DiskFile> = disk
        .iter()
        .filter(|f| {
            facts
                .get(&f.rel)
                .map(|old| old.mtime != f.mtime || old.size != f.size)
                .unwrap_or(true)
        })
        .collect();
    stats.files_deleted = deleted.len();
    stats.files_parsed = changed.len();

    let mut removed_ids: HashSet<SymbolId> = HashSet::new();
    for rel in deleted.iter().map(|s| s.as_str()).chain(changed.iter().map(|f| f.rel.as_str())) {
        if let Some(old) = facts.remove(rel) {
            stats.symbols_removed += old.symbols.len();
            removed_ids.extend(old.symbols.iter().map(|s| s.id));
        }
    }

    // Parse changed files in parallel.
    let t_parse = std::time::Instant::now();
    let failures = AtomicUsize::new(0);
    let parsed: Vec<(FileFacts, Vec<u8>)> = changed
        .par_iter()
        .filter_map(|f| {
            let ext = f.abs.extension()?.to_str()?;
            let extractor = registry.for_extension(ext)?;
            let content = std::fs::read(&f.abs).ok()?;
            match extractor.extract(Path::new(&f.rel), &content) {
                Ok(r) => Some((
                    FileFacts {
                        file: f.rel.clone(),
                        lang: extractor.language().to_string(),
                        mtime: f.mtime,
                        size: f.size,
                        symbols: r.symbols,
                        refs: r.refs,
                        imports: r.imports,
                        bindings: r.bindings,
                    },
                    content,
                )),
                Err(e) => {
                    tracing::warn!("Extract failed for {}: {e}", f.rel);
                    failures.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        })
        .collect();
    stats.parse_failures = failures.load(Ordering::Relaxed);
    timing("parse", t_parse);

    let new_files: Vec<String> = parsed.iter().map(|(f, _)| f.file.clone()).collect();
    for (f, _) in &parsed {
        stats.symbols_added += f.symbols.len();
    }

    // Optional regex plugins add extra references.
    let mut parsed_facts: Vec<FileFacts> = Vec::with_capacity(parsed.len());
    if !plugins.pattern_names().is_empty() {
        let mut symbol_index: HashMap<String, SymbolId> = HashMap::new();
        for f in facts.values().chain(parsed.iter().map(|(f, _)| f)) {
            for s in &f.symbols {
                symbol_index.entry(s.name.clone()).or_insert(s.id);
            }
        }
        for (mut f, content) in parsed {
            if let Ok(text) = std::str::from_utf8(&content) {
                let mut r = crate::model::ExtractionResult::new();
                plugins.apply_all(&f.file, text, &mut r, &symbol_index);
                f.refs.extend(r.refs);
            }
            parsed_facts.push(f);
        }
    } else {
        parsed_facts = parsed.into_iter().map(|(f, _)| f).collect();
    }
    for f in parsed_facts {
        facts.insert(f.file.clone(), f);
    }

    // Resolve the whole project.
    let t_resolve = std::time::Instant::now();
    let packages = detect_packages(config, base);
    let store = Store::from_facts_with_packages(facts, registry.profiles(), packages);
    timing("resolve", t_resolve);
    tracing::info!(
        "Graph: {} symbols, {} edges ({} files parsed, {} deleted)",
        store.symbol_count(),
        store.edge_count(),
        stats.files_parsed,
        stats.files_deleted
    );

    // Search index.
    let t_search = std::time::Instant::now();
    let search = SearchIndex::open(&index_dir)?;
    let mut writer = search.writer(50_000_000)?;
    if full {
        writer.delete_all_documents()?;
    } else {
        for rel in deleted.iter().chain(new_files.iter()) {
            search.remove_file(&writer, rel);
        }
    }
    for rel in &new_files {
        let Some(f) = store.facts.get(rel) else { continue };
        let content = std::fs::read(base.join(rel)).unwrap_or_default();
        for sym in &f.symbols {
            let body = if sym.kind == "file" {
                String::new()
            } else {
                let end = sym.span.end_byte.min(content.len());
                let start = sym.span.start_byte.min(end);
                crate::search::body_words(&String::from_utf8_lossy(&content[start..end]), 300)
            };
            search.index_symbol_with_body(&writer, sym, &body);
        }
    }
    writer.commit()?;
    timing("search index", t_search);

    // Vector index.
    let t_embed = std::time::Instant::now();
    let embedder = embed::create_embedder();
    let dim = embedder.dim();
    let mut vectors = if full {
        VectorIndex::new(dim)
    } else {
        VectorIndex::load(&index_dir.join("vectors.bin"))
            .ok()
            .filter(|v| v.dim() == dim)
            .unwrap_or_else(|| VectorIndex::new(dim))
    };
    vectors.remove(&removed_ids);
    let to_embed: Vec<&crate::model::Symbol> = if vectors.is_empty() {
        store.facts.values().flat_map(|f| f.symbols.iter()).collect()
    } else {
        new_files
            .iter()
            .filter_map(|r| store.facts.get(r))
            .flat_map(|f| f.symbols.iter())
            .collect()
    };
    // Hash vectors carry no meaning: only build vectors with a real model.
    let to_embed: Vec<&crate::model::Symbol> = if embedder.is_semantic() {
        to_embed.into_iter().filter(|s| embed::should_embed(s)).collect()
    } else {
        Vec::new()
    };
    if !to_embed.is_empty() {
        let texts: Vec<String> = to_embed.iter().map(|s| embed::embed_text(s)).collect();
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        match embedder.embed_batch(&refs) {
            Ok(vecs) => {
                for (sym, v) in to_embed.iter().zip(vecs) {
                    vectors.insert(sym.id, v);
                }
            }
            Err(e) => tracing::warn!("Embedding failed: {e}"),
        }
    }

    timing("embed", t_embed);

    // Persist.
    let t_save = std::time::Instant::now();
    let state = IndexState {
        git_head: detect_git_head(base),
        indexed_at: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        ),
        file_count: store.file_count(),
        format_version: FORMAT_VERSION,
    };
    state.save(&index_dir.join("state.json"))?;
    store.save(&index_dir.join("graph.bin"))?;
    vectors.save(&index_dir.join("vectors.bin"))?;
    timing("save", t_save);

    Ok((store, search, vectors, stats))
}

/// Print a phase timing when `ACG_TIMINGS` is set.
fn timing(phase: &str, t: std::time::Instant) {
    if std::env::var_os("ACG_TIMINGS").is_some() {
        eprintln!("[timing] {phase}: {:.2}s", t.elapsed().as_secs_f64());
    }
    tracing::debug!("{phase}: {:.2}s", t.elapsed().as_secs_f64());
}

/// Find package roots from manifests: `Cargo.toml` (crate names),
/// `go.mod` (module path) and `package.json` (package name).
pub fn detect_packages(config: &Config, base: &Path) -> Vec<crate::resolve::Package> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(base)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .max_depth(Some(6))
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            n != "node_modules" && n != "target" && n != "vendor"
        })
        .build();
    let index_dir = base.join(&config.index_dir);
    for entry in walker.flatten() {
        let path = entry.path();
        if path.starts_with(&index_dir) {
            continue;
        }
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !matches!(fname, "Cargo.toml" | "go.mod" | "package.json") {
            continue;
        }
        let dir = path.parent().map(|d| rel_path(base, d)).unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        match fname {
            "Cargo.toml" => {
                if let Ok(v) = text.parse::<toml::Table>() {
                    let pkg = v.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str());
                    let lib = v.get("lib").and_then(|p| p.get("name")).and_then(|n| n.as_str());
                    for name in [pkg, lib].into_iter().flatten() {
                        out.push(crate::resolve::Package {
                            name: name.replace('-', "_"),
                            dir: dir.clone(),
                        });
                    }
                }
            }
            "go.mod" => {
                if let Some(m) = text.lines().find_map(|l| l.trim().strip_prefix("module ")) {
                    out.push(crate::resolve::Package {
                        name: m.trim().to_string(),
                        dir: dir.clone(),
                    });
                }
            }
            _ => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(n) = v.get("name").and_then(|n| n.as_str()) {
                        out.push(crate::resolve::Package {
                            name: n.to_string(),
                            dir: dir.clone(),
                        });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| (&a.name, &a.dir).cmp(&(&b.name, &b.dir)));
    out.dedup();
    out
}

/// Workspace-relative path with forward slashes.
pub fn rel_path(base: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(base).unwrap_or(path);
    rel.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Collect all indexable files from configured roots.
fn collect_files(
    config: &Config,
    registry: &ExtractorRegistry,
    base: &Path,
) -> anyhow::Result<Vec<DiskFile>> {
    let mut gs = globset::GlobSetBuilder::new();
    for pat in &config.exclude {
        if let Ok(g) = globset::Glob::new(pat) {
            gs.add(g);
        }
    }
    let excludes = gs.build()?;
    let index_dir = base.join(&config.index_dir);

    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for root in &config.roots {
        let abs_root = if root.as_os_str() == "." {
            base.to_path_buf()
        } else {
            base.join(root)
        };
        let walker = ignore::WalkBuilder::new(&abs_root)
            .hidden(true)
            .git_ignore(true)
            .require_git(false)
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if path.starts_with(&index_dir) {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
            if registry.for_extension(ext).is_none() {
                continue;
            }
            let rel = rel_path(base, path);
            if excludes.is_match(&rel) || excludes.is_match(format!("/{rel}")) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() > config.max_file_bytes {
                tracing::debug!("skipping large file {rel} ({} bytes)", meta.len());
                continue;
            }
            if !seen.insert(rel.clone()) {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            files.push(DiskFile {
                abs: path.to_path_buf(),
                rel,
                mtime,
                size: meta.len(),
            });
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

/// Detect current git HEAD SHA.
fn detect_git_head(base: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(base)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
