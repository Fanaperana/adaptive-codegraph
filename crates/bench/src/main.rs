//! # acg-bench
//!
//! Measures how trustworthy the code graph is:
//!
//! 1. **Call-graph accuracy** on hand-labelled fixture projects
//!    (`bench/fixtures/<lang>/`). Each definition carries a `@def <id>`
//!    comment and `expected.txt` lists the true `caller -> callee` edges.
//! 2. **Incremental consistency**: index, edit a file, reindex incrementally,
//!    and compare against a fresh full index of the edited tree.
//! 3. **Scale stats** on real repositories passed with `--repo` (indexed into
//!    a temp directory, so the repo itself is never written to).

mod compat;

use anyhow::{Context, Result};
use clap::Parser;
use compat::{Edge, Sym};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "acg-bench", about = "Benchmark adaptive-codegraph accuracy and scale")]
struct Args {
    /// Directory holding fixture projects (one per sub-directory).
    #[arg(long, default_value = "bench/fixtures")]
    fixtures: PathBuf,
    /// Real repositories to index for scale stats.
    #[arg(long)]
    repo: Vec<PathBuf>,
    /// Label for this run (used in the output file name).
    #[arg(long, default_value = "dev")]
    label: String,
    /// Where to write the JSON report.
    #[arg(long, default_value = "bench/results")]
    out: PathBuf,
    /// Print every false positive / false negative.
    #[arg(long)]
    verbose: bool,
    /// Only run fixtures (skip --repo even if given).
    #[arg(long)]
    fixtures_only: bool,
}

#[derive(Serialize, Default, Clone)]
struct Accuracy {
    tp: usize,
    fp: usize,
    fn_: usize,
    precision: f64,
    recall: f64,
    f1: f64,
}

impl Accuracy {
    fn finish(mut self) -> Self {
        let p = self.tp as f64 / (self.tp + self.fp).max(1) as f64;
        let r = self.tp as f64 / (self.tp + self.fn_).max(1) as f64;
        self.precision = p;
        self.recall = r;
        self.f1 = if p + r > 0.0 { 2.0 * p * r / (p + r) } else { 0.0 };
        self
    }
}

#[derive(Serialize)]
struct FixtureReport {
    name: String,
    /// Accuracy per minimum confidence level ("1", "2", "3").
    accuracy: BTreeMap<String, Accuracy>,
    false_positives: Vec<String>,
    false_negatives: Vec<String>,
    missing_symbols: Vec<String>,
    duplicate_symbols: usize,
    incremental_missing_edges: usize,
    incremental_extra_edges: usize,
    incremental_missing_symbols: usize,
    incremental_extra_symbols: usize,
}

#[derive(Serialize)]
struct RepoReport {
    repo: String,
    index_secs: f64,
    files: usize,
    symbols: usize,
    duplicate_symbol_ids: usize,
    call_edges: usize,
    call_edges_by_conf: BTreeMap<u8, usize>,
    duplicate_edges: usize,
    calls_into_non_callable: usize,
    calls_from_containers: usize,
    absolute_paths: bool,
    graph_bytes: u64,
    avg_neighborhood_bytes: usize,
    query_ms_avg: f64,
}

#[derive(Serialize)]
struct Report {
    label: String,
    fixtures: Vec<FixtureReport>,
    overall: BTreeMap<String, Accuracy>,
    repos: Vec<RepoReport>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Always use this workspace's language definitions.
    let langs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../languages");
    std::env::set_var("ADAPTIVE_CODEGRAPH_LANGUAGES", langs.canonicalize()?);
    std::env::set_var("RUST_LOG", "error");

    let mut fixtures = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&args.fixtures)
        .with_context(|| format!("read {}", args.fixtures.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("expected.txt").exists())
        .collect();
    dirs.sort();
    for dir in &dirs {
        fixtures.push(run_fixture(dir, args.verbose)?);
    }

    // Totals per group: "design" fixtures were written together with the
    // resolver; "holdout" fixtures were written afterwards and test
    // generalisation.
    let mut overall: BTreeMap<String, Accuracy> = BTreeMap::new();
    for f in &fixtures {
        let group = if f.name.ends_with("-holdout") { "holdout" } else { "design" };
        for (k, a) in &f.accuracy {
            for key in [format!("{group}@{k}"), format!("all@{k}")] {
                let o = overall.entry(key).or_default();
                o.tp += a.tp;
                o.fp += a.fp;
                o.fn_ += a.fn_;
            }
        }
    }
    let overall: BTreeMap<String, Accuracy> =
        overall.into_iter().map(|(k, a)| (k, a.finish())).collect();

    let mut repos = Vec::new();
    if !args.fixtures_only {
        for repo in &args.repo {
            eprintln!("indexing {} ...", repo.display());
            repos.push(run_repo(repo)?);
        }
    }

    let report = Report {
        label: args.label.clone(),
        fixtures,
        overall,
        repos,
    };
    print_markdown(&report);

    std::fs::create_dir_all(&args.out)?;
    let out = args.out.join(format!("{}.json", args.label));
    std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
    eprintln!("\nwrote {}", out.display());
    Ok(())
}

