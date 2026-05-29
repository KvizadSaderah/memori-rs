/// Integration tests for the Memory facade.
///
/// Each test opens a fresh LanceDB in a tempdir — no side-effects on ~/.memori.
/// Embedding is real (BGE-small-en-v1.5 ONNX); model is cached per-process via OnceCell.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::{ForgetFilter, Memory, Query};
    use chrono::Utc;

    async fn fresh_memory() -> (Memory, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mem = Memory::open(dir.path()).await.expect("Memory::open");
        (mem, dir)
    }

    // ── update ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn update_preserves_id_metadata_and_reembeds() {
        let (mem, _dir) = fresh_memory().await;

        let original = mem
            .store("cats are mammals", vec!["bio".into()], Some("seed".into()))
            .await
            .expect("store");

        let updated = mem
            .update(original.id, "the capital of France is Paris")
            .await
            .expect("update");

        // id, created_at, tags, source preserved
        assert_eq!(updated.id, original.id);
        assert_eq!(updated.created_at, original.created_at);
        assert_eq!(updated.tags, vec!["bio"]);
        assert_eq!(updated.source.as_deref(), Some("seed"));
        assert_eq!(updated.content, "the capital of France is Paris");

        // exactly one record remains (delete-then-insert, no duplicate)
        let (all, _) = mem.list(100, None, vec![], None).await.expect("list");
        assert_eq!(all.len(), 1);

        // recall reflects the new content, not the old
        let hits = mem
            .recall(Query {
                text: "France Paris capital".into(),
                top_k: 3,
                tag_filter: vec![],
                source_filter: None,
            })
            .await
            .expect("recall");
        assert_eq!(hits[0].record.id, original.id);
        assert_eq!(hits[0].record.content, "the capital of France is Paris");
    }

    #[tokio::test]
    async fn update_nonexistent_id_errors() {
        let (mem, _dir) = fresh_memory().await;
        let res = mem.update(uuid::Uuid::new_v4(), "whatever").await;
        assert!(res.is_err(), "updating a missing id must error");
    }

    #[tokio::test]
    async fn update_empty_content_rejected() {
        let (mem, _dir) = fresh_memory().await;
        let r = mem.store("seed", vec![], None).await.expect("store");
        assert!(mem.update(r.id, "   ").await.is_err());
    }

    // ── store / recall round-trip ───────────────────────────────────────────

    #[tokio::test]
    async fn store_recall_round_trip() {
        let (mem, _dir) = fresh_memory().await;

        let record = mem
            .store("The quick brown fox jumps over the lazy dog", vec![], None)
            .await
            .expect("store");

        let results = mem
            .recall(Query {
                text: "quick fox".to_string(),
                top_k: 5,
                tag_filter: vec![],
                source_filter: None,
            })
            .await
            .expect("recall");

        assert!(!results.is_empty(), "recall returned no results");
        assert_eq!(
            results[0].record.id, record.id,
            "top result should be the stored record"
        );
        assert!(
            results[0].score > 0.5,
            "score should be reasonably high for matching text"
        );
    }

    #[tokio::test]
    async fn store_preserves_metadata() {
        let (mem, _dir) = fresh_memory().await;

        let before = Utc::now();
        let record = mem
            .store(
                "Rust is a systems programming language",
                vec!["lang".into(), "systems".into()],
                Some("test-agent".into()),
            )
            .await
            .expect("store");
        let after = Utc::now();

        assert_eq!(record.tags, vec!["lang", "systems"]);
        assert_eq!(record.source.as_deref(), Some("test-agent"));
        assert!(record.created_at >= before && record.created_at <= after);
        assert!(!record.id.is_nil());
    }

    // ── tag filtering ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn recall_tag_filter() {
        let (mem, _dir) = fresh_memory().await;

        mem.store("Rust memory safety", vec!["rust".into()], None)
            .await
            .unwrap();
        mem.store("Python garbage collection", vec!["python".into()], None)
            .await
            .unwrap();

        let results = mem
            .recall(Query {
                text: "memory management".to_string(),
                top_k: 5,
                tag_filter: vec!["rust".into()],
                source_filter: None,
            })
            .await
            .unwrap();

        assert!(
            results
                .iter()
                .all(|r| r.record.tags.contains(&"rust".to_string())),
            "tag filter should exclude non-rust results"
        );
    }

    // ── list + pagination ───────────────────────────────────────────────────

    #[tokio::test]
    async fn list_basic() {
        let (mem, _dir) = fresh_memory().await;

        for i in 0..5 {
            mem.store(format!("memory item {i}"), vec![], None)
                .await
                .unwrap();
        }

        let (items, cursor) = mem.list(3, None, vec![], None).await.unwrap();
        assert_eq!(items.len(), 3);
        assert!(cursor.is_some(), "should have next cursor");

        let (items2, cursor2) = mem.list(3, cursor.as_deref(), vec![], None).await.unwrap();
        assert_eq!(items2.len(), 2);
        assert!(cursor2.is_none(), "no more pages");
    }

    #[tokio::test]
    async fn list_limit_validation() {
        let (mem, _dir) = fresh_memory().await;

        let err = mem.list(101, None, vec![], None).await.unwrap_err();
        assert!(
            err.to_string().contains("100"),
            "should mention limit of 100"
        );
    }

    // ── forget by id ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn forget_by_id() {
        let (mem, _dir) = fresh_memory().await;

        let r = mem.store("delete me", vec![], None).await.unwrap();
        let deleted = mem.forget(Some(r.id), None).await.unwrap();
        assert_eq!(deleted, 1);

        let (items, _) = mem.list(10, None, vec![], None).await.unwrap();
        assert!(
            items.iter().all(|i| i.id != r.id),
            "deleted record should not appear in list"
        );
    }

    #[tokio::test]
    async fn forget_by_id_nonexistent_returns_zero() {
        let (mem, _dir) = fresh_memory().await;
        let deleted = mem.forget(Some(uuid::Uuid::new_v4()), None).await.unwrap();
        assert_eq!(deleted, 0);
    }

    // ── forget by filter ────────────────────────────────────────────────────

    #[tokio::test]
    async fn forget_by_tag_filter() {
        let (mem, _dir) = fresh_memory().await;

        mem.store("keep me", vec!["keep".into()], None)
            .await
            .unwrap();
        mem.store("delete me", vec!["old".into()], None)
            .await
            .unwrap();
        mem.store("delete me too", vec!["old".into()], None)
            .await
            .unwrap();

        let deleted = mem
            .forget(
                None,
                Some(ForgetFilter {
                    older_than: None,
                    tags: vec!["old".into()],
                    source: None,
                }),
            )
            .await
            .unwrap();
        assert_eq!(deleted, 2);

        let (items, _) = mem.list(10, None, vec![], None).await.unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].tags.contains(&"keep".to_string()));
    }

    #[tokio::test]
    async fn forget_by_source_filter() {
        let (mem, _dir) = fresh_memory().await;

        mem.store("from agent-a", vec![], Some("agent-a".into()))
            .await
            .unwrap();
        mem.store("from agent-b", vec![], Some("agent-b".into()))
            .await
            .unwrap();

        let deleted = mem
            .forget(
                None,
                Some(ForgetFilter {
                    older_than: None,
                    tags: vec![],
                    source: Some("agent-a".into()),
                }),
            )
            .await
            .unwrap();
        assert_eq!(deleted, 1);

        let (items, _) = mem.list(10, None, vec![], None).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source.as_deref(), Some("agent-b"));
    }

    // ── validation ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn store_empty_content_rejected() {
        let (mem, _dir) = fresh_memory().await;
        let err = mem.store("   ", vec![], None).await.unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn recall_empty_query_rejected() {
        let (mem, _dir) = fresh_memory().await;
        let err = mem
            .recall(Query {
                text: " ".into(),
                top_k: 5,
                tag_filter: vec![],
                source_filter: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn forget_no_args_rejected() {
        let (mem, _dir) = fresh_memory().await;
        let err = mem.forget(None, None).await.unwrap_err();
        assert!(err.to_string().contains("must supply"));
    }

    #[tokio::test]
    async fn forget_both_id_and_filter_rejected() {
        let (mem, _dir) = fresh_memory().await;
        let err = mem
            .forget(
                Some(uuid::Uuid::new_v4()),
                Some(ForgetFilter {
                    older_than: None,
                    tags: vec!["x".into()],
                    source: None,
                }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not both"));
    }
}
