use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
    TimestampMicrosecondArray, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use lancedb::{
    Table, connect,
    query::{ExecutableQuery, QueryBase},
};
use uuid::Uuid;

use crate::{
    MemoriError,
    embedding::EMBEDDING_DIM,
    model::{ForgetFilter, MemoryRecord, RecallResult},
};

pub const TABLE_NAME: &str = "memories";

pub fn memories_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBEDDING_DIM as i32,
            ),
            false,
        ),
        Field::new(
            "created_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("tags", DataType::Utf8, false),
        Field::new("source", DataType::Utf8, true),
    ]))
}

pub struct Storage {
    table: Table,
}

impl Storage {
    pub async fn open(data_dir: &Path) -> Result<Self, MemoriError> {
        let db_path = data_dir.join("memories.lance");
        let db_path_str = db_path.to_string_lossy().to_string();

        let conn = connect(&db_path_str).execute().await?;
        let table_names = conn.table_names().execute().await?;

        let table = if table_names.contains(&TABLE_NAME.to_string()) {
            conn.open_table(TABLE_NAME).execute().await?
        } else {
            // Create with a single empty batch to establish the schema
            let schema = memories_schema();
            let empty_batch = RecordBatch::new_empty(schema.clone());
            conn.create_table(TABLE_NAME, empty_batch).execute().await?
        };

        Ok(Self { table })
    }

    pub async fn insert(&self, record: &MemoryRecord) -> Result<(), MemoriError> {
        let schema = memories_schema();
        let tags_json = serde_json::to_string(&record.tags)?;

        let embedding_array = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            std::iter::once(Some(
                record
                    .embedding
                    .iter()
                    .copied()
                    .map(Some)
                    .collect::<Vec<_>>(),
            )),
            EMBEDDING_DIM as i32,
        );

        let source_arr: StringArray = StringArray::from(vec![record.source.as_deref()]);

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![record.id.to_string()])),
                Arc::new(StringArray::from(vec![record.content.clone()])),
                Arc::new(embedding_array),
                Arc::new(TimestampMicrosecondArray::with_timezone_opt(
                    vec![record.created_at.timestamp_micros()].into(),
                    Some("UTC"),
                )),
                Arc::new(StringArray::from(vec![tags_json])),
                Arc::new(source_arr),
            ],
        )?;

        // Use Box<dyn RecordBatchReader + Send> which implements Scannable
        let reader: Box<dyn arrow_array::RecordBatchReader + Send> =
            Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema));
        self.table.add(reader).execute().await?;
        Ok(())
    }

    pub async fn knn_search(
        &self,
        query_vec: Vec<f32>,
        top_k: usize,
        tag_filter: &[String],
        source_filter: Option<&str>,
    ) -> Result<Vec<RecallResult>, MemoriError> {
        let mut q = self
            .table
            .query()
            .nearest_to(query_vec)?
            .limit(top_k)
            .column("embedding");

        let filter = build_filter(tag_filter, source_filter);
        if let Some(f) = filter {
            q = q.only_if(f);
        }

        let batches = collect_stream(q.execute().await?).await?;

        let mut results = Vec::new();
        for batch in &batches {
            let records = batch_to_records(batch)?;
            let scores = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
                .map(|a| a.values().to_vec())
                .unwrap_or_else(|| vec![0.0f32; records.len()]);

            for (record, raw_dist) in records.into_iter().zip(scores.iter()) {
                let score = 1.0 / (1.0 + raw_dist);
                results.push(RecallResult { record, score });
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    pub async fn scan(
        &self,
        limit: usize,
        offset: usize,
        tag_filter: &[String],
        source_filter: Option<&str>,
    ) -> Result<Vec<MemoryRecord>, MemoriError> {
        let mut q = self
            .table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "id".into(),
                "content".into(),
                "created_at".into(),
                "tags".into(),
                "source".into(),
            ]))
            .limit(limit + offset);

        let filter = build_filter(tag_filter, source_filter);
        if let Some(f) = filter {
            q = q.only_if(f);
        }

        let batches = collect_stream(q.execute().await?).await?;

        let mut records = Vec::new();
        for batch in &batches {
            records.extend(batch_to_records_no_embedding(batch)?);
        }

        records.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Ok(records.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn get_by_id(&self, id: Uuid) -> Result<Option<MemoryRecord>, MemoriError> {
        let q = self
            .table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "id".into(),
                "content".into(),
                "created_at".into(),
                "tags".into(),
                "source".into(),
            ]))
            .only_if(format!("id = '{id}'"))
            .limit(1);

        let batches = collect_stream(q.execute().await?).await?;
        for batch in &batches {
            if let Some(r) = batch_to_records_no_embedding(batch)?.into_iter().next() {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }

    pub async fn delete_by_id(&self, id: Uuid) -> Result<u64, MemoriError> {
        let predicate = format!("id = '{id}'");
        let before = self.table.count_rows(None).await?;
        self.table.delete(&predicate).await?;
        let after = self.table.count_rows(None).await?;
        Ok(before.saturating_sub(after) as u64)
    }

    pub async fn delete_by_filter(&self, filter: &ForgetFilter) -> Result<u64, MemoriError> {
        let mut parts: Vec<String> = Vec::new();

        if let Some(older_than) = filter.older_than {
            parts.push(format!(
                "created_at < TIMESTAMP '{}'",
                older_than.format("%Y-%m-%dT%H:%M:%S")
            ));
        }
        for tag in &filter.tags {
            let escaped = tag.replace('\'', "''");
            parts.push(format!("tags LIKE '%\"{escaped}\"%'"));
        }
        if let Some(src) = &filter.source {
            let escaped = src.replace('\'', "''");
            parts.push(format!("source = '{escaped}'"));
        }

        if parts.is_empty() {
            return Err(MemoriError::InvalidInput(
                "forget filter is empty — supply at least one criterion".into(),
            ));
        }

        let predicate = parts.join(" AND ");
        let before = self.table.count_rows(None).await?;
        self.table.delete(&predicate).await?;
        let after = self.table.count_rows(None).await?;
        Ok(before.saturating_sub(after) as u64)
    }
}

