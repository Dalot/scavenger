use criterion::{Criterion, criterion_group, criterion_main};
use scavenger::query::intent::classify;

fn bench_intent_classification(c: &mut Criterion) {
    c.bench_function("intent_classify_find_callers", |b| {
        b.iter(|| classify("who calls process_message"))
    });

    c.bench_function("intent_classify_debug", |b| {
        b.iter(|| classify("why is this function broken"))
    });

    c.bench_function("intent_classify_refactor", |b| {
        b.iter(|| classify("rename this function to handle_request"))
    });
}

criterion_group!(benches, bench_intent_classification);
criterion_main!(benches);
