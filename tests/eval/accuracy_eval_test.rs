use scavenger::eval::{CorpusEntry, Thresholds, run_accuracy_eval};
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
fn test_accuracy_eval_runs() {
    let corpus = make_sample_corpus();
    let thresholds = Thresholds::default();
    let result = run_accuracy_eval(&corpus, &thresholds);
    assert!(result.is_ok());
}

#[test]
fn test_accuracy_eval_intent_classification() {
    let corpus = make_sample_corpus();
    let thresholds = Thresholds::default();
    let results = run_accuracy_eval(&corpus, &thresholds).unwrap();
    let intent_cases: Vec<_> = results
        .iter()
        .filter(|r| r.case_name.starts_with("intent-"))
        .collect();
    assert!(!intent_cases.is_empty());
}
