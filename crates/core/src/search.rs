//! # Full-text Search (Tantivy BM25)
//!
//! Provides BM25 text search over symbol names, fqnames, signatures,
//! and file paths using Tantivy.

use crate::model::SymbolId;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};

/// Fields in the search index.
struct SearchFields {
    id: Field,
    name: Field,
    fqname: Field,
    file: Field,
    signature: Field,
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
        std::fs::create_dir_all(&dir)?;

        let index = Index::open_or_create(
            tantivy::directory::MmapDirectory::open(&dir)?,
            schema,
        )?;

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
        let mut builder = Schema::builder();
        builder.add_text_field("id", STRING | STORED);
        builder.add_text_field("name", TEXT | STORED);
        builder.add_text_field("fqname", TEXT | STORED);
        builder.add_text_field("file", TEXT | STORED);
        builder.add_text_field("signature", TEXT);
        builder.add_text_field("kind", STRING | STORED);
        builder.add_text_field("lang", STRING | STORED);
        builder.build()
    }

    fn get_fields(schema: &Schema) -> SearchFields {
        SearchFields {
            id: schema.get_field("id").unwrap(),
            name: schema.get_field("name").unwrap(),
            fqname: schema.get_field("fqname").unwrap(),
            file: schema.get_field("file").unwrap(),
            signature: schema.get_field("signature").unwrap(),
            kind: schema.get_field("kind").unwrap(),
            lang: schema.get_field("lang").unwrap(),
        }
    }

    /// Get a writer for batch indexing.
    pub fn writer(&self, heap_size: usize) -> anyhow::Result<IndexWriter> {
        Ok(self.index.writer(heap_size)?)
    }

    /// Index a single symbol.
    pub fn index_symbol(
        &self,
        writer: &IndexWriter,
        id: &SymbolId,
        name: &str,
        fqname: &str,
        file: &str,
        signature: Option<&str>,
        kind: &str,
        lang: &str,
    ) {
        let _ = writer.add_document(doc!(
            self.fields.id => id.to_hex(),
            self.fields.name => name,
            self.fields.fqname => fqname,
            self.fields.file => file,
            self.fields.signature => signature.unwrap_or(""),
            self.fields.kind => kind,
            self.fields.lang => lang,
        ));
    }

    /// Remove all documents for a given file path.
    pub fn remove_file(&self, writer: &IndexWriter, file_path: &str) {
        let term = tantivy::Term::from_field_text(self.fields.file, file_path);
        writer.delete_term(term);
    }

    /// Search for symbols matching a query string.
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchHit>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let searcher = reader.searcher();

        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.name,
                self.fields.fqname,
                self.fields.file,
                self.fields.signature,
            ],
        );

        let parsed = query_parser.parse_query(query)?;
        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(addr)?;

            let id_hex = doc
                .get_first(self.fields.id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = doc
                .get_first(self.fields.name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let file = doc
                .get_first(self.fields.file)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let kind = doc
                .get_first(self.fields.kind)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let lang = doc
                .get_first(self.fields.lang)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            hits.push(SearchHit {
                id: SymbolId::from_hex(&id_hex).unwrap_or_default(),
                name,
                file,
                kind,
                lang,
                score,
            });
        }

        Ok(hits)
    }
}

/// A search result.
#[derive(Debug)]
pub struct SearchHit {
    pub id: SymbolId,
    pub name: String,
    pub file: String,
    pub kind: String,
    pub lang: String,
    pub score: f32,
}
