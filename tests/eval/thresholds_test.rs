use scavenger::eval::{RelevanceMetric, Thresholds, load_thresholds};
use std::path::PathBuf;

#[test]
fn test_load_default_thresholds() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/thresholds.toml");
    let result = load_thresholds(&path);
    assert!(result.is_ok());
    let t = result.unwrap();
    // Verify values match what's in eval/thresholds.toml
    assert_eq!(t.relevance.min_recall, 0.80);
    assert_eq!(t.relevance.min_precision, 0.60);
    assert_eq!(t.accuracy.min_intent_accuracy, 0.90);
    assert_eq!(t.performance.max_capsule_latency_p95_ms, 200);
}

#[test]
fn test_threshold_check_pass() {
    let t = Thresholds::default();
    assert!(t.relevance.passes(RelevanceMetric::Recall, 0.90));
    assert!(t.relevance.passes(RelevanceMetric::Precision, 0.70));
}

#[test]
fn test_threshold_check_fail() {
    let t = Thresholds::default();
    assert!(!t.relevance.passes(RelevanceMetric::Recall, 0.50));
    assert!(!t.relevance.passes(RelevanceMetric::Precision, 0.30));
}

#[test]
fn test_threshold_missing_file() {
    let result = load_thresholds(&PathBuf::from("/nonexistent/thresholds.toml"));
    assert!(result.is_ok());
    // Should return defaults
    let t = result.unwrap();
    assert_eq!(t.relevance.min_recall, 0.80);
}

#[test]
fn test_load_thresholds_custom_values() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let custom_path = tmp_dir.path().join("custom_thresholds.toml");
    std::fs::write(
        &custom_path,
        r#"
[relevance]
min_recall = 0.95
min_precision = 0.85
min_correct_rate = 0.90
max_incorrect_rate = 0.05

[accuracy]
min_intent_accuracy = 0.99
min_ndcg_at_5 = 0.90

[performance]
max_index_time_per_100_files_ms = 1000
max_capsule_latency_p95_ms = 50
max_reindex_time_ms = 100

[agent]
min_token_reduction_pct = 50
min_success_rate = 0.95
"#,
    )
    .unwrap();

    let result = load_thresholds(&custom_path).unwrap();
    assert_eq!(result.relevance.min_recall, 0.95);
    assert_eq!(result.relevance.min_precision, 0.85);
    assert_eq!(result.accuracy.min_intent_accuracy, 0.99);
    assert_eq!(result.performance.max_capsule_latency_p95_ms, 50);
    assert_eq!(result.agent.min_token_reduction_pct, 50.0);

    // tmp_dir automatically cleaned up when dropped
}
