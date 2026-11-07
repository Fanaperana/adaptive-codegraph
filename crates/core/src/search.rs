//! # Full-text Search (Tantivy BM25)
//!
//! Code-aware lexical search. Each symbol is indexed with:
//! - its name, plus the name split into words (`parseConfig` → `parse config`)
//! - its qualified name, file path and signature
//! - its doc comment
//! - the words used in its body (identifiers, comments, strings)
//!
//! Prose fields are stemmed, queries are split the same way and parsed
//! leniently (so `Store::new` or `a+b` never error), and fields are
//! weighted so a match on the name beats a match in the body.

use crate::model::{Symbol, SymbolId};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};

/// Bump when the schema changes; an old on-disk index is rebuilt.
const SCHEMA_VERSION: &str = "2";

/// Fields in the search index.
struct SearchFields {
    id: Field,
    name: Field,
    words: Field,
    fqname: Field,
    file: Field,
    file_text: Field,
    signature: Field,
    doc: Field,
    body: Field,
    kind: Field,
    lang: Field,
}

/// BM25 search index backed by Tantivy.
pub struct SearchIndex {
    index: Index,
    fields: SearchFields,
}

impl SearchIndex {
    /// Create or open a search index at the given directory.
    pub fn open(index_dir: &Path) -> anyhow::Result<Self> {
        let schema = Self::build_schema();
        let fields = Self::get_fields(&schema);

        let dir = index_dir.join("tantivy");
        let version_file = dir.join("schema_version");
        let current = std::fs::read_to_string(&version_file).unwrap_or_default();
        if dir.exists() && current.trim() != SCHEMA_VERSION {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&version_file, SCHEMA_VERSION)?;

        let index = Index::open_or_create(tantivy::directory::MmapDirectory::open(&dir)?, schema)?;
        Ok(Self { index, fields })
    }

    /// Create an in-memory index (for testing).
    pub fn in_memory() -> anyhow::Result<Self> {
        let schema = Self::build_schema();
        let fields = Self::get_fields(&schema);
        let index = Index::create_in_ram(schema);
        Ok(Self { index, fields })
    }

    fn build_schema() -> Schema {
        let stemmed = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("en_stem")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let mut builder = Schema::builder();
        builder.add_text_field("id", STRING | STORED);
        builder.add_text_field("name", TEXT | STORED);
        builder.add_text_field("words", stemmed.clone());
        builder.add_text_field("fqname", TEXT | STORED);
        builder.add_text_field("file", STRING | STORED);
        builder.add_text_field("file_text", TEXT);
        builder.add_text_field("signature", stemmed.clone());
        builder.add_text_field("doc", stemmed.clone());
        builder.add_text_field("body", stemmed);
        builder.add_text_field("kind", STRING | STORED);
        builder.add_text_field("lang", STRING | STORED);
        builder.build()
    }

    fn get_fields(schema: &Schema) -> SearchFields {
        let f = |n: &str| schema.get_field(n).unwrap();
        SearchFields {
            id: f("id"),
            name: f("name"),
            words: f("words"),
            fqname: f("fqname"),
            file: f("file"),
            file_text: f("file_text"),
            signature: f("signature"),
            doc: f("doc"),
            body: f("body"),
            kind: f("kind"),
            lang: f("lang"),
        }
    }

    /// Get a writer for batch indexing.
    pub fn writer(&self, heap_size: usize) -> anyhow::Result<IndexWriter> {
        Ok(self.index.writer(heap_size)?)
    }

    /// Index a single symbol (without body words).
    pub fn index_symbol(&self, writer: &IndexWriter, sym: &Symbol) {
        self.index_symbol_with_body(writer, sym, "");
    }

