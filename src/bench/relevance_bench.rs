use criterion::{Criterion, criterion_group, criterion_main};

fn bench_relevance_metrics(c: &mut Criterion) {
    c.bench_function("recall_calculation", |b| {
        b.iter(|| {
            let found = 8;
            let total = 10;
            found as f64 / total as f64
        })
    });
}

criterion_group!(benches, bench_relevance_metrics);
criterion_main!(benches);
