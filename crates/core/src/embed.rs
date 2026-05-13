//! # Vector Embeddings (HNSW + Fastembed)
//!
//! Two modes:
//! - **fastembed** (optional feature): BGE-small-en-v1.5 transformer embeddings
//! - **hash fallback**: BLAKE3-based pseudo-embeddings for zero-dependency builds

use crate::model::SymbolId;
use std::path::Path;

/// Dimensionality for BLAKE3 hash embeddings (aligned to 32).
const HASH_DIM: usize = 32;

/// Trait for embedding providers.
pub trait Embedder: Send + Sync {
    /// Dimension of the embedding vectors.
    fn dim(&self) -> usize;

    /// Embed a single text string.
    fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>>;

    /// Embed a batch of text strings.
    fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// BLAKE3 hash-based pseudo-embedder. Zero dependencies, deterministic.
pub struct HashEmbedder;

impl Embedder for HashEmbedder {
    fn dim(&self) -> usize {
        HASH_DIM
    }

    fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let hash = blake3::hash(text.as_bytes());
        let bytes = hash.as_bytes();
        Ok(bytes.iter().map(|&b| (b as f32) / 255.0).collect())
    }

    fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed_one(t)).collect()
    }
}

/// Fastembed transformer-based embedder (optional feature).
#[cfg(feature = "fastembed")]
pub struct TransformerEmbedder {
    model: fastembed::TextEmbedding,
}

#[cfg(feature = "fastembed")]
impl TransformerEmbedder {
    pub fn new() -> anyhow::Result<Self> {
        let model = fastembed::TextEmbedding::try_new(
            fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15)
                .with_show_download_progress(true),
        )?;
        Ok(Self { model })
    }
}

#[cfg(feature = "fastembed")]
impl Embedder for TransformerEmbedder {
    fn dim(&self) -> usize {
        384 // BGE-small-en-v1.5 dimension
    }

    fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let results = self.model.embed(vec![text.to_string()], None)?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty embedding result"))
    }

    fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        let results = self.model.embed(owned, None)?;
        Ok(results)
    }
}

/// HNSW vector index for approximate nearest-neighbor search.
pub struct VectorIndex {
    dim: usize,
    ids: Vec<SymbolId>,
    vectors: Vec<Vec<f32>>,
    // In a full implementation, this would use hnsw_rs::hnsw::Hnsw
    // For now, brute-force for correctness, HNSW for production.
}

impl VectorIndex {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            ids: Vec::new(),
            vectors: Vec::new(),
        }
    }

    /// Insert a vector with its symbol ID.
    pub fn insert(&mut self, id: SymbolId, vector: Vec<f32>) {
        debug_assert_eq!(vector.len(), self.dim);
        self.ids.push(id);
        self.vectors.push(vector);
    }

    /// Search for the k nearest neighbors to a query vector.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(SymbolId, f32)> {
        let mut scored: Vec<(SymbolId, f32)> = self
            .ids
            .iter()
            .zip(self.vectors.iter())
            .map(|(&id, vec)| {
                let sim = cosine_similarity(query, vec);
                (id, sim)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    /// Number of vectors in the index.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Save to disk.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let data = bincode::serialize(&(&self.dim, &self.ids, &self.vectors))?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Load from disk.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        let (dim, ids, vectors): (usize, Vec<SymbolId>, Vec<Vec<f32>>) =
            bincode::deserialize(&data)?;
        Ok(Self { dim, ids, vectors })
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Create the appropriate embedder based on feature flags.
pub fn create_embedder() -> Box<dyn Embedder> {
    #[cfg(feature = "fastembed")]
    {
        match TransformerEmbedder::new() {
            Ok(e) => return Box::new(e),
            Err(e) => {
                tracing::warn!("Failed to load fastembed model, falling back to hash: {e}");
            }
        }
    }
    Box::new(HashEmbedder)
}
