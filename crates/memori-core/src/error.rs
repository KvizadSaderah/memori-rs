use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoriError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("storage error: {0}")]
    Storage(#[from] lancedb::Error),

    #[error("embedding error: {0}")]
    Embedding(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
}
