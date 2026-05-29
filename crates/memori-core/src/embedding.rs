use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use once_cell::sync::OnceCell;

use crate::MemoriError;

pub const EMBEDDING_DIM: usize = 384;

static EMBEDDER: OnceCell<TextEmbedding> = OnceCell::new();

fn get_embedder() -> Result<&'static TextEmbedding, MemoriError> {
    EMBEDDER.get_or_try_init(|| {
        TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15).with_show_download_progress(false),
        )
        .map_err(|e| MemoriError::Embedding(e.to_string()))
    })
}

pub fn embed(text: &str) -> Result<Vec<f32>, MemoriError> {
    let embedder = get_embedder()?;
    let mut results = embedder
        .embed(vec![text.to_string()], None)
        .map_err(|e| MemoriError::Embedding(e.to_string()))?;

    let vec = results
        .pop()
        .ok_or_else(|| MemoriError::Embedding("no embedding returned".into()))?;

    if vec.len() != EMBEDDING_DIM {
        return Err(MemoriError::Embedding(format!(
            "dimension mismatch: got {}, expected {EMBEDDING_DIM}",
            vec.len()
        )));
    }

    Ok(vec)
}
