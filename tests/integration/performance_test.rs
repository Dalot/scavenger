// T070: Performance validation — index time, capsule latency.

use scavenger::db;
use scavenger::graph::{GraphState, index};
use std::time::Instant;

#[test]
fn test_bulk_index_under_5_seconds() {
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // Generate ~50 Rust files with 10 functions each
    for i in 0..50 {
        let mut code = String::new();
        for j in 0..10 {
            code.push_str(&format!("fn func_{i}_{j}(x: i32) -> i32 {{ x + {j} }}\n"));
        }
        std::fs::write(src_dir.join(format!("mod_{i}.rs")), code).unwrap();
    }

    let files: Vec<_> = (0..50)
        .map(|i| src_dir.join(format!("mod_{i}.rs")))
        .collect();

    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();
    let conn = db::open_branch_db(&scav_dir, "main").unwrap();

    let start = Instant::now();
    let mut g = GraphState::new();
    let stats = index::bulk_index(&conn, &mut g, &files).unwrap();
    let elapsed = start.elapsed();

    assert!(
        stats.symbols_extracted >= 500,
        "expected >=500 symbols, got {}",
        stats.symbols_extracted
    );
    assert!(
        elapsed.as_secs() < 5,
        "bulk index took {elapsed:?}, expected < 5s"
    );
    eprintln!(
        "Bulk index: {} symbols in {elapsed:?}",
        stats.symbols_extracted
    );
}

#[test]
fn test_capsule_latency_under_200ms() {
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // Create a moderate-sized file
    let mut code = String::new();
    for i in 0..30 {
        code.push_str(&format!("fn func_{i}() -> i32 {{ {i} }}\n"));
    }
    std::fs::write(src_dir.join("main.rs"), &code).unwrap();

    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();
    let conn = db::open_branch_db(&scav_dir, "main").unwrap();
    let mut g = GraphState::new();
    index::bulk_index(&conn, &mut g, &[src_dir.join("main.rs")]).unwrap();

    let config = scavenger::config::Config::default();
    let file_str = src_dir.join("main.rs").to_string_lossy().to_string();

    let start = Instant::now();
    let qr = scavenger::query::run_query(&conn, &g, &config, &file_str, Some("func_0"), None);
    let capsule = scavenger::capsule::assemble(&conn, &g, &config, &qr, None);
    let elapsed = start.elapsed();

    assert!(!capsule.text.is_empty());
    assert!(
        elapsed.as_millis() < 200,
        "capsule generation took {elapsed:?}, expected < 200ms"
    );
    eprintln!("Capsule: {} tokens in {elapsed:?}", capsule.token_count);
}

#[test]
fn test_incremental_reindex_under_500ms() {
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let mut code = String::new();
    for i in 0..100 {
        code.push_str(&format!("fn func_{i}() -> i32 {{ {i} }}\n"));
    }
    let path = src_dir.join("big.rs");
    std::fs::write(&path, &code).unwrap();

    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();
    let conn = db::open_branch_db(&scav_dir, "main").unwrap();
    let mut g = GraphState::new();
    index::bulk_index(&conn, &mut g, &[path.clone()]).unwrap();

    // Modify and re-index
    code.push_str("fn func_new() -> i32 { 999 }\n");
    std::fs::write(&path, &code).unwrap();

    let file_str = path.to_string_lossy().to_string();
    let start = Instant::now();
    let prep = index::incremental_reindex_prep(&conn, &g, &file_str)
        .unwrap()
        .expect("file was modified, prep should not be None");
    let _stats = index::incremental_reindex_swap(&conn, &mut g, prep).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "incremental reindex took {elapsed:?}, expected < 500ms"
    );
    eprintln!("Incremental reindex: {elapsed:?}");
}