// ── Fixtures ────────────────────────────────────────────────────────────

struct Anchor {
    id: String,
    file: String,
    line: u32,
    name: String,
}

fn load_anchors(root: &Path) -> Result<Vec<Anchor>> {
    let mut out = Vec::new();
    for file in walk(root)? {
        let rel = rel_path(root, &file);
        if rel.ends_with(".txt") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            if let Some(pos) = line.find("@def ") {
                let id = line[pos + 5..].split_whitespace().next().unwrap_or("").to_string();
                let name = id.rsplit("::").next().unwrap_or(&id).to_string();
                out.push(Anchor {
                    id,
                    file: rel.clone(),
                    line: i as u32,
                    name,
                });
            }
        }
    }
    Ok(out)
}

fn load_expected(dir: &Path, anchors: &[Anchor]) -> Result<BTreeSet<(String, String)>> {
    let text = std::fs::read_to_string(dir.join("expected.txt"))?;
    let ids: BTreeSet<&str> = anchors.iter().map(|a| a.id.as_str()).collect();
    let mut set = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (a, b) = line
            .split_once("->")
            .with_context(|| format!("bad expected line: {line}"))?;
        let (a, b) = (a.trim().to_string(), b.trim().to_string());
        for x in [&a, &b] {
            anyhow::ensure!(ids.contains(x.as_str()), "{}: unknown anchor '{x}'", dir.display());
        }
        set.insert((a, b));
    }
    Ok(set)
}

/// Map each graph symbol to the anchor it represents (same file, same name,
/// span covering the anchor line).
fn anchor_of<'a>(sym: &Sym, anchors: &'a [Anchor]) -> Option<&'a Anchor> {
    anchors
        .iter()
        .filter(|a| a.file == sym.file && a.name == sym.name)
        .filter(|a| sym.start_line <= a.line && a.line <= sym.end_line)
        .min_by_key(|a| a.line - sym.start_line)
}

fn run_fixture(dir: &Path, verbose: bool) -> Result<FixtureReport> {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let tmp = tempfile::tempdir()?;
    let work = tmp.path().join("src");
    copy_dir(dir, &work)?;
    let work = work.canonicalize()?;
    let idx = tmp.path().join("idx");

    let anchors = load_anchors(&work)?;
    let expected = load_expected(&work, &anchors)?;

    let graph = compat::full_index(&work, &idx)?;
    let syms: HashMap<_, _> = graph.symbols.iter().map(|s| (s.id, s)).collect();

    let missing_symbols: Vec<String> = anchors
        .iter()
        .filter(|a| !graph.symbols.iter().any(|s| anchor_of(s, &anchors).map(|x| x.id == a.id).unwrap_or(false)))
        .map(|a| a.id.clone())
        .collect();
    let distinct: std::collections::HashSet<_> = graph.symbols.iter().map(|s| s.id).collect();
    let duplicate_symbols = graph.symbols.len() - distinct.len();

    let mut accuracy = BTreeMap::new();
    let mut fps_report = Vec::new();
    let mut fns_report = Vec::new();
    for min_conf in [1u8, 2, 3] {
        let mut actual: BTreeSet<(String, String)> = BTreeSet::new();
        let mut unmapped: BTreeSet<String> = BTreeSet::new();
        for e in graph.edges.iter().filter(|e| e.kind == "calls" && e.conf >= min_conf) {
            let (Some(f), Some(t)) = (syms.get(&e.from), syms.get(&e.to)) else { continue };
            if f.kind == "file" {
                continue; // module-level code: not scored
            }
            match (anchor_of(f, &anchors), anchor_of(t, &anchors)) {
                (Some(a), Some(b)) => {
                    actual.insert((a.id.clone(), b.id.clone()));
                }
                _ => {
                    unmapped.insert(format!(
                        "{} ({}) -> {} ({})",
                        f.name, f.kind, t.name, t.kind
                    ));
                }
            }
        }
        let tp = actual.intersection(&expected).count();
        let fp_set: Vec<String> = actual
            .difference(&expected)
            .map(|(a, b)| format!("{a} -> {b}"))
            .chain(unmapped.iter().map(|u| format!("[unmapped] {u}")))
            .collect();
        let fn_set: Vec<String> = expected
            .difference(&actual)
            .map(|(a, b)| format!("{a} -> {b}"))
            .collect();
        accuracy.insert(
            min_conf.to_string(),
            Accuracy {
                tp,
                fp: fp_set.len(),
                fn_: fn_set.len(),
                ..Default::default()
            }
            .finish(),
        );
        if min_conf == 1 {
            fps_report = fp_set;
            fns_report = fn_set;
        }
    }

    // Incremental consistency
    let (inc_me, inc_xe, inc_ms, inc_xs) = incremental_check(dir)?;

    if verbose {
        eprintln!("── {name}: false positives");
        for f in &fps_report {
            eprintln!("   + {f}");
        }
        eprintln!("── {name}: false negatives");
        for f in &fns_report {
            eprintln!("   - {f}");
        }
        if !missing_symbols.is_empty() {
            eprintln!("── {name}: anchors with no symbol: {missing_symbols:?}");
        }
    }

    Ok(FixtureReport {
        name,
        accuracy,
        false_positives: fps_report,
        false_negatives: fns_report,
        missing_symbols,
        duplicate_symbols,
        incremental_missing_edges: inc_me,
        incremental_extra_edges: inc_xe,
        incremental_missing_symbols: inc_ms,
        incremental_extra_symbols: inc_xs,
    })
}

