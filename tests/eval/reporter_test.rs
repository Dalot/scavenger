use scavenger::eval::{CaseResult, Correctness, EvalTier};
use scavenger::eval::{print_json, print_summary, run_suite};
use std::collections::HashMap;

fn make_test_results() -> Vec<CaseResult> {
    vec![
        CaseResult {
            case_name: "test-pass".to_string(),
            category: "test".to_string(),
            metrics: HashMap::from([("recall".to_string(), 0.9)]),
            correctness: Correctness::Partial,
            passed: true,
            failure_reason: None,
            bm25_recall: 0.8,
            graph_recall: 0.9,
        },
        CaseResult {
            case_name: "test-fail".to_string(),
            category: "test".to_string(),
            metrics: HashMap::from([("recall".to_string(), 0.5)]),
            correctness: Correctness::Incorrect,
            passed: false,
            failure_reason: Some("below threshold".to_string()),
            bm25_recall: 0.3,
            graph_recall: 0.5,
        },
    ]
}

#[test]
fn test_run_suite_computes_summary() {
    let results = make_test_results();
    let run = run_suite(EvalTier::Component, "relevance", "sample", results);

    assert_eq!(run.summary.total_cases, 2);
    assert_eq!(run.summary.passed, 1);
    assert_eq!(run.summary.failed, 1);
    assert!((run.summary.averages.get("recall").unwrap() - 0.7).abs() < 0.01);
}

#[test]
fn test_run_suite_empty_results() {
    let run = run_suite(EvalTier::Component, "relevance", "sample", vec![]);

    assert_eq!(run.summary.total_cases, 0);
    assert_eq!(run.summary.passed, 0);
    assert_eq!(run.summary.failed, 0);
    assert!(run.summary.averages.is_empty());
}

#[test]
fn test_print_json_output() {
    let results = make_test_results();
    let run = run_suite(EvalTier::Component, "relevance", "sample", results);

    let result = print_json(&run);
    assert!(result.is_ok());
}

#[test]
fn test_print_summary_does_not_panic() {
    let results = make_test_results();
    let run = run_suite(EvalTier::Component, "relevance", "sample", results);

    print_summary(&run);
}
