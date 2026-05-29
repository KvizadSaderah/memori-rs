use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<String>,
    pub source: Option<String>,
    #[serde(skip)]
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub text: String,
    pub top_k: usize,
    pub tag_filter: Vec<String>,
    pub source_filter: Option<String>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            top_k: 5,
            tag_filter: Vec::new(),
            source_filter: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallResult {
    pub record: MemoryRecord,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ForgetFilter {
    pub older_than: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub source: Option<String>,
}