/// Returns (missing edges, extra edges, missing symbols, extra symbols) of the
/// incrementally-updated graph compared with a fresh full index.
fn incremental_check(dir: &Path) -> Result<(usize, usize, usize, usize)> {
    let edit_spec = std::fs::read_to_string(dir.join("edit.txt"))?;
    let (target, body) = edit_spec.split_once('\n').context("edit.txt needs a path line")?;

    let tmp = tempfile::tempdir()?;
    let work = tmp.path().join("src");
    copy_dir(dir, &work)?;
    let work = work.canonicalize()?;
    let idx_inc = tmp.path().join("idx_inc");
    let idx_full = tmp.path().join("idx_full");

    compat::full_index(&work, &idx_inc)?;
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let target_path = work.join(target.trim());
    let mut text = std::fs::read_to_string(&target_path)?;
    text.push('\n');
    text.push_str(body);
    std::fs::write(&target_path, text)?;

    let inc = compat::incremental_index(&work, &idx_inc)?;
    let full = compat::full_index(&work, &idx_full)?;

    let key = |g: &compat::Graph| {
        let by: HashMap<_, _> = g.symbols.iter().map(|s| (s.id, s)).collect();
        let syms: BTreeSet<String> = g
            .symbols
            .iter()
            .map(|s| format!("{}:{}:{}", s.file, s.name, s.start_line))
            .collect();
        let edges: BTreeSet<String> = g
            .edges
            .iter()
            .filter_map(|e| {
                let (f, t) = (by.get(&e.from)?, by.get(&e.to)?);
                Some(format!(
                    "{}:{}:{} -{}-> {}:{}:{}",
                    f.file, f.name, f.start_line, e.kind, t.file, t.name, t.start_line
                ))
            })
            .collect();
        (syms, edges)
    };
    let (is, ie) = key(&inc);
    let (fs, fe) = key(&full);
    Ok((
        fe.difference(&ie).count(),
        ie.difference(&fe).count(),
        fs.difference(&is).count(),
        is.difference(&fs).count(),
    ))
}

// ── Real repos ──────────────────────────────────────────────────────────

const NON_CALLABLE: &[&str] = &[
    "enum_variant", "field", "module", "impl", "variable", "constant", "static", "file",
    "decorated", "definition", "type_alias", "type", "interface", "trait", "union", "enum",
];
const CONTAINERS: &[&str] = &["module", "impl", "trait", "interface", "enum", "struct", "union"];

