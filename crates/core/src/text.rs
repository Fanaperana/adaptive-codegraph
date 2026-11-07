//! # Text index
//!
//! A trigram index over every text file in the workspace: code, config,
//! docs and files in languages the graph does not parse. A literal or regex
//! search first narrows the files to those that contain every trigram the
//! pattern requires, then runs the regex on those files only. Contents are
//! held in memory, so a warm search reads no files.
//!
//! - Trigrams are built from ASCII-lowercased text, so one index serves both
//!   case-sensitive and case-insensitive searches.
//! - The index is refreshed before each search: files whose mtime or size
//!   changed are read again, so results always match the files on disk.
//! - Files larger than [`MAX_IN_MEMORY`] are not held in memory: they are
//!   read from disk and scanned on every search, so no file is skipped.

use crate::config::Config;
use crate::index::walk_files;
use rayon::prelude::*;
use regex_syntax::hir::{Class, Hir, HirKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const VERSION: u32 = 3;
/// Larger files are searched from disk instead of memory.
pub const MAX_IN_MEMORY: u64 = 4_000_000;
/// Longest line text kept in a hit.
const MAX_LINE_CHARS: usize = 240;

#[derive(Serialize, Deserialize, Clone)]
struct TextFile {
    path: String,
    mtime: u128,
    size: u64,
    /// Binary files keep no content and are never searched.
    binary: bool,
    /// Too large to keep in memory: read from disk at search time.
    large: bool,
    content: String,
    /// Sorted, deduplicated trigrams of the lowercased content.
    tris: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    files: Vec<TextFile>,
}

/// What to search for.
#[derive(Clone, Debug, Default)]
pub struct TextQuery {
    pub pattern: String,
    /// Treat `pattern` as a literal string, not a regex.
    pub fixed: bool,
    pub ignore_case: bool,
    /// Match whole words only.
    pub word: bool,
    /// Only files under this path prefix, or matching this glob (`*` in it).
    pub path: Option<String>,
}

/// One matching line.
#[derive(Clone, Debug, Serialize)]
pub struct TextHit {
    pub file: String,
    /// 1-based.
    pub line: u32,
    /// 1-based byte column of the first match on the line.
    pub column: u32,
    pub text: String,
}

/// Search outcome.
#[derive(Debug, Default)]
pub struct TextResult {
    pub hits: Vec<TextHit>,
    /// Files searched after trigram filtering, and in total.
    pub candidates: usize,
    pub files: usize,
}

#[derive(Default)]
pub struct TextIndex {
    files: Vec<TextFile>,
    postings: HashMap<u32, Vec<u32>>,
    /// Workspace root, for reading large files (set by `refresh`).
    base: PathBuf,
    /// 0 means [`MAX_IN_MEMORY`].
    max_in_memory: u64,
}

impl TextIndex {
    fn file_path(index_dir: &Path) -> PathBuf {
        index_dir.join("text.bin")
    }

    /// Load the saved index, or start empty.
    pub fn load(index_dir: &Path) -> Self {
        let saved = std::fs::read(Self::file_path(index_dir))
            .ok()
            .and_then(|b| bincode::deserialize::<Saved>(&b).ok())
            .filter(|s| s.version == VERSION);
        let mut t = Self {
            files: saved.map(|s| s.files).unwrap_or_default(),
            ..Default::default()
        };
        t.rebuild_postings();
        t
    }

    pub fn file_count(&self) -> usize {
        self.files.iter().filter(|f| !f.binary).count()
    }

    /// Bring the index in line with the files on disk. Returns how many
    /// files were read again or removed (0 when nothing changed).
    pub fn refresh(&mut self, config: &Config, base: &Path, index_dir: &Path) -> anyhow::Result<usize> {
        // Search what ripgrep would: .gitignore decides, not the graph's
        // default excludes (`build/` is source code in many projects).
        // Excludes the user added to the config still apply.
        let defaults = Config::default().exclude;
        let user: Vec<String> = config.exclude.iter().filter(|e| !defaults.contains(e)).cloned().collect();
        self.base = base.to_path_buf();
        let max = if self.max_in_memory == 0 { MAX_IN_MEMORY } else { self.max_in_memory };
        let disk = walk_files(config, base, &user, u64::MAX, &|_| true)?;
        let mut old: HashMap<String, TextFile> = std::mem::take(&mut self.files)
            .into_iter()
            .map(|f| (f.path.clone(), f))
            .collect();
        let mut slots: Vec<Option<TextFile>> = Vec::with_capacity(disk.len());
        let mut to_read = Vec::new();
        for (i, d) in disk.iter().enumerate() {
            match old.remove(&d.rel) {
                Some(f) if f.mtime == d.mtime && f.size == d.size => slots.push(Some(f)),
                _ => {
                    slots.push(None);
                    to_read.push(i);
                }
            }
        }
        // Whatever is left in `old` is no longer on disk.
        let changed = to_read.len() + old.len();
        let read: Vec<(usize, TextFile)> = to_read
            .par_iter()
            .map(|&i| {
                let d = &disk[i];
                let large = d.size > max;
                let bytes = if large { read_head(&d.abs, 8192) } else { std::fs::read(&d.abs).unwrap_or_default() };
                let binary = is_binary(&bytes);
                let (content, tris) = if binary || large {
                    (String::new(), Vec::new())
                } else {
                    let text = decode(&bytes);
                    (text.clone(), trigrams(text.as_bytes()))
                };
                (i, TextFile { path: d.rel.clone(), mtime: d.mtime, size: d.size, binary, large, content, tris })
            })
            .collect();
        for (i, f) in read {
            slots[i] = Some(f);
        }
        self.files = slots.into_iter().flatten().collect();
        if changed > 0 || self.postings.is_empty() {
            self.rebuild_postings();
        }
        if changed > 0 {
            std::fs::create_dir_all(index_dir)?;
            let saved = Saved { version: VERSION, files: self.files.clone() };
            let tmp = index_dir.join("text.bin.tmp");
            std::fs::write(&tmp, bincode::serialize(&saved)?)?;
            std::fs::rename(&tmp, Self::file_path(index_dir))?;
        }
        Ok(changed)
    }

    fn rebuild_postings(&mut self) {
        let mut p: HashMap<u32, Vec<u32>> = HashMap::new();
        for (i, f) in self.files.iter().enumerate() {
            for &t in &f.tris {
                p.entry(t).or_default().push(i as u32);
            }
        }
        self.postings = p;
    }

    /// Run a search. Hits are sorted by file, then line.
    pub fn search(&self, q: &TextQuery) -> anyhow::Result<TextResult> {
        let mut pat = if q.fixed { regex::escape(&q.pattern) } else { q.pattern.clone() };
        if q.word {
            pat = format!(r"\b(?:{pat})\b");
        }
        let re = regex::RegexBuilder::new(&pat)
            .case_insensitive(q.ignore_case)
            .multi_line(true)
            .build()?;
        let hir = regex_syntax::ParserBuilder::new()
            .case_insensitive(q.ignore_case)
            .multi_line(true)
            .build()
            .parse(&pat)?;
        let mut lits = Vec::new();
        required_literals(&hir, &mut lits);

        let path_filter = PathFilter::new(q.path.as_deref())?;
        let searchable: Vec<u32> = (0..self.files.len() as u32)
            .filter(|&i| {
                let f = &self.files[i as usize];
                !f.binary && path_filter.matches(&f.path)
            })
            .collect();
        let candidates = self.candidates(&lits, &searchable);

        let mut per_file: Vec<(u32, Vec<TextHit>)> = candidates
            .par_iter()
            .filter_map(|&i| {
                let f = &self.files[i as usize];
                let hits = if f.large {
                    let bytes = std::fs::read(self.base.join(&f.path)).ok()?;
                    match_lines(&re, &f.path, &decode(&bytes))
                } else {
                    match_lines(&re, &f.path, &f.content)
                };
                (!hits.is_empty()).then_some((i, hits))
            })
            .collect();
        per_file.sort_by_key(|(i, _)| *i);
        Ok(TextResult {
            hits: per_file.into_iter().flat_map(|(_, h)| h).collect(),
            candidates: candidates.len(),
            files: searchable.len(),
        })
    }

    /// Files that contain every trigram of every required literal, plus
    /// large files (not in the trigram index, always scanned).
    fn candidates(&self, lits: &[Vec<u8>], searchable: &[u32]) -> Vec<u32> {
        let mut out = self.indexed_candidates(lits, searchable);
        out.extend(searchable.iter().copied().filter(|&i| self.files[i as usize].large));
        out.sort_unstable();
        out.dedup();
        out
    }

    fn indexed_candidates(&self, lits: &[Vec<u8>], searchable: &[u32]) -> Vec<u32> {
        let mut tris: Vec<u32> = lits.iter().flat_map(|l| trigrams(l)).collect();
        tris.sort_unstable();
        tris.dedup();
        if tris.is_empty() {
            return searchable.iter().copied().filter(|&i| !self.files[i as usize].large).collect();
        }
        let mut lists: Vec<&Vec<u32>> = Vec::with_capacity(tris.len());
        for t in &tris {
            match self.postings.get(t) {
                Some(l) => lists.push(l),
                None => return Vec::new(),
            }
        }
        lists.sort_by_key(|l| l.len());
        let mut cur: Vec<u32> = lists[0].clone();
        for l in &lists[1..] {
            cur.retain(|x| l.binary_search(x).is_ok());
            if cur.is_empty() {
                break;
            }
        }
        cur.retain(|x| searchable.binary_search(x).is_ok());
        cur
    }
}

/// Each matching line once, with the column of its first match.
fn match_lines(re: &regex::Regex, path: &str, content: &str) -> Vec<TextHit> {
    let mut out = Vec::new();
    let bytes = content.as_bytes();
    let mut line_no = 0u32;
    let mut scanned = 0usize; // bytes counted for line_no
    let mut last_line_end = 0usize; // end of the last reported line
    for m in re.find_iter(content) {
        if m.start() < last_line_end && !out.is_empty() {
            continue; // same line as the previous hit
        }
        // The empty position after a final newline is not a line (as in ripgrep).
        if m.start() == bytes.len() && (bytes.is_empty() || bytes.ends_with(b"\n")) {
            break;
        }
        line_no += bytes[scanned..m.start()].iter().filter(|&&b| b == b'\n').count() as u32;
        scanned = m.start();
        let start = bytes[..m.start()].iter().rposition(|&b| b == b'\n').map(|p| p + 1).unwrap_or(0);
        let end = bytes[m.start()..].iter().position(|&b| b == b'\n').map(|p| m.start() + p).unwrap_or(bytes.len());
        last_line_end = end + 1;
        let line = content[start..end].trim_end_matches('\r');
        let text: String = if line.chars().count() > MAX_LINE_CHARS {
            let mut t: String = line.chars().take(MAX_LINE_CHARS).collect();
            t.push('…');
            t
        } else {
            line.to_string()
        };
        out.push(TextHit { file: path.to_string(), line: line_no + 1, column: (m.start() - start + 1) as u32, text });
    }
    out
}

struct PathFilter {
    prefix: Option<String>,
    glob: Option<globset::GlobMatcher>,
}

impl PathFilter {
    fn new(p: Option<&str>) -> anyhow::Result<Self> {
        let Some(p) = p.map(|p| p.trim().replace('\\', "/")).filter(|p| !p.is_empty()) else {
            return Ok(Self { prefix: None, glob: None });
        };
        if p.contains(['*', '?', '[', '{']) {
            // `*.rs` should match in any folder, like ripgrep's -g.
            let g = if p.contains('/') { p } else { format!("**/{p}") };
            Ok(Self { prefix: None, glob: Some(globset::Glob::new(&g)?.compile_matcher()) })
        } else {
            Ok(Self { prefix: Some(p.trim_start_matches("./").to_string()), glob: None })
        }
    }

    fn matches(&self, path: &str) -> bool {
        if let Some(p) = &self.prefix {
            return path.starts_with(p.as_str());
        }
        if let Some(g) = &self.glob {
            return g.is_match(path);
        }
        true
    }
}

/// UTF-16 text starts with a byte-order mark; anything else with a NUL byte
/// in its first 8 KB is binary (ripgrep's rule).
fn is_binary(bytes: &[u8]) -> bool {
    !bytes.starts_with(&[0xFF, 0xFE]) && !bytes.starts_with(&[0xFE, 0xFF]) && bytes.iter().take(8192).any(|&b| b == 0)
}

/// File bytes as text: a UTF-8 byte-order mark is dropped, UTF-16 (with a
/// byte-order mark) is converted, invalid UTF-8 is replaced.
fn decode(bytes: &[u8]) -> String {
    let utf16 = |be: bool| {
        let units = bytes[2..].chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) });
        char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')).collect()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        utf16(false)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        utf16(true)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The first `n` bytes of a file (for binary detection of large files).