    /// Index a symbol together with the words used in its body.
    pub fn index_symbol_with_body(&self, writer: &IndexWriter, sym: &Symbol, body: &str) {
        let mut words = split_ident(&sym.name);
        if let Some(c) = &sym.container {
            words.push(' ');
            words.push_str(&split_ident(c));
        }
        let _ = writer.add_document(doc!(
            self.fields.id => sym.id.to_hex(),
            self.fields.name => sym.name.as_str(),
            self.fields.words => words,
            self.fields.fqname => sym.fqname.as_str(),
            self.fields.file => sym.file.as_str(),
            self.fields.file_text => split_ident(&sym.file),
            self.fields.signature => split_ident(sym.signature.as_deref().unwrap_or("")),
            self.fields.doc => sym.doc.as_deref().unwrap_or(""),
            self.fields.body => body,
            self.fields.kind => sym.kind.as_str(),
            self.fields.lang => sym.lang.as_str(),
        ));
    }

    /// Remove all documents for a given file path.
    pub fn remove_file(&self, writer: &IndexWriter, file_path: &str) {
        let term = tantivy::Term::from_field_text(self.fields.file, file_path);
        writer.delete_term(term);
    }

    /// Search for symbols matching a query (identifiers or natural language).
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchHit>> {
        let mut parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.name,
                self.fields.words,
                self.fields.fqname,
                self.fields.file_text,
                self.fields.signature,
                self.fields.doc,
                self.fields.body,
            ],
        );
        parser.set_field_boost(self.fields.name, 4.0);
        parser.set_field_boost(self.fields.words, 3.0);
        parser.set_field_boost(self.fields.doc, 2.0);
        parser.set_field_boost(self.fields.signature, 1.5);
        parser.set_field_boost(self.fields.fqname, 1.0);
        parser.set_field_boost(self.fields.file_text, 0.5);
        parser.set_field_boost(self.fields.body, 0.6);

        let q = prepare_query(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let (parsed, _errors) = parser.parse_query_lenient(&q);
        self.run(&*parsed, limit)
    }

    /// The original search behaviour (names/paths/signatures only, strict
    /// query syntax). Kept for benchmarking against.
    #[doc(hidden)]
    pub fn search_legacy(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchHit>> {
        let parser = QueryParser::for_index(
            &self.index,
            vec![self.fields.name, self.fields.fqname, self.fields.file_text, self.fields.signature],
        );
        let Ok(parsed) = parser.parse_query(query) else { return Ok(Vec::new()) };
        self.run(&*parsed, limit)
    }

    fn run(&self, query: &dyn tantivy::query::Query, limit: usize) -> anyhow::Result<Vec<SearchHit>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let searcher = reader.searcher();
        let top_docs = searcher.search(query, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(addr)?;
            let get = |f: Field| {
                doc.get_first(f)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            hits.push(SearchHit {
                id: SymbolId::from_hex(&get(self.fields.id)).unwrap_or_default(),
                name: get(self.fields.name),
                file: get(self.fields.file),
                kind: get(self.fields.kind),
                lang: get(self.fields.lang),
                score,
            });
        }
        Ok(hits)
    }
}

/// A search result.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: SymbolId,
    pub name: String,
    pub file: String,
    pub kind: String,
    pub lang: String,
    pub score: f32,
}

/// Split identifiers into lowercase words: `parseHTTPRequest_v2` →
/// `parse http request v 2`. Non-identifier characters become spaces.
pub fn split_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        if i > 0 {
            let p = chars[i - 1];
            let next_lower = chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
            let boundary = (p.is_lowercase() && c.is_uppercase())
                || (p.is_alphabetic() && c.is_ascii_digit())
                || (p.is_ascii_digit() && c.is_alphabetic())
                || (p.is_uppercase() && c.is_uppercase() && next_lower);
            if boundary && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        out.extend(c.to_lowercase());
    }
    out.trim().to_string()
}

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "to", "in", "for", "and", "or", "is", "it", "its", "that", "this",
    "with", "from", "on", "by", "be", "are", "was", "how", "what", "which", "does", "do", "when",
    "where", "all", "only", "after", "before", "their", "at", "as", "into", "if", "then", "so",
    "can", "should", "i", "we", "you", "find", "code", "function", "method", "given", "some",
    "any", "them", "they", "there", "has", "have", "were", "been", "being", "did",
];