fn run_repo(repo: &Path) -> Result<RepoReport> {
    let repo = repo.canonicalize()?;
    let tmp = tempfile::tempdir()?;
    let idx = tmp.path().join("idx");

    let t0 = Instant::now();
    let graph = compat::full_index(&repo, &idx)?;
    let index_secs = t0.elapsed().as_secs_f64();

    let by: HashMap<_, _> = graph.symbols.iter().map(|s| (s.id, s)).collect();
    let distinct: std::collections::HashSet<_> = graph.symbols.iter().map(|s| s.id).collect();
    let calls: Vec<&Edge> = graph.edges.iter().filter(|e| e.kind == "calls").collect();
    let distinct_edges: std::collections::HashSet<_> = graph.edges.iter().map(|e| (e.from, e.to, e.kind.clone())).collect();
    let mut by_conf = BTreeMap::new();
    for e in &calls {
        *by_conf.entry(e.conf).or_insert(0) += 1;
    }
    let into_nc = calls
        .iter()
        .filter(|e| by.get(&e.to).map(|t| NON_CALLABLE.contains(&t.kind.as_str())).unwrap_or(false))
        .count();
    let from_cont = calls
        .iter()
        .filter(|e| by.get(&e.from).map(|t| CONTAINERS.contains(&t.kind.as_str())).unwrap_or(false))
        .count();
    let files: BTreeSet<&str> = graph.symbols.iter().map(|s| s.file.as_str()).collect();
    let absolute_paths = graph.symbols.iter().any(|s| s.raw_file.contains(":\\") || s.raw_file.starts_with('/'));

    let graph_bytes = std::fs::metadata(idx.join("graph.bin")).map(|m| m.len()).unwrap_or(0);
    let (avg_neighborhood_bytes, query_ms_avg) = compat::neighborhood_cost(&idx, 20)?;

    Ok(RepoReport {
        repo: repo.file_name().unwrap().to_string_lossy().to_string(),
        index_secs,
        files: files.len(),
        symbols: graph.symbols.len(),
        duplicate_symbol_ids: graph.symbols.len() - distinct.len(),
        call_edges: calls.len(),
        call_edges_by_conf: by_conf,
        duplicate_edges: graph.edges.len() - distinct_edges.len(),
        calls_into_non_callable: into_nc,
        calls_from_containers: from_cont,
        absolute_paths,
        graph_bytes,
        avg_neighborhood_bytes,
        query_ms_avg,
    })
}

// ── Output ──────────────────────────────────────────────────────────────

fn print_markdown(r: &Report) {
    println!("# Benchmark: {}\n", r.label);
    println!("## Call-graph accuracy (fixtures, min confidence 1 / 2 / 3)\n");
    println!("| fixture | P@1 | R@1 | F1@1 | P@2 | R@2 | F1@2 | P@3 | R@3 | dup syms | incr Δedges | incr Δsyms |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    let row = |name: &str, acc: &BTreeMap<String, Accuracy>, extra: String| {
        let g = |k: &str| acc.get(k).cloned().unwrap_or_default();
        let (a, b, c) = (g("1"), g("2"), g("3"));
        println!(
            "| {name} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {extra} |",
            a.precision, a.recall, a.f1, b.precision, b.recall, b.f1, c.precision, c.recall
        );
    };
    for f in &r.fixtures {
        row(
            &f.name,
            &f.accuracy,
            format!(
                "{} | -{} +{} | -{} +{}",
                f.duplicate_symbols,
                f.incremental_missing_edges,
                f.incremental_extra_edges,
                f.incremental_missing_symbols,
                f.incremental_extra_symbols
            ),
        );
    }
    for group in ["design", "holdout", "all"] {
        let acc: BTreeMap<String, Accuracy> = r
            .overall
            .iter()
            .filter_map(|(k, a)| k.strip_prefix(&format!("{group}@")).map(|c| (c.to_string(), a.clone())))
            .collect();
        if !acc.is_empty() {
            row(&format!("**{group}**"), &acc, " | | ".to_string());
        }
    }
    for group in ["design", "holdout", "all"] {
        if let Some(a) = r.overall.get(&format!("{group}@1")) {
            println!("\n{group} @1: TP {} FP {} FN {}", a.tp, a.fp, a.fn_);
        }
    }

    if !r.repos.is_empty() {
        println!("\n## Real repositories\n");
        println!("| repo | files | symbols | dup ids | call edges | dup edges | →non-callable | from container | abs paths | index s | graph MB | nbhd bytes | query ms |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for x in &r.repos {
            println!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} | {:.1} | {} | {:.2} |",
                x.repo,
                x.files,
                x.symbols,
                x.duplicate_symbol_ids,
                x.call_edges,
                x.duplicate_edges,
                x.calls_into_non_callable,
                x.calls_from_containers,
                x.absolute_paths,
                x.index_secs,
                x.graph_bytes as f64 / 1e6,
                x.avg_neighborhood_bytes,
                x.query_ms_avg
            );
        }
    }
}

// ── fs helpers ──────────────────────────────────────────────────────────

fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for f in walk(src)? {
        let rel = f.strip_prefix(src)?;
        let to = dst.join(rel);
        std::fs::create_dir_all(to.parent().unwrap())?;
        std::fs::copy(&f, &to)?;
    }
    Ok(())
}

pub fn rel_path(root: &Path, file: &Path) -> String {
    norm(&file.strip_prefix(root).unwrap_or(file).to_string_lossy())
}

pub fn norm(s: &str) -> String {
    s.replace('\\', "/")
}
