use scavenger::eval::{CorpusEntry, Thresholds, run_performance_checks, run_relevance_eval};
use std::path::PathBuf;

fn make_sample_corpus() -> Vec<CorpusEntry> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    vec![CorpusEntry {
        name: "sample_project".to_string(),
        path,
        tracked_files: Some(5),
    }]
}

#[test]
fn test_relevance_eval_runs() {
    let corpus = make_sample_corpus();
    let thresholds = Thresholds::default();
    let result = run_relevance_eval(&corpus, &thresholds);
    assert!(result.is_ok());
}

#[test]
fn test_relevance_eval_produces_results() {
    let corpus = make_sample_corpus();
    let thresholds = Thresholds::default();
    let results = run_relevance_eval(&corpus, &thresholds).unwrap();
    assert!(!results.is_empty());
}

#[test]
fn test_relevance_eval_computes_recall() {
    let corpus = make_sample_corpus();
    let thresholds = Thresholds::default();
    let results = run_relevance_eval(&corpus, &thresholds).unwrap();
    // Should have multiple test cases from the eval/cases/relevance/ directory
    assert!(!results.is_empty(), "Expected at least one result");
    // Check that recall metric exists for each result
    for result in &results {
        assert!(
            result.metrics.contains_key("recall"),
            "Expected recall metric"
        );
    }
}

#[test]
fn test_performance_checks_runs() {
    let corpus = make_sample_corpus();
    let thresholds = Thresholds::default();
    let result = run_performance_checks(&corpus, &thresholds);
    assert!(result.is_ok());
}
