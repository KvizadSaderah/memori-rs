use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    MemoriError,
    embedding::embed,
    model::{ForgetFilter, MemoryRecord, Query, RecallResult},
    storage::Storage,
};

pub struct Memory {
    storage: Storage,
}

impl Memory {
    pub async fn open(data_dir: &Path) -> Result<Self, MemoriError> {
        let storage = Storage::open(data_dir).await?;
        Ok(Self { storage })
    }

    pub fn default_data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("memori")
    }

    pub async fn store(
        &self,
        content: impl Into<String>,
        tags: Vec<String>,
        source: Option<String>,
    ) -> Result<MemoryRecord, MemoriError> {
        let content = content.into();
        validate_content(&content)?;
        validate_tags(&tags)?;

        let embedding = embed(&content)?;
        let record = MemoryRecord {
            id: Uuid::new_v4(),
            content,
            created_at: now_micros(),
            tags,
            source,
            embedding,
        };

        self.storage.insert(&record).await?;
        Ok(record)
    }

    pub async fn recall(&self, query: Query) -> Result<Vec<RecallResult>, MemoriError> {
        validate_query(&query)?;
        let query_vec = embed(&query.text)?;
        self.storage
            .knn_search(
                query_vec,
                query.top_k,
                &query.tag_filter,
                query.source_filter.as_deref(),
            )
            .await
    }

    pub async fn list(
        &self,
        limit: usize,
        cursor: Option<&str>,
        tag_filter: Vec<String>,
        source_filter: Option<String>,
    ) -> Result<(Vec<MemoryRecord>, Option<String>), MemoriError> {
        let limit = if limit == 0 { 20 } else { limit };
        if limit > 100 {
            return Err(MemoriError::InvalidInput(
                "limit must not exceed 100".into(),
            ));
        }

        let offset = if let Some(c) = cursor {
            let bytes = B64
                .decode(c)
                .map_err(|_| MemoriError::InvalidInput("cursor is invalid".into()))?;
            let s = String::from_utf8(bytes)
                .map_err(|_| MemoriError::InvalidInput("cursor is invalid".into()))?;
            s.parse::<usize>()
                .map_err(|_| MemoriError::InvalidInput("cursor is invalid".into()))?
        } else {
            0
        };

        let records = self
            .storage
            .scan(limit + 1, offset, &tag_filter, source_filter.as_deref())
            .await?;

        let has_more = records.len() > limit;
        let records: Vec<_> = records.into_iter().take(limit).collect();
        let next_cursor = if has_more {
            Some(B64.encode((offset + limit).to_string()))
        } else {
            None
        };

        Ok((records, next_cursor))
    }

    /// Replace the content of an existing memory in place, re-embedding it.
    /// The id, creation time, tags and source are preserved.
    pub async fn update(
        &self,
        id: Uuid,
        content: impl Into<String>,
    ) -> Result<MemoryRecord, MemoriError> {
        let content = content.into();
        validate_content(&content)?;

        let existing = self
            .storage
            .get_by_id(id)
            .await?
            .ok_or_else(|| MemoriError::InvalidInput(format!("no memory with id {id}")))?;

        let embedding = embed(&content)?;
        let record = MemoryRecord {
            id,
            content,
            created_at: existing.created_at,
            tags: existing.tags,
            source: existing.source,
            embedding,
        };

        // Delete-then-insert keeps the same id/created_at (LanceDB has no
        // in-place update for our use here).
        self.storage.delete_by_id(id).await?;
        self.storage.insert(&record).await?;
        Ok(record)
    }

    pub async fn forget(
        &self,
        id: Option<Uuid>,
        filter: Option<ForgetFilter>,
    ) -> Result<u64, MemoriError> {
        match (id, filter) {
            (None, None) => Err(MemoriError::InvalidInput("must supply id or filter".into())),
            (Some(_), Some(_)) => Err(MemoriError::InvalidInput(
                "supply id or filter, not both".into(),
            )),
            (Some(id), None) => self.storage.delete_by_id(id).await,
            (None, Some(f)) => self.storage.delete_by_filter(&f).await,
        }
    }
}

/// Current UTC time truncated to microsecond precision.
///
/// LanceDB persists `created_at` as `Timestamp(Microsecond)`, so any
/// sub-microsecond component of `Utc::now()` (nanoseconds on Linux) is lost on
/// the storage round-trip. Truncating at creation keeps the in-memory record
/// bit-for-bit equal to what `update`/`list` read back.
fn now_micros() -> chrono::DateTime<Utc> {
    let now = Utc::now();
    chrono::DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}

fn validate_content(content: &str) -> Result<(), MemoriError> {
    if content.trim().is_empty() {
        return Err(MemoriError::InvalidInput(
            "content must not be empty".into(),
        ));
    }
    if content.len() > 65536 {
        return Err(MemoriError::InvalidInput(
            "content exceeds 64 KiB limit".into(),
        ));
    }
    Ok(())
}

fn validate_tags(tags: &[String]) -> Result<(), MemoriError> {
    for tag in tags {
        if tag.is_empty() {
            return Err(MemoriError::InvalidInput("tag must not be empty".into()));
        }
    }
    Ok(())
}

fn validate_query(query: &Query) -> Result<(), MemoriError> {
    if query.text.trim().is_empty() {
        return Err(MemoriError::InvalidInput(
            "query text must not be empty".into(),
        ));
    }
    if query.top_k == 0 || query.top_k > 25 {
        return Err(MemoriError::InvalidInput("top_k must be in [1, 25]".into()));
    }
    Ok(())
}
