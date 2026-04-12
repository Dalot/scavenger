use criterion::{Criterion, criterion_group, criterion_main};
use rusqlite::Connection;
use scavenger::capsule::assemble;
use scavenger::capsule::budget::CapsuleConstraints;
use scavenger::config::Config;
use scavenger::db::schema;
use scavenger::graph::GraphState;
use scavenger::graph::types::NodeId;
use scavenger::query::QueryResult;
use scavenger::query::intent::{Intent, IntentResult};

fn bench_capsule_generation(c: &mut Criterion) {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();

    conn.execute(
        "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
         VALUES ('n1', 'Function', 'hello', 'src/lib.rs', 1, 5, 'fn hello()', 'aabb0011', 'fn hello()', X'CAFE')",
        [],
    )
    .unwrap();

    let mut graph = GraphState::new();
    graph.load_from_db(&conn).unwrap();

    let config = Config::default();
    let qr = QueryResult {
        target: Some(NodeId("n1".to_string())),
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: Vec::new(),
        search_results: Vec::new(),
    };
    let constraints =
        CapsuleConstraints::from_detail(scavenger::capsule::budget::DetailLevel::Standard);

    c.bench_function("capsule_generation", |b| {
        b.iter(|| assemble(&conn, &graph, &config, &qr, None, &constraints))
    });
}

fn bench_incremental_reindex(c: &mut Criterion) {
    let conn = Connection::open_in_memory().unwrap();

    c.bench_function("incremental_reindex", |b| {
        b.iter(|| {
            let _ = schema::ensure_branch_schema(&conn);
        })
    });
}

criterion_group!(benches, bench_capsule_generation, bench_incremental_reindex);
criterion_main!(benches);