fn read_head(path: &Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(k) => {
            buf.truncate(k);
            buf
        }
        Err(_) => Vec::new(),
    }
}

fn lower(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

/// Sorted, deduplicated trigrams of ASCII-lowercased bytes.
fn trigrams(bytes: &[u8]) -> Vec<u32> {
    if bytes.len() < 3 {
        return Vec::new();
    }
    let mut v: Vec<u32> = bytes
        .windows(3)
        .map(|w| (lower(w[0]) as u32) << 16 | (lower(w[1]) as u32) << 8 | lower(w[2]) as u32)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Literal byte strings (lowercased) that every match must contain.
/// Only runs of 3+ bytes are useful for the trigram filter.
fn required_literals(h: &Hir, out: &mut Vec<Vec<u8>>) {
    let mut run = Vec::new();
    walk(h, &mut run, out);
    flush(&mut run, out);
}

fn flush(run: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
    if run.len() >= 3 {
        out.push(std::mem::take(run));
    } else {
        run.clear();
    }
}

fn walk(h: &Hir, run: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
    match h.kind() {
        HirKind::Literal(l) => run.extend(l.0.iter().map(|&b| lower(b))),
        HirKind::Class(c) => match single_byte(c) {
            Some(b) => run.push(b),
            None => flush(run, out),
        },
        HirKind::Concat(xs) => {
            for x in xs {
                walk(x, run, out);
            }
        }
        HirKind::Capture(c) => walk(&c.sub, run, out),
        HirKind::Repetition(r) => {
            flush(run, out);
            if r.min >= 1 {
                let mut inner = Vec::new();
                walk(&r.sub, &mut inner, out);
                flush(&mut inner, out);
            }
        }
        // Zero-width assertions do not break a literal run.
        HirKind::Look(_) | HirKind::Empty => {}
        HirKind::Alternation(_) => flush(run, out),
    }
}

/// A class that only matches the case variants of one ASCII byte (as
/// `(?i)a` becomes `[Aa]`) acts as that lowercase byte.
fn single_byte(c: &Class) -> Option<u8> {
    let mut found: Option<u8> = None;
    let mut push = |cp: u32| -> bool {
        if cp > 0x7f {
            // Non-ASCII case variants (e.g. K and the Kelvin sign) map to the
            // ASCII letter's own class: ignore them, the letter decides.
            return matches!(cp, 0x212a | 0x17f);
        }
        let b = lower(cp as u8);
        match found {
            None => {
                found = Some(b);
                true
            }
            Some(x) => x == b,
        }
    };
    match c {
        Class::Unicode(u) => {
            let mut n = 0u32;
            for r in u.ranges() {
                n += r.end() as u32 - r.start() as u32 + 1;
                if n > 4 {
                    return None;
                }
                for cp in r.start() as u32..=r.end() as u32 {
                    if !push(cp) {
                        return None;
                    }
                }
            }
        }
        Class::Bytes(b) => {
            let mut n = 0u32;
            for r in b.ranges() {
                n += r.end() as u32 - r.start() as u32 + 1;
                if n > 2 {
                    return None;
                }
                for cp in r.start() as u32..=r.end() as u32 {
                    if !push(cp) {
                        return None;
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lits(p: &str, ci: bool) -> Vec<String> {
        let hir = regex_syntax::ParserBuilder::new().case_insensitive(ci).build().parse(p).unwrap();
        let mut out = Vec::new();
        required_literals(&hir, &mut out);
        out.into_iter().map(|b| String::from_utf8(b).unwrap()).collect()
    }

    #[test]
    fn literals_from_regex() {
        assert_eq!(lits("foo_bar", false), vec!["foo_bar"]);
        assert_eq!(lits(r"fn\s+parse_\w+\(", false), vec!["parse_"]);
        assert_eq!(lits("Store", true), vec!["store"]);
        assert_eq!(lits("ab(cd|ef)gh", false), Vec::<String>::new());
        assert_eq!(lits(r"\bindex_dir\b", false), vec!["index_dir"]);
        assert_eq!(lits("(?:hello)+ world", false), vec!["hello", " world"]);
    }

    fn index(files: &[(&str, &str)]) -> TextIndex {
        let mut t = TextIndex::default();
        for (p, c) in files {
            t.files.push(TextFile {
                path: p.to_string(),
                mtime: 0,
                size: c.len() as u64,
                binary: false,
                large: false,
                content: c.to_string(),
                tris: trigrams(c.as_bytes()),
            });
        }
        t.rebuild_postings();
        t
    }

    #[test]
    fn search_filters_and_reports_lines() {
        let t = index(&[
            ("a.rs", "fn main() {\n    let x = load_config();\n}\n"),
            ("b.toml", "[server]\nport = 8080\n"),
            ("c.md", "Call LOAD_CONFIG first.\n"),
        ]);
        let r = t.search(&TextQuery { pattern: "load_config".into(), fixed: true, ..Default::default() }).unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!((r.hits[0].file.as_str(), r.hits[0].line, r.hits[0].column), ("a.rs", 2, 13));
        // The trigram filter must keep c.md: trigrams are case-folded.
        assert_eq!(r.candidates, 2);

        let r = t.search(&TextQuery { pattern: "load_config".into(), ignore_case: true, ..Default::default() }).unwrap();
        assert_eq!(r.hits.len(), 2);

        let r = t.search(&TextQuery { pattern: r"port\s*=\s*\d+".into(), ..Default::default() }).unwrap();
        assert_eq!(r.hits[0].file, "b.toml");

        let r = t.search(&TextQuery { pattern: "config".into(), ignore_case: true, path: Some("*.md".into()), ..Default::default() }).unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].file, "c.md");
    }

    #[test]
    fn word_and_multiple_matches_per_line() {
        let t = index(&[("a.py", "run(run_all(run))\nrerun()\n")]);
        let r = t.search(&TextQuery { pattern: "run".into(), word: true, ..Default::default() }).unwrap();
        assert_eq!(r.hits.len(), 1, "one hit per line");
        assert_eq!(r.hits[0].line, 1);
    }

    #[test]
    fn refresh_tracks_disk() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("notes.txt"), "alpha beta\n").unwrap();
        std::fs::write(base.join("img.bin"), [0u8, 1, 2, 3]).unwrap();
        let cfg = Config::default();
        let idx_dir = base.join(".adaptive-codegraph");
        let mut t = TextIndex::load(&idx_dir);
        assert_eq!(t.refresh(&cfg, base, &idx_dir).unwrap(), 2);
        assert_eq!(t.refresh(&cfg, base, &idx_dir).unwrap(), 0, "no change, no work");
        assert_eq!(t.file_count(), 1, "binary file is not searchable");

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(base.join("notes.txt"), "alpha gamma delta\n").unwrap();
        assert!(t.refresh(&cfg, base, &idx_dir).unwrap() >= 1);
        let q = TextQuery { pattern: "gamma".into(), ..Default::default() };
        assert_eq!(t.search(&q).unwrap().hits.len(), 1);

        // Reloaded from disk, the index gives the same answer.
        let t2 = TextIndex::load(&idx_dir);
        assert_eq!(t2.search(&q).unwrap().hits.len(), 1);
    }

    #[test]
    fn encodings_and_line_ends_match_ripgrep() {
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(b"import os\nimport sys\n");
        let mut utf16 = vec![0xFF, 0xFE];
        for u in "utf16 target\n".encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        assert!(!is_binary(&utf16));
        assert_eq!(decode(&utf16), "utf16 target\n");
        let t = index(&[("bom.py", &decode(&bom)), ("e.txt", "a\n\n\nb\n")]);
        let q = |p: &str| TextQuery { pattern: p.into(), ..Default::default() };
        assert_eq!(t.search(&q("^import")).unwrap().hits.len(), 2, "BOM does not hide line 1");
        let lines: Vec<u32> = t.search(&q("^$")).unwrap().hits.iter().filter(|h| h.file == "e.txt").map(|h| h.line).collect();
        assert_eq!(lines, [2, 3], "no phantom line after the final newline");
    }

    #[test]
    fn large_files_are_searched_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let big = format!("{}needle here\n", "filler line\n".repeat(50));
        std::fs::write(base.join("big.txt"), &big).unwrap();
        std::fs::write(base.join("small.txt"), "needle too\n").unwrap();
        let idx_dir = base.join(".adaptive-codegraph");
        let mut t = TextIndex { max_in_memory: 100, ..Default::default() };
        t.refresh(&Config::default(), base, &idx_dir).unwrap();
        let r = t.search(&TextQuery { pattern: "needle".into(), ..Default::default() }).unwrap();
        let files: Vec<&str> = r.hits.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(files, ["big.txt", "small.txt"]);
        assert_eq!(r.hits[0].line, 51);
        // A pattern with no literal still reaches the large file.
        let r = t.search(&TextQuery { pattern: r"\w{3}dl\w".into(), ..Default::default() }).unwrap();
        assert_eq!(r.hits.len(), 2);
    }
}
