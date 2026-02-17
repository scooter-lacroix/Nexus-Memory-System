//! Benchmarks for the embedding service

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use nexus_embeddings::{MockEmbeddingService, EMBEDDING_DIMENSION};
use nexus_core::traits::EmbeddingService;

fn bench_single_embedding(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let service = MockEmbeddingService::new();

    let text = "This is a sample text for benchmarking the embedding service.";

    c.bench_function("single_embed_mock", |b| {
        b.to_async(&rt).iter(|| async {
            service.embed(black_box(text)).await.unwrap()
        });
    });
}

fn bench_batch_embedding(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let service = MockEmbeddingService::new();

    let mut group = c.benchmark_group("batch_embed");

    for size in [1, 5, 10, 50, 100].iter() {
        let texts: Vec<String> = (0..*size)
            .map(|i| format!("Sample text number {} for batch embedding benchmark", i))
            .collect();

        group.bench_with_input(BenchmarkId::new("mock", size), size, |b, _| {
            b.to_async(&rt).iter(|| async {
                service.embed_batch(black_box(&texts)).await.unwrap()
            });
        });
    }

    group.finish();
}

fn bench_embedding_dimension(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("dimension");

    for dim in [128, 256, 384, 512, 768, 1024].iter() {
        let service = MockEmbeddingService::with_dimension(*dim);
        let text = "Text for dimension benchmark";

        group.bench_with_input(BenchmarkId::new("dim", dim), dim, |b, _| {
            b.to_async(&rt).iter(|| async {
                service.embed(black_box(text)).await.unwrap()
            });
        });
    }

    group.finish();
}

fn bench_cosine_similarity(c: &mut Criterion) {
    let service = MockEmbeddingService::new();
    let rt = tokio::runtime::Runtime::new().unwrap();

    let e1 = rt.block_on(async { service.embed("text one").await.unwrap() });
    let e2 = rt.block_on(async { service.embed("text two").await.unwrap() });

    c.bench_function("cosine_similarity", |b| {
        b.iter(|| {
            MockEmbeddingService::cosine_similarity(
                black_box(&e1),
                black_box(&e2)
            )
        });
    });
}

criterion_group!(
    benches,
    bench_single_embedding,
    bench_batch_embedding,
    bench_embedding_dimension,
    bench_cosine_similarity,
);

criterion_main!(benches);
