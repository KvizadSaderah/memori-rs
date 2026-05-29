/// Criterion benchmark: recall latency at 100, 1 000, and 10 000 stored memories.
///
/// Target NFR (DESIGN §3.3): p95 ≤ 100 ms at 10k records.
/// Run with: `cargo bench -p memori-core`
///
/// Note: the first run downloads BAAI/bge-small-en-v1.5 into .fastembed_cache/ (~25 MB).
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use memori_core::{Memory, Query};
use std::time::Duration;
use tokio::runtime::Runtime;

fn seed(rt: &Runtime, mem: &Memory, n: usize) {
    rt.block_on(async {
        for i in 0..n {
            mem.store(
                format!("benchmark memory item {i}: Rust systems programming and memory safety"),
                vec![],
                None,
            )
            .await
            .expect("store");
        }
    });
}

fn bench_recall(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("recall");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(20);

    for &n in &[100usize, 1_000, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let mem = rt.block_on(Memory::open(dir.path())).unwrap();
        seed(&rt, &mem, n);

        group.bench_with_input(BenchmarkId::new("knn_top5", n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                mem.recall(Query {
                    text: "memory safety Rust ownership".into(),
                    top_k: 5,
                    tag_filter: vec![],
                    source_filter: None,
                })
                .await
                .expect("recall")
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_recall);
criterion_main!(benches);