async fn collect_stream(
    mut stream: lancedb::arrow::SendableRecordBatchStream,
) -> Result<Vec<RecordBatch>, MemoriError> {
    let mut batches = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(batch) => batches.push(batch),
            Err(e) => return Err(MemoriError::Storage(e)),
        }
    }
    Ok(batches)
}

fn build_filter(tag_filter: &[String], source_filter: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for tag in tag_filter {
        let escaped = tag.replace('\'', "''");
        parts.push(format!("tags LIKE '%\"{escaped}\"%'"));
    }
    if let Some(src) = source_filter {
        let escaped = src.replace('\'', "''");
        parts.push(format!("source = '{escaped}'"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" AND "))
    }
}

fn batch_to_records(batch: &RecordBatch) -> Result<Vec<MemoryRecord>, MemoriError> {
    let ids = col_utf8(batch, "id")?;
    let contents = col_utf8(batch, "content")?;
    let created_ats = col_ts(batch, "created_at")?;
    let tags_jsons = col_utf8(batch, "tags")?;
    let sources = col_utf8_opt(batch, "source");
    let embeddings = col_embedding(batch)?;

    let n = batch.num_rows();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(MemoryRecord {
            id: ids[i].parse().unwrap_or_else(|_| Uuid::nil()),
            content: contents[i].to_string(),
            created_at: DateTime::from_timestamp_micros(created_ats[i])
                .unwrap_or_default()
                .with_timezone(&Utc),
            tags: serde_json::from_str(tags_jsons[i]).unwrap_or_default(),
            source: sources.as_ref().and_then(|s| s[i].map(|v| v.to_string())),
            embedding: embeddings[i].clone(),
        });
    }
    Ok(out)
}

fn batch_to_records_no_embedding(batch: &RecordBatch) -> Result<Vec<MemoryRecord>, MemoriError> {
    let ids = col_utf8(batch, "id")?;
    let contents = col_utf8(batch, "content")?;
    let created_ats = col_ts(batch, "created_at")?;
    let tags_jsons = col_utf8(batch, "tags")?;
    let sources = col_utf8_opt(batch, "source");

    let n = batch.num_rows();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(MemoryRecord {
            id: ids[i].parse().unwrap_or_else(|_| Uuid::nil()),
            content: contents[i].to_string(),
            created_at: DateTime::from_timestamp_micros(created_ats[i])
                .unwrap_or_default()
                .with_timezone(&Utc),
            tags: serde_json::from_str(tags_jsons[i]).unwrap_or_default(),
            source: sources.as_ref().and_then(|s| s[i].map(|v| v.to_string())),
            embedding: Vec::new(),
        });
    }
    Ok(out)
}

fn col_utf8<'a>(batch: &'a RecordBatch, name: &str) -> Result<Vec<&'a str>, MemoriError> {
    let col = batch
        .column_by_name(name)
        .ok_or_else(|| MemoriError::InvalidInput(format!("missing column: {name}")))?;
    let arr = col
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| MemoriError::InvalidInput(format!("column {name} is not Utf8")))?;
    Ok((0..arr.len()).map(|i| arr.value(i)).collect())
}

fn col_utf8_opt<'a>(batch: &'a RecordBatch, name: &str) -> Option<Vec<Option<&'a str>>> {
    let col = batch.column_by_name(name)?;
    let arr = col.as_any().downcast_ref::<StringArray>()?;
    Some(
        (0..arr.len())
            .map(|i| {
                if arr.is_null(i) {
                    None
                } else {
                    Some(arr.value(i))
                }
            })
            .collect(),
    )
}

fn col_ts(batch: &RecordBatch, name: &str) -> Result<Vec<i64>, MemoriError> {
    let col = batch
        .column_by_name(name)
        .ok_or_else(|| MemoriError::InvalidInput(format!("missing column: {name}")))?;
    let arr = col
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
        .ok_or_else(|| MemoriError::InvalidInput(format!("column {name} is not timestamp")))?;
    Ok((0..arr.len()).map(|i| arr.value(i)).collect())
}

fn col_embedding(batch: &RecordBatch) -> Result<Vec<Vec<f32>>, MemoriError> {
    let col = batch
        .column_by_name("embedding")
        .ok_or_else(|| MemoriError::InvalidInput("missing column: embedding".into()))?;
    let arr = col
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .ok_or_else(|| MemoriError::InvalidInput("embedding is not FixedSizeList".into()))?;

    let mut out = Vec::with_capacity(arr.len());
    for i in 0..arr.len() {
        let vals = arr
            .value(i)
            .as_any()
            .downcast_ref::<Float32Array>()
            .ok_or_else(|| MemoriError::InvalidInput("embedding values are not f32".into()))?
            .values()
            .to_vec();
        out.push(vals);
    }
    Ok(out)
}