/// Turn free text into a list of search terms.
fn prepare_query(q: &str) -> String {
    let mut terms: Vec<String> = Vec::new();
    // Keep the raw identifiers too, so exact names still score on `name`.
    for raw in q.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if raw.len() > 1 && (raw.contains('_') || raw.chars().any(|c| c.is_uppercase())) {
            terms.push(raw.to_lowercase());
        }
    }
    for w in split_ident(q).split_whitespace() {
        if !STOPWORDS.contains(&w) && !terms.iter().any(|t| t == w) {
            terms.push(w.to_string());
        }
    }
    terms.join(" ")
}

/// Words used inside a symbol's source (identifiers split, comments,
/// strings), deduplicated and capped.
pub fn body_words(source: &str, max_words: usize) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tok in source.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if tok.len() < 2 || tok.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
            continue;
        }
        for w in split_ident(tok).split_whitespace() {
            if w.len() > 1 && !STOPWORDS.contains(&w) && seen.insert(w.to_string()) {
                out.push(w.to_string());
                if out.len() >= max_words {
                    return out.join(" ");
                }
            }
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAP: usize = 15_000_000;

    fn sym(name: &str, kind: &str, lang: &str, file: &str) -> Symbol {
        Symbol::new(lang, kind, name, format!("{}::{}", file, name), file, Default::default())
    }

    #[test]
    fn in_memory_index_and_search() {
        let search = SearchIndex::in_memory().unwrap();
        let mut writer = search.writer(HEAP).unwrap();
        search.index_symbol(&writer, &sym("process_data", "function", "python", "main.py"));
        search.index_symbol(&writer, &sym("Config", "struct", "rust", "config.rs"));
        writer.commit().unwrap();

        let hits = search.search("process_data", 10).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].name, "process_data");
        assert_eq!(hits[0].kind, "function");
        assert_eq!(hits[0].lang, "python");
    }

    #[test]
    fn search_returns_empty_for_no_match() {
        let search = SearchIndex::in_memory().unwrap();
        let mut writer = search.writer(HEAP).unwrap();
        search.index_symbol(&writer, &sym("hello", "function", "rust", "lib.rs"));
        writer.commit().unwrap();

        let hits = search.search("nonexistent_xyz", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn camel_case_and_docs_are_searchable() {
        let search = SearchIndex::in_memory().unwrap();
        let mut writer = search.writer(HEAP).unwrap();
        let mut s = sym("writeAtomically", "function", "typescript", "io.ts");
        s.doc = Some("Replace the file only after the new content is fully written.".into());
        search.index_symbol(&writer, &s);
        search.index_symbol(&writer, &sym("readConfig", "function", "typescript", "cfg.ts"));
        writer.commit().unwrap();

        assert_eq!(search.search("write atomically", 5).unwrap()[0].name, "writeAtomically");
        assert_eq!(search.search("replacing file contents", 5).unwrap()[0].name, "writeAtomically");
    }

    #[test]
    fn special_characters_do_not_error() {
        let search = SearchIndex::in_memory().unwrap();
        let mut writer = search.writer(HEAP).unwrap();
        search.index_symbol(&writer, &sym("new", "method", "rust", "store.rs"));
        writer.commit().unwrap();
        assert!(search.search("Store::new(", 5).is_ok());
        assert!(search.search("a+b \"c", 5).is_ok());
    }

    #[test]
    fn split_ident_cases() {
        assert_eq!(split_ident("parseHTTPRequest_v2"), "parse http request v 2");
        assert_eq!(split_ident("src/store.rs"), "src store rs");
        assert_eq!(split_ident("XMLParser"), "xml parser");
    }

    #[test]
    fn body_words_dedup_and_cap() {
        let w = body_words("let fooBar = foo_bar(); // fetch the bar", 3);
        assert_eq!(w, "let foo bar");
    }
}
