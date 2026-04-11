use scavenger::eval::{EvalOptions, EvalSuite, EvalTier, run_evals};
use std::path::PathBuf;

#[test]
fn test_run_evals_default_options() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let opts = EvalOptions {
        suites: vec![EvalSuite::Relevance],
        tier: EvalTier::Component,
        corpus_path: Some(fixtures.to_string_lossy().to_string()),
        json: false,
        thresholds_path: None,
        ..Default::default()
    };

    let result = run_evals(&opts);
    // Should succeed even with stub implementations returning empty results
    assert!(result.is_ok());
}

#[test]
fn test_run_evals_no_corpus() {
    let opts = EvalOptions {
        suites: vec![EvalSuite::Relevance],
        tier: EvalTier::Component,
        corpus_path: Some("/nonexistent/path".to_string()),
        ..Default::default()
    };

    let result = run_evals(&opts);
    assert!(result.is_err());
}

#[test]
fn test_run_evals_multiple_suites() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let opts = EvalOptions {
        suites: vec![EvalSuite::Relevance, EvalSuite::Accuracy],
        tier: EvalTier::Component,
        corpus_path: Some(fixtures.to_string_lossy().to_string()),
        json: false,
        thresholds_path: None,
        ..Default::default()
    };

    let result = run_evals(&opts);
    assert!(result.is_ok());
    let runs = result.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].summary.suite_name, "relevance");
    assert_eq!(runs[1].summary.suite_name, "accuracy");
}

#[test]
fn test_run_evals_all_tiers() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let opts = EvalOptions {
        suites: vec![EvalSuite::Relevance],
        tier: EvalTier::All,
        corpus_path: Some(fixtures.to_string_lossy().to_string()),
        json: false,
        thresholds_path: None,
        ..Default::default()
    };

    let result = run_evals(&opts);
    assert!(result.is_ok());
}
